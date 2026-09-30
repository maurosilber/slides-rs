// Shows the deck in VS Code's integrated browser: `slides-rs --watch` renders it into
// its page, again whenever the deck or a file it imports is saved, and the browser
// reloads the page as it changes. Starting a process needs VS Code on the desktop.

import * as vscode from 'vscode';
import type { ChildProcess } from 'node:child_process';
import { command } from './kernel';

/** The integrated browser's command, which older versions of VS Code do not have. */
const INTEGRATED_BROWSER = 'workbench.action.browser.open';

/** An import, as a line of the markdown writes it, which src/markdown/mod.rs reads the same way. */
const IMPORT = /^\s*<import-slide\s*src="([^"]*)"/;

/** The line `slides-rs` writes once it renders the page, naming it. */
const RENDERED = /^rendered (.+) in \S+$/;

export class Previews implements vscode.Disposable {
	/** The deck being rendered for each file shown, by its uri. */
	private readonly watches = new Map<string, Watch>();

	constructor(
		private readonly extensionUri: vscode.Uri,
		private readonly log: vscode.LogOutputChannel,
	) {}

	/**
	 * Renders the slides of the file at `uri`, if it is not yet, and shows their page:
	 * as they are in the deck that imports them, if one does, in its theme and shape.
	 */
	async show(uri: vscode.Uri) {
		if (uri.scheme !== 'file') {
			void vscode.window.showErrorMessage('Only a deck saved on disk can be rendered.');
			return;
		}
		const deck = await deckOf(uri, this.log);
		const key = `${uri}\n${deck ?? ''}`;
		let watch = this.watches.get(key);
		if (!watch) {
			const started = new Watch(uri, deck, await command(this.extensionUri), this.log);
			started.onExit(() => {
				if (this.watches.get(key) === started) {
					this.watches.delete(key);
				}
			});
			this.watches.set(key, started);
			watch = started;
		}
		try {
			await open(await watch.page);
		} catch (error) {
			const message = `Could not render ${vscode.workspace.asRelativePath(uri)}: ${error instanceof Error ? error.message : error}`;
			this.log.error(message);
			void vscode.window.showErrorMessage(message);
		}
	}

	/** Stops rendering every deck. */
	stop() {
		this.watches.forEach((watch) => watch.dispose());
		this.watches.clear();
	}

	dispose() {
		this.stop();
	}
}

/** `slides-rs --watch`, rendering one deck. */
class Watch implements vscode.Disposable {
	/** The page, once it is first rendered. */
	readonly page: Promise<string>;
	private process: ChildProcess | undefined;
	private readonly exitListeners: (() => void)[] = [];

	constructor(
		private readonly uri: vscode.Uri,
		/** The deck that imports it, which it is rendered as it is in. */
		private readonly deck: vscode.Uri | undefined,
		command: string,
		private readonly log: vscode.LogOutputChannel,
	) {
		this.page = this.start(command);
		this.page.catch(() => {});
	}

	private async start(command: string): Promise<string> {
		const { spawn } = await import('node:child_process');
		const { createInterface } = await import('node:readline');
		const path = await import('node:path');
		const name = vscode.workspace.asRelativePath(this.uri);
		const dir = path.dirname(this.uri.fsPath);
		// A group of its own, so that stopping it stops the kernel it may be running too.
		const args = [this.uri.fsPath, '--watch', ...(this.deck ? ['--deck', this.deck.fsPath] : [])];
		const process = spawn(command, args, { cwd: dir, detached: globalThis.process.platform !== 'win32' });
		this.process = process;
		return new Promise((resolve, reject) => {
			process.once('error', (error) => reject(new Error(`Could not run \`${command}\`: ${error.message}. The \`slides.path\` setting names the \`slides-rs\` to run.`)));
			createInterface({ input: process.stderr! }).on('line', (line) => {
				this.log.info(`${name}: ${line}`);
				const rendered = RENDERED.exec(line);
				if (rendered) {
					resolve(path.resolve(dir, rendered[1]));
				}
			});
			process.once('close', () => {
				this.log.info(`${name}: stopped rendering`);
				reject(new Error('slides-rs exited before rendering it. Its output is in the Slides Notebook log.'));
				this.exitListeners.forEach((listener) => listener());
			});
		});
	}

	onExit(listener: () => void) {
		this.exitListeners.push(listener);
	}

	dispose() {
		const process = this.process;
		if (process?.pid === undefined || process.exitCode !== null) {
			return;
		}
		try {
			if (globalThis.process.platform === 'win32') {
				process.kill();
			} else {
				globalThis.process.kill(-process.pid);
			}
		} catch {
			// It exited meanwhile.
		}
	}
}

/**
 * The deck the file at `uri` is part of: the markdown in the workspace that imports it,
 * or that imports one that does, which nothing imports itself. Nothing, if no file
 * imports it. Of several, the first by path.
 */
async function deckOf(uri: vscode.Uri, log: vscode.LogOutputChannel): Promise<vscode.Uri | undefined> {
	const path = await import('node:path');
	const files = await vscode.workspace.findFiles('**/*.md', '{**/node_modules/**,**/_outputs/**}');
	const importers = new Map<string, vscode.Uri[]>();
	await Promise.all(files.map(async (file) => {
		let text: string;
		try {
			text = new TextDecoder().decode(await vscode.workspace.fs.readFile(file));
		} catch {
			return;
		}
		for (const line of text.split('\n')) {
			const src = IMPORT.exec(line)?.[1];
			if (src !== undefined) {
				const imported = path.resolve(path.dirname(file.fsPath), src);
				importers.set(imported, [...(importers.get(imported) ?? []), file]);
			}
		}
	}));
	let deck: vscode.Uri | undefined;
	const seen = new Set([uri.fsPath]);
	for (;;) {
		const found = (importers.get((deck ?? uri).fsPath) ?? [])
			.filter((file) => !seen.has(file.fsPath))
			.sort((a, b) => a.fsPath.localeCompare(b.fsPath));
		if (found.length === 0) {
			break;
		}
		if (found.length > 1) {
			log.info(`${vscode.workspace.asRelativePath(deck ?? uri)}: imported by ${found.map((file) => vscode.workspace.asRelativePath(file)).join(', ')}, shown as in the first`);
		}
		deck = found[0];
		seen.add(deck.fsPath);
	}
	return deck;
}

/** Opens the page in the integrated browser, or else in the default one. */
async function open(page: string) {
	const uri = vscode.Uri.file(page);
	const commands = await vscode.commands.getCommands(true);
	if (commands.includes(INTEGRATED_BROWSER)) {
		await vscode.commands.executeCommand(INTEGRATED_BROWSER, uri.toString());
	} else {
		await vscode.env.openExternal(uri);
	}
}
