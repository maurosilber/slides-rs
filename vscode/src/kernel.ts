// Runs the code cells as the deck does, on a kernel that `slides-rs kernel`
// starts for each notebook: next to its file, in the environment its lock file
// pins. The WebAssembly cannot start a process, so this needs VS Code on the
// desktop.

import * as vscode from 'vscode';
import type { ChildProcessWithoutNullStreams } from 'node:child_process';
import { cellOutput, Saved } from './output';

/** A cell that ran: its outputs, which are saved in the outputs directory, whether it raised,
 * its count, and whether its outputs were saved with the files it read, which the deck trusts. */
interface Ran {
	outputs: Saved[];
	ok: boolean;
	count: number;
	trusted: boolean;
}

type Answer = { kernel: string } | Ran | { error: string };

/** Whether this extension host can start a process. */
export function canRunCells(): boolean {
	return typeof process === 'object' && typeof process.versions?.node === 'string';
}

export class Kernels implements vscode.Disposable {
	private readonly controller: vscode.NotebookController;
	/** The kernel of each open notebook that has run a cell, by its uri. */
	private readonly sessions = new Map<string, Session>();
	private readonly disposables: vscode.Disposable[] = [];

	constructor(
		notebookType: string,
		private readonly extensionUri: vscode.Uri,
		private readonly log: vscode.LogOutputChannel,
	) {
		this.controller = vscode.notebooks.createNotebookController('slides-rs', notebookType, 'slides-rs');
		this.controller.description = 'Runs the cells as the deck does';
		this.controller.supportedLanguages = ['python'];
		this.controller.supportsExecutionOrder = true;
		this.controller.executeHandler = (cells, notebook) => this.execute(cells, notebook);
		this.controller.interruptHandler = (notebook) => this.sessions.get(notebook.uri.toString())?.interrupt();
		const prefer = (notebook: vscode.NotebookDocument) => {
			if (notebook.notebookType === notebookType) {
				this.controller.updateNotebookAffinity(notebook, vscode.NotebookControllerAffinity.Preferred);
			}
		};
		vscode.workspace.notebookDocuments.forEach(prefer);
		this.disposables.push(
			this.controller,
			vscode.workspace.onDidOpenNotebookDocument(prefer),
			vscode.workspace.onDidCloseNotebookDocument((notebook) => this.restart(notebook)),
		);
	}

	/** Shuts down the notebook's kernel, so that the next cell to run starts a new one. */
	restart(notebook: vscode.NotebookDocument) {
		const key = notebook.uri.toString();
		this.sessions.get(key)?.dispose();
		this.sessions.delete(key);
	}

	/** Runs the cells one after another, stopping at the first that fails. */
	private async execute(cells: vscode.NotebookCell[], notebook: vscode.NotebookDocument) {
		// Created up front, so the cells show as queued behind the one running.
		const executions = cells.map((cell) => this.controller.createNotebookCellExecution(cell));
		const session = this.session(notebook);
		for (const [index, execution] of executions.entries()) {
			const ok = await session.queue(() => this.run(session, notebook, execution));
			if (!ok) {
				// The rest end without having started, as they did not run.
				for (const rest of executions.slice(index + 1)) {
					rest.start();
					rest.end(undefined);
				}
				return;
			}
		}
	}

	private async run(session: Session, notebook: vscode.NotebookDocument, execution: vscode.NotebookCellExecution): Promise<boolean> {
		execution.start(Date.now());
		await execution.clearOutput();
		try {
			// Its address is the hash of every code cell up to it, as the notebook has them now.
			const cells = notebook
				.getCells()
				.slice(0, execution.cell.index + 1)
				.filter((cell) => cell.kind === vscode.NotebookCellKind.Code)
				.map((cell) => cell.document.getText());
			const answer = await session.run(cells);
			if (!answer.trusted) {
				this.log.info(`${vscode.workspace.asRelativePath(notebook.uri)}: cell ${execution.cell.index + 1} did not run after just the cells before it, in order, so the deck runs it again`);
			}
			await execution.replaceOutput(answer.outputs.map(cellOutput));
			execution.executionOrder = answer.count;
			execution.end(answer.ok, Date.now());
			return answer.ok;
		} catch (error) {
			// The kernel is gone, and its state with it: the next cell starts a new one.
			this.restart(notebook);
			await execution.replaceOutput(new vscode.NotebookCellOutput([vscode.NotebookCellOutputItem.error(asError(error))]));
			execution.end(false, Date.now());
			return false;
		}
	}

	private session(notebook: vscode.NotebookDocument): Session {
		const key = notebook.uri.toString();
		let session = this.sessions.get(key);
		if (!session) {
			session = new Session(notebook.uri, command(this.extensionUri), this.log);
			session.onExit(() => this.sessions.get(key) === session && this.sessions.delete(key));
			this.sessions.set(key, session);
		}
		return session;
	}

	dispose() {
		this.sessions.forEach((session) => session.dispose());
		this.sessions.clear();
		this.disposables.forEach((disposable) => disposable.dispose());
	}
}

/** A `slides-rs kernel` for one notebook, started along with the first cell it runs. */
class Session {
	private readonly started: Promise<ChildProcessWithoutNullStreams>;
	/** The answers yet to be read, as the lines arrive before they are waited for. */
	private readonly lines: string[] = [];
	private waiting: ((line: string | undefined) => void) | undefined;
	private exited = false;
	private readonly exitListeners: (() => void)[] = [];
	/** The cell running or last run, which the next one waits for. */
	private last: Promise<unknown> = Promise.resolve();

	constructor(
		private readonly uri: vscode.Uri,
		command: Promise<string>,
		private readonly log: vscode.LogOutputChannel,
	) {
		this.started = command.then((command) => this.start(command));
		// Failing to start is the first cell's to report.
		this.started.catch(() => {});
	}

	/** Runs `task` once every task queued before it is done. */
	queue<T>(task: () => Promise<T>): Promise<T> {
		const result = this.last.then(task);
		this.last = result.catch(() => {});
		return result;
	}

	/** Runs the last of the code cells, and saves its outputs. */
	async run(cells: string[]): Promise<Ran> {
		const process = await this.started;
		process.stdin.write(JSON.stringify({ cells }) + '\n');
		const answer = await this.answer();
		if (!('outputs' in answer)) {
			throw new Error('error' in answer ? answer.error : `unexpected answer ${JSON.stringify(answer)}`);
		}
		return answer;
	}

	/** Stops the cell running. xeus-python cannot stop one, and exits instead. */
	interrupt() {
		void this.started.then((process) => process.kill('SIGINT'));
	}

	onExit(listener: () => void) {
		this.exitListeners.push(listener);
	}

	/** Closing stdin shuts the kernel down, which killing `slides-rs` would leave running. */
	dispose() {
		void this.started.then((process) => process.stdin.end());
	}

	private async start(command: string): Promise<ChildProcessWithoutNullStreams> {
		if (this.uri.scheme !== 'file') {
			throw new Error('Only a notebook saved on disk can run its cells, next to its file.');
		}
		const { spawn } = await import('node:child_process');
		const { createInterface } = await import('node:readline');
		const process = spawn(command, ['kernel', this.uri.fsPath]);
		const spawned = new Promise<void>((resolve, reject) => {
			process.once('spawn', resolve);
			process.once('error', (error) => reject(new Error(`Could not run \`${command}\`: ${error.message}. The \`slides.path\` setting names the \`slides-rs\` to run.`)));
		});
		createInterface({ input: process.stdout }).on('line', (line) => this.receive(line));
		createInterface({ input: process.stderr }).on('line', (line) => this.log.info(`${vscode.workspace.asRelativePath(this.uri)}: ${line}`));
		process.once('close', () => {
			this.exited = true;
			this.receive(undefined);
			this.exitListeners.forEach((listener) => listener());
		});
		await spawned;
		const answer = await this.answer();
		if (!('kernel' in answer)) {
			throw new Error('error' in answer ? answer.error : `unexpected answer ${JSON.stringify(answer)}`);
		}
		this.log.info(`${vscode.workspace.asRelativePath(this.uri)}: started the ${answer.kernel} kernel`);
		return process;
	}

	private receive(line: string | undefined) {
		if (this.waiting) {
			const waiting = this.waiting;
			this.waiting = undefined;
			waiting(line);
		} else if (line !== undefined) {
			this.lines.push(line);
		}
	}

	private async answer(): Promise<Answer> {
		const line = this.lines.shift() ?? (this.exited ? undefined : await new Promise<string | undefined>((resolve) => (this.waiting = resolve)));
		if (line === undefined) {
			throw new Error('slides-rs exited without answering. Its output is in the Slides Notebook log.');
		}
		return JSON.parse(line) as Answer;
	}
}

/** The `slides-rs` the settings name, or else the one that comes with the extension, or else the one on the `PATH`. */
export async function command(extensionUri: vscode.Uri): Promise<string> {
	const configured = vscode.workspace.getConfiguration('slides').get<string>('path');
	if (configured) {
		return configured;
	}
	const bundled = vscode.Uri.joinPath(extensionUri, 'dist', 'slides-rs');
	try {
		await vscode.workspace.fs.stat(bundled);
		return bundled.fsPath;
	} catch {
		return 'slides-rs';
	}
}

function asError(error: unknown): Error {
	return error instanceof Error ? error : new Error(String(error));
}
