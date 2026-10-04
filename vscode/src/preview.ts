// Shows the deck in VS Code's integrated browser: `slides-rs --watch` renders it into
// its page, again whenever the deck or a file it imports is saved, and the browser
// reloads the page as it changes. Once a save of the file shown is rendered, the
// browser goes to the slide and the step the cursor is at. Starting a process needs
// VS Code on the desktop.

import * as vscode from 'vscode';
import type { ChildProcess } from 'node:child_process';
import { command } from './kernel';
import type { Decks } from './decks';
import type { Module } from './wasm';

/** The integrated browser's command, which older versions of VS Code do not have. */
const INTEGRATED_BROWSER = 'workbench.action.browser.open';

/** The line `slides-rs` writes once it renders the page, naming it. */
const RENDERED = /^rendered (.+) in \S+$/;

/** The line `slides-rs` writes once it renders the page as it was already. */
const UNCHANGED = /^(.+) is unchanged, after \S+$/;

export class Previews implements vscode.Disposable {
	/** The deck being rendered for each file shown, by its uri. */
	private readonly watches = new Map<string, Watch>();
	private readonly disposables: vscode.Disposable[] = [];

	constructor(
		private readonly extensionUri: vscode.Uri,
		private readonly module: Module,
		private readonly decks: Decks,
		private readonly log: vscode.LogOutputChannel,
	) {
		this.disposables.push(vscode.workspace.onDidSaveTextDocument((document) => void this.saved(document)));
	}

	/**
	 * Renders the slides of the file at `uri`, if it is not yet, and shows their page:
	 * as they are in the deck that imports them, if one does, in its theme and shape.
	 */
	async show(uri: vscode.Uri) {
		if (uri.scheme !== 'file') {
			void vscode.window.showErrorMessage('Only a deck saved on disk can be rendered.');
			return;
		}
		const deck = await this.decks.deckOf(uri, this.log);
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
			// Where the cursor is, if the file is in a text editor, which a tab that shows the
			// page already goes to rather than back to the first slide.
			const at = this.cursor(uri);
			const page = await watch.page;
			const fragment = await at.then(({ slide, step }) => `${slide}.${step}`, () => undefined);
			await open(page, fragment);
		} catch (error) {
			const message = `Could not render ${vscode.workspace.asRelativePath(uri)}: ${error instanceof Error ? error.message : error}`;
			this.log.error(message);
			void vscode.window.showErrorMessage(message);
		}
	}

	/** Once a save of a file shown is rendered, goes to where its cursor is, on the page
	 * the browser shows, which only the integrated browser can be told to. */
	private async saved(document: vscode.TextDocument) {
		const watches = [...this.watches.values()].filter((watch) => watch.uri.toString() === document.uri.toString());
		if (watches.length === 0 || !(await vscode.commands.getCommands(true)).includes(INTEGRATED_BROWSER)) {
			return;
		}
		// Found as the save has it, before the render that follows it is done.
		const at = this.cursor(document.uri);
		at.catch(() => {});
		for (const watch of watches) {
			watch.nextRender(async (page) => {
				const { slide, step } = await at;
				await vscode.commands.executeCommand(INTEGRATED_BROWSER, browserTab(page, `${slide}.${step}`));
			});
		}
	}

	/** Where the cursor of the text editor of the file at `uri` is on the page, which
	 * fails if no text editor shows the file. */
	private async cursor(uri: vscode.Uri): Promise<{ slide: number; step: number }> {
		const editor = [vscode.window.activeTextEditor, ...vscode.window.visibleTextEditors].find((editor) => editor?.document.uri.toString() === uri.toString());
		if (!editor) {
			throw new Error('no text editor shows it');
		}
		const line = editor.selection.active.line;
		try {
			return await position(this.module, uri, editor.document.getText(), line);
		} catch (error) {
			this.log.error(`${vscode.workspace.asRelativePath(uri)}: could not find the slide at line ${line + 1}: ${error}`);
			throw error;
		}
	}

	/** Stops rendering every deck. */
	stop() {
		this.watches.forEach((watch) => watch.dispose());
		this.watches.clear();
	}

	dispose() {
		this.stop();
		this.disposables.forEach((disposable) => disposable.dispose());
	}
}

/** `slides-rs --watch`, rendering one deck. */
class Watch implements vscode.Disposable {
	/** The page, once it is first rendered. */
	readonly page: Promise<string>;
	private process: ChildProcess | undefined;
	private readonly exitListeners: (() => void)[] = [];
	/** What to do once the page is rendered again, which a later save replaces. */
	private next: ((page: string) => Promise<void>) | undefined;

	constructor(
		readonly uri: vscode.Uri,
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
				const rendered = RENDERED.exec(line) ?? UNCHANGED.exec(line);
				if (rendered) {
					const page = path.resolve(dir, rendered[1]);
					resolve(page);
					const next = this.next;
					this.next = undefined;
					next?.(page).catch((error) => this.log.error(`${name}: could not go to the slide: ${error}`));
				}
			});
			process.once('close', () => {
				this.log.info(`${name}: stopped rendering`);
				reject(new Error('slides-rs exited before rendering it. Its output is in the Slides Notebook log.'));
				this.exitListeners.forEach((listener) => listener());
			});
		});
	}

	/** Calls `listener` with the page once it is rendered again, unless another is set first. */
	nextRender(listener: (page: string) => Promise<void>) {
		this.next = listener;
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

/** Opens the page in the integrated browser, at `#slide.step` if given, in the tab that
 * shows it already if one does, or else in the default browser. */
async function open(page: string, fragment?: string) {
	const uri = vscode.Uri.file(page);
	const commands = await vscode.commands.getCommands(true);
	if (commands.includes(INTEGRATED_BROWSER)) {
		await vscode.commands.executeCommand(INTEGRATED_BROWSER, browserTab(page, fragment));
	} else {
		await vscode.env.openExternal(uri);
	}
}

/** What the integrated browser is asked to open: the page, at `#slide.step` if given, in
 * the tab that shows it already, whatever slide that is at, which then goes there. */
function browserTab(page: string, fragment?: string) {
	const uri = vscode.Uri.file(page);
	return { url: uri.with({ fragment: fragment ?? '' }).toString(), reuseUrlFilter: uri.toString() };
}

/** Where a line is on the page, as slides.js numbers it: its slide, from 1, among those
 * of the file and those its imports bring, and the step it shows at. */
async function position(module: Module, uri: vscode.Uri, text: string, line: number): Promise<{ slide: number; step: number }> {
	const lines = text.split(/\r?\n/);
	const breaks = await module.slides(text);
	const imports = new Map(breaks.imports.map(({ line, src }) => [line, src]));
	let slide = 0;
	let start = breaks.start;
	for (const end of [...breaks.rules, ...imports.keys()].sort((a, b) => a - b)) {
		if (end >= line) {
			break;
		}
		if (!isEmpty(lines, start, end)) {
			slide++;
		}
		const src = imports.get(end);
		if (src !== undefined) {
			slide += await slideCount(module, vscode.Uri.joinPath(uri, '..', src), new Set([uri.toString()]));
		}
		start = end + 1;
	}
	// On the line of an import, it is at the first of the slides it brings.
	if (imports.has(line)) {
		return { slide: slide + 1, step: 1 };
	}
	// What steps at or before the line, in its slide, shows it.
	const steps = await module.steps(text, uri);
	const own = steps.filter((slide) => slide.line <= line).at(-1);
	const before = own?.steps.filter((step) => step.line <= line && step.line >= start) ?? [];
	const last = before.at(-1)?.line;
	const step = Math.max(1, ...before.filter((step) => step.line === last).map((step) => step.from));
	return { slide: slide + 1, step };
}

/** How many slides a file brings, along with those its imports bring, as the deck
 * leaves the empty ones out. A file imported again within itself brings none. */
async function slideCount(module: Module, uri: vscode.Uri, importing: Set<string>): Promise<number> {
	if (importing.has(uri.toString())) {
		return 0;
	}
	let text: string;
	try {
		text = new TextDecoder().decode(await vscode.workspace.fs.readFile(uri));
	} catch {
		return 0;
	}
	const lines = text.split(/\r?\n/);
	const breaks = await module.slides(text);
	const within = new Set([...importing, uri.toString()]);
	const imports = new Map(breaks.imports.map(({ line, src }) => [line, src]));
	let count = 0;
	let start = breaks.start;
	for (const end of [...breaks.rules, ...imports.keys(), lines.length].sort((a, b) => a - b)) {
		if (!isEmpty(lines, start, end)) {
			count++;
		}
		const src = imports.get(end);
		if (src !== undefined) {
			count += await slideCount(module, vscode.Uri.joinPath(uri, '..', src), within);
		}
		start = end + 1;
	}
	return count;
}

/** Whether the lines from `start` up to `end`, excluded, are all blank. */
function isEmpty(lines: string[], start: number, end: number): boolean {
	return lines.slice(start, end).every((line) => /^\s*$/.test(line));
}
