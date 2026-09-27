// The Rust half of the extension, the deck's own code compiled to WASI: every
// request runs it once, reading the request from a file and writing the answer
// to another, both in JSON. Not on stdout, as vscode-wasm drops what is still
// queued there when the process exits, which a busy extension host can leave
// the whole answer.

import * as vscode from 'vscode';
import { MountPointDescriptor, Wasm } from '@vscode/wasm-wasi/v1';
import { canRunCells } from './kernel';
import type { Saved } from './output';

/** The memory the module imports, as `.cargo/config.toml` sizes it, in pages of 64 KiB. */
const MEMORY: WebAssembly.MemoryDescriptor = { initial: 256, maximum: 16384, shared: true };

/** Where the request is, in a file system of its own. */
const IO = '/io';
const REQUEST = 'request.json';

/** Where the answers are written, in the extension's storage. */
const ANSWERS = '/answers';

/** Where the disk holding a notebook's file is mounted. */
const DISK = '/fs';

export type Kind = 'markdown' | 'code';

/** A cell, as the module reads it from markdown and writes it back. */
export interface Cell {
	kind: Kind;
	source: string;
	language: string;
	/** How it was written, which the module needs back to write it the same. */
	written: unknown;
}

/** One representation of an output, its bytes in base64. */
export interface Item {
	mime: string;
	data: string;
}

/** A code cell and its outputs, to be saved. */
export interface CodeCell {
	source: string;
	outputs: { name: string | null; items: Item[] }[];
}

/** Where a notebook's file is: its directory as the deck hashes it, and as the module reads it. */
interface Place {
	dir: string;
	path: string;
}

export class Module {
	private loading: Promise<{ wasm: Wasm; module: WebAssembly.Module }> | undefined;
	/** The directory the answers are written in, and read from once the module exits. */
	private readonly answers: vscode.Uri;

	constructor(
		private readonly extensionUri: vscode.Uri,
		storageUri: vscode.Uri,
		private readonly log: vscode.LogOutputChannel,
	) {
		this.answers = vscode.Uri.joinPath(storageUri, 'answers');
	}

	/** The cells of a markdown file. */
	cells(markdown: string): Promise<Cell[]> {
		return this.run({ command: 'cells', markdown });
	}

	/** The markdown file the cells make. */
	markdown(cells: Cell[]): Promise<string> {
		return this.run({ command: 'markdown', cells });
	}

	/** The saved outputs of each code cell of the file at `uri`, given their sources in order. */
	async load(uri: vscode.Uri, sources: string[]): Promise<Saved[][]> {
		return this.run({ command: 'load', place: await place(uri), sources }, [disk(uri)]);
	}

	/** Saves the outputs of the code cells of the file at `uri`, returning how many cells it saved. */
	async save(uri: vscode.Uri, cells: CodeCell[]): Promise<number> {
		return this.run({ command: 'save', place: await place(uri), cells }, [disk(uri)]);
	}

	private async run<T>(request: object, mountPoints: MountPointDescriptor[] = []): Promise<T> {
		const { wasm, module } = await this.module();
		const io = await wasm.createMemoryFileSystem();
		io.createFile(REQUEST, new TextEncoder().encode(JSON.stringify(request)));
		const name = `${crypto.randomUUID()}.json`;
		const process = await wasm.createProcess('slides-notebook', module, MEMORY, {
			args: [`${IO}/${REQUEST}`, `${ANSWERS}/${name}`],
			// What it warns of may lose its end, as stdout would.
			stdio: { out: { kind: 'pipeOut' }, err: { kind: 'pipeOut' } },
			mountPoints: [
				{ kind: 'memoryFileSystem', fileSystem: io, mountPoint: IO },
				{ kind: 'vscodeFileSystem', uri: this.answers, mountPoint: ANSWERS },
				...mountPoints,
			],
		});
		const stderr: Uint8Array[] = [];
		process.stderr!.onData((data) => stderr.push(data));
		const answer = vscode.Uri.joinPath(this.answers, name);
		try {
			const code = await process.run();
			const errors = decode(stderr).trim();
			if (errors) {
				this.log.warn(errors);
			}
			if (code !== 0) {
				throw new Error(errors || `slides-notebook exited with ${code}`);
			}
			return JSON.parse(new TextDecoder().decode(await vscode.workspace.fs.readFile(answer))) as T;
		} finally {
			void Promise.resolve(vscode.workspace.fs.delete(answer)).catch(() => {});
		}
	}

	/** The module, compiled the first time it is needed. */
	private module(): Promise<{ wasm: Wasm; module: WebAssembly.Module }> {
		this.loading ??= (async () => {
			await vscode.workspace.fs.createDirectory(this.answers);
			const wasm = await Wasm.load();
			const module = await wasm.compile(vscode.Uri.joinPath(this.extensionUri, 'dist', 'slides-notebook.wasm'));
			return { wasm, module };
		})();
		return this.loading;
	}
}

/** The disk holding the file, mounted whole, so that the module looks for the lock file
 * and the outputs directory up to its root, as the deck does. */
function disk(uri: vscode.Uri): MountPointDescriptor {
	return { kind: 'vscodeFileSystem', uri: uri.with({ path: '/', query: '', fragment: '' }), mountPoint: DISK };
}

async function place(uri: vscode.Uri): Promise<Place> {
	const dir = vscode.Uri.joinPath(uri, '..');
	return {
		// The deck hashes the path on the disk; elsewhere, as in the browser, there is none.
		dir: dir.scheme === 'file' ? await realpath(dir.fsPath) : dir.path,
		path: DISK + dir.path,
	};
}

/** The path with its links resolved, as the deck and `slides-rs kernel` hash it. Only on
 * the desktop can they be. */
async function realpath(path: string): Promise<string> {
	if (!canRunCells()) {
		return path;
	}
	try {
		const fs = await import('node:fs/promises');
		return await fs.realpath(path);
	} catch {
		return path;
	}
}

function decode(chunks: Uint8Array[]): string {
	const decoder = new TextDecoder();
	return chunks.map((chunk) => decoder.decode(chunk, { stream: true })).join('') + decoder.decode();
}
