// Shows the deck in VS Code's integrated browser: `slides-rs --watch` renders it into
// its page, again whenever the deck or a file it imports is saved, and the browser
// reloads the page as it changes. Starting a process needs VS Code on the desktop.

import * as vscode from 'vscode';
import type { ChildProcess } from 'node:child_process';
import { command } from './kernel';

/** The integrated browser's command, which older versions of VS Code do not have. */
const INTEGRATED_BROWSER = 'workbench.action.browser.open';

/** The line `slides-rs` writes once it renders the page, naming it. */
const RENDERED = /^rendered (.+) in \S+$/;

export class Previews implements vscode.Disposable {
	/** The deck being rendered for each file shown, by its uri. */
	private readonly watches = new Map<string, Watch>();

	constructor(
		private readonly extensionUri: vscode.Uri,
		private readonly log: vscode.LogOutputChannel,
	) {}

	/** Renders the deck of the file at `uri`, if it is not yet, and shows its page. */
	async show(uri: vscode.Uri) {
		if (uri.scheme !== 'file') {
			void vscode.window.showErrorMessage('Only a deck saved on disk can be rendered.');
			return;
		}
		const key = uri.toString();
		let watch = this.watches.get(key);
		if (!watch) {
			const started = new Watch(uri, await command(this.extensionUri), this.log);
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
		const process = spawn(command, [this.uri.fsPath, '--watch'], { cwd: dir, detached: globalThis.process.platform !== 'win32' });
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
