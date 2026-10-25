// The extension's language server, `slides-rs lsp`, started in the workspace's pixi
// environment, or as the `slides.path` setting names it. It completes and describes the
// frontmatter and the attributes the deck reads, links each import to the file it imports,
// warns of what is wrong with how the slides step, and says which files are decks. The code
// cells have what a Python language server it starts in their environment has. Its own
// requests, under `slides/`, answer what the notebook and the slide editor ask of the deck.
// It names the `slides-rs` it is, which runs the cells and renders the slides too.

import * as vscode from 'vscode';
import { LanguageClient, ServerOptions } from 'vscode-languageclient/node';
import type { Saved } from './output';

export type Kind = 'markdown' | 'code';

/** A cell, as the server reads it from markdown and writes it back. */
export interface Cell {
	kind: Kind;
	source: string;
	language: string;
	/** How it was written, which the server needs back to write it the same. */
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

/** Where a file's slides break, by line, counted from zero, as the deck breaks them. */
export interface Breaks {
	/** The first line after the frontmatter, where the first slide begins. */
	start: number;
	/** The lines of the rules between two slides. */
	rules: number[];
	/** The imports, which bring slides of their own, by line, with the file each imports,
	 * relative to the importing one. */
	imports: { line: number; src: string }[];
	/** The headings of the slides, by their first and last lines, and their level, from 1. */
	headings: { line: number; last: number; level: number }[];
	/** Whether it has what only a deck, or a part of one, has: frontmatter the deck reads,
	 * an import or a code cell. */
	deck: boolean;
}

/** The steps of a slide, as the deck numbers them, by line, counted from zero. */
export interface SlideSteps {
	/** The line its slide opens at: the first of the file, or the one after its break. */
	line: number;
	/** How many steps it has. */
	count: number;
	/** What steps in it, in order, but what shows from the start all along. */
	steps: { line: number; from: number; to: number | null; collapse: boolean }[];
	/** What is wrong with how it steps, by line. */
	warnings: { line: number; message: string }[];
	/** Its columns, as the deck boxes them, from the line of the heading that starts each
	 * to the last of what it holds, and which of the columns side by side each is, from 0. */
	columns: { line: number; last: number; index: number }[];
}

/** The languages a deck is written in. */
export const SELECTOR: { language: string }[] = [{ language: 'markdown' }, { language: 'slides' }];

export class SlidesServer implements vscode.Disposable {
	private readonly client: LanguageClient;
	private readonly disposables: vscode.Disposable[] = [];
	/** Once the server has started, and its completions, hovers and links are registered. */
	readonly started: Promise<void>;

	constructor(log: vscode.LogOutputChannel) {
		this.client = new LanguageClient('slides-rs', 'slides-rs', serverOptions(), {
			documentSelector: SELECTOR,
			synchronize: {
				fileEvents: [
					vscode.workspace.createFileSystemWatcher('**/*.md'),
					// The outputs of the cells step as they do on the slides, so a run that saves
					// them moves the steps, though the document stays as it is.
					vscode.workspace.createFileSystemWatcher('**/_outputs/*/outputs.txt'),
				],
			},
			// Read again as it restarts.
			initializationOptions: () => ({ pythonServer: pythonServer() }),
			outputChannel: log,
		});
		this.started = this.client.start().catch((error) => {
			const message = `Could not start the slides-rs language server: ${error}. It runs as \`pixi run --frozen slides-rs lsp\` in the workspace, or as the \`slides.path\` setting names it.`;
			log.error(message);
			void vscode.window.showErrorMessage(message);
			throw error;
		});
		// Failing to start is each request's to report.
		this.started.catch(() => {});
		this.disposables.push(
			vscode.workspace.onDidChangeConfiguration((event) => {
				if (event.affectsConfiguration('slides.pythonServer')) {
					void this.client.restart();
				}
			}),
		);
	}

	/** Calls `listener` with the paths of the files that are decks, or parts of one, as
	 * they change. */
	onDecks(listener: (paths: string[]) => void): vscode.Disposable {
		return this.client.onNotification('slides/decks', ({ paths }: { paths: string[] }) => listener(paths));
	}

	/** The `slides-rs` the server is, which runs the cells and renders the slides. */
	async executable(): Promise<string> {
		await this.started;
		return this.client.initializeResult?.capabilities.experimental?.executable ?? 'slides-rs';
	}

	/** The cells of a markdown file. */
	cells(markdown: string): Promise<Cell[]> {
		return this.request('slides/cells', { markdown });
	}

	/** The markdown file the cells make. */
	markdown(cells: Cell[]): Promise<string> {
		return this.request('slides/markdown', { cells });
	}

	/** Where the slides of a markdown file break. */
	slides(markdown: string): Promise<Breaks> {
		return this.request('slides/breaks', { markdown });
	}

	/** The steps of the slides of a markdown file, by line, with the saved outputs of its
	 * cells, which step as they do on the slides, if it is the file at `uri`. */
	steps(markdown: string, uri: vscode.Uri): Promise<SlideSteps[]> {
		return this.request('slides/steps', { markdown, uri: uri.toString() });
	}

	/** Where a line of the file at `uri`, whose markdown is `markdown`, is on its page, as
	 * slides.js numbers it: its slide, from 1, among those of the file and those its
	 * imports bring, and the step it shows at. */
	position(markdown: string, line: number, uri: vscode.Uri): Promise<{ slide: number; step: number }> {
		return this.request('slides/position', { markdown, line, uri: uri.toString() });
	}

	/** The saved outputs of each code cell of the file at `uri`, given their sources in order. */
	load(uri: vscode.Uri, sources: string[]): Promise<Saved[][]> {
		return this.request('slides/load', { uri: uri.toString(), sources });
	}

	/** Saves the outputs of the code cells of the file at `uri`, returning how many cells it saved. */
	save(uri: vscode.Uri, cells: CodeCell[]): Promise<number> {
		return this.request('slides/save', { uri: uri.toString(), cells });
	}

	/**
	 * The deck the file at `uri` is part of: the file that imports it, or that imports one
	 * that does, which nothing imports itself. Nothing, if no file imports it. Of several,
	 * the first by path.
	 */
	async deckOf(uri: vscode.Uri): Promise<vscode.Uri | undefined> {
		const deck = await this.request<string | null>('slides/deckOf', { uri: uri.toString() });
		return deck === null ? undefined : vscode.Uri.parse(deck);
	}

	private async request<T>(method: string, params: object): Promise<T> {
		await this.started;
		return this.client.sendRequest<T>(method, params);
	}

	dispose() {
		this.disposables.forEach((disposable) => disposable.dispose());
		void this.client.stop();
	}
}

/** The Python language server the `slides.pythonServer` setting names, or `off`. */
function pythonServer(): string {
	return vscode.workspace.getConfiguration('slides').get<string>('pythonServer', '');
}

/** `slides-rs lsp`, as the `slides.path` setting names it, or else in the pixi environment
 * of the workspace's first folder, or else on the `PATH`. */
function serverOptions(): ServerOptions {
	const configured = vscode.workspace.getConfiguration('slides').get<string>('path');
	if (configured) {
		return { command: configured, args: ['lsp'] };
	}
	const folder = vscode.workspace.workspaceFolders?.[0];
	if (folder?.uri.scheme === 'file') {
		return { command: 'pixi', args: ['run', '--frozen', 'slides-rs', 'lsp'], options: { cwd: folder.uri.fsPath } };
	}
	return { command: 'slides-rs', args: ['lsp'] };
}
