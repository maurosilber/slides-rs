// Which markdown files are decks, or parts of one, so that the deck's commands show on
// them alone: a file with what only a deck has, as slides-rs frontmatter, an import or
// a code cell, and a file another one imports. VS Code reads their paths from the
// `slides.decks` context key. A file in the slides language, as `*.slides.md` is, is
// one whatever it holds, which the menus' `when` clauses say themselves.

import * as vscode from 'vscode';

/** The language of a file named `*.slides.md`, which is markdown to the deck. */
export const LANGUAGE = 'slides';

/** The languages a deck is written in. */
export const SELECTOR: vscode.DocumentFilter[] = [{ language: 'markdown' }, { language: LANGUAGE }];

/** An import, as a line of the markdown writes it, which src/markdown/mod.rs reads the same way. */
const IMPORT = /^\s*<import-slide\s*src="([^"]*)"/;

/** A tilde fence, which opens a code cell. */
const CELL = /^ {0,3}~~~/m;

/** The frontmatter, between the `---` that opens the file and the next. */
const FRONTMATTER = /^---\r?\n([\s\S]*?)\r?\n---\s*$/m;

/** A key of the frontmatter that only a deck reads, as src/frontmatter.schema.json has them. */
const KEY = /^(theme|aspect-ratio|steps|figures)\s*:/m;

/** What a file holds, as far as telling decks apart goes. */
interface Scanned {
	/** Whether it has what only a deck has. */
	deck: boolean;
	/** The files it imports, by their uri. */
	imports: string[];
}

export class Decks implements vscode.Disposable {
	/** Each markdown file of the workspace, and those opened from outside it, by uri. */
	private readonly files = new Map<string, Scanned>();
	/** The paths of the files that are decks, or parts of one. */
	private decks = new Set<string>();
	private readonly changed = new vscode.EventEmitter<void>();
	/** Fires as the files that are decks change. */
	readonly onDidChange = this.changed.event;
	private readonly ready: Promise<void>;
	private readonly disposables: vscode.Disposable[] = [];

	constructor() {
		const watcher = vscode.workspace.createFileSystemWatcher('**/*.md');
		const rescan = (uri: vscode.Uri) => void this.rescan(uri);
		this.disposables.push(
			this.changed,
			watcher,
			watcher.onDidCreate(rescan),
			watcher.onDidChange(rescan),
			watcher.onDidDelete(async (uri) => {
				await this.ready;
				this.files.delete(uri.toString());
				this.publish();
			}),
			// A file outside the workspace, which the watcher does not see.
			vscode.workspace.onDidOpenTextDocument((document) => isSaved(document) && rescan(document.uri)),
			vscode.workspace.onDidSaveTextDocument((document) => isSaved(document) && rescan(document.uri)),
		);
		this.ready = this.scanAll();
	}

	private async scanAll() {
		const files = await vscode.workspace.findFiles('**/*.md', '{**/node_modules/**,**/_outputs/**}');
		const opened = vscode.workspace.textDocuments.filter(isSaved).map((document) => document.uri);
		await Promise.all([...files, ...opened].map((file) => this.scan(file)));
		this.publish();
	}

	private async rescan(uri: vscode.Uri) {
		await this.ready;
		await this.scan(uri);
		this.publish();
	}

	private async scan(uri: vscode.Uri) {
		let text: string;
		try {
			text = new TextDecoder().decode(await vscode.workspace.fs.readFile(uri));
		} catch {
			this.files.delete(uri.toString());
			return;
		}
		const dir = vscode.Uri.joinPath(uri, '..');
		const imports = text
			.split('\n')
			.map((line) => IMPORT.exec(line)?.[1])
			.filter((src) => src !== undefined)
			.map((src) => vscode.Uri.joinPath(dir, src).toString());
		const frontmatter = FRONTMATTER.exec(text);
		const deck = imports.length > 0 || CELL.test(text) || (frontmatter?.index === 0 && KEY.test(frontmatter[1]));
		this.files.set(uri.toString(), { deck, imports });
	}

	/** Tells VS Code which files are decks. */
	private publish() {
		const decks = new Set<string>();
		for (const [uri, file] of this.files) {
			if (file.deck) {
				decks.add(uri);
			}
			file.imports.forEach((imported) => decks.add(imported));
		}
		this.decks = new Set([...decks].map((uri) => vscode.Uri.parse(uri).fsPath));
		void vscode.commands.executeCommand('setContext', 'slides.decks', [...this.decks]);
		this.changed.fire();
	}

	/** Whether the document is a deck, or part of one. */
	isDeck(document: vscode.TextDocument): boolean {
		return document.languageId === LANGUAGE || (document.languageId === 'markdown' && this.decks.has(document.uri.fsPath));
	}

	/** The files that import the one at `uri`, by path. */
	private importers(uri: vscode.Uri): vscode.Uri[] {
		const key = uri.toString();
		return [...this.files]
			.filter(([, file]) => file.imports.includes(key))
			.map(([importer]) => vscode.Uri.parse(importer))
			.sort((a, b) => a.fsPath.localeCompare(b.fsPath));
	}

	/**
	 * The deck the file at `uri` is part of: the file that imports it, or that imports
	 * one that does, which nothing imports itself. Nothing, if no file imports it. Of
	 * several, the first by path.
	 */
	async deckOf(uri: vscode.Uri, log: vscode.LogOutputChannel): Promise<vscode.Uri | undefined> {
		await this.ready;
		let deck: vscode.Uri | undefined;
		const seen = new Set([uri.toString()]);
		for (;;) {
			const found = this.importers(deck ?? uri).filter((file) => !seen.has(file.toString()));
			if (found.length === 0) {
				return deck;
			}
			if (found.length > 1) {
				log.info(`${vscode.workspace.asRelativePath(deck ?? uri)}: imported by ${found.map((file) => vscode.workspace.asRelativePath(file)).join(', ')}, shown as in the first`);
			}
			deck = found[0];
			seen.add(deck.toString());
		}
	}

	dispose() {
		this.disposables.forEach((disposable) => disposable.dispose());
	}
}

/** Whether the document is markdown saved as a file, rather than a notebook's cell, a
 * diff's side or a file not saved yet. */
function isSaved(document: vscode.TextDocument): boolean {
	const schemes = new Set(['file', ...(vscode.workspace.workspaceFolders ?? []).map((folder) => folder.uri.scheme)]);
	return schemes.has(document.uri.scheme) && vscode.languages.match(SELECTOR, document) > 0;
}
