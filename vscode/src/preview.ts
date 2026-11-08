// Shows the deck in VS Code's integrated browser, as the language server renders it with
// `slides-rs --watch`, again whenever the deck or a file it imports is saved, and the
// browser reloads the page as it changes. The integrated browser opens the page following
// the text editor: as its cursor moves, which the extension tells the server of, and once
// a save is rendered, the page goes to the slide and the step the cursor is at, which the
// server sends it, as telling the browser to go there would bring its tab to the front.

import * as vscode from 'vscode';
import { SELECTOR, type Shown, type SlidesServer } from './server';
import { previewFollowsCursor } from './settings';

/** The integrated browser's command, which older versions of VS Code do not have. */
const INTEGRATED_BROWSER = 'workbench.action.browser.open';

/** How long the cursor rests before the page goes to where it is. */
const FOLLOW_DELAY = 100;

export class Previews implements vscode.Disposable {
	/** The files shown, by uri. */
	private readonly shown = new Set<string>();
	/** The timer of the cursor of each document, by uri, which the page follows. */
	private readonly timers = new Map<string, ReturnType<typeof setTimeout>>();
	private readonly disposables: vscode.Disposable[] = [];

	constructor(
		private readonly server: SlidesServer,
		private readonly log: vscode.LogOutputChannel,
	) {
		this.disposables.push(
			vscode.window.onDidChangeTextEditorSelection(({ textEditor }) => this.moved(textEditor)),
			// A save rendered, whose page follows no cursor to be sent where it is.
			server.onOpen(async ({ page, fragment }) => {
				if ((await vscode.commands.getCommands(true)).includes(INTEGRATED_BROWSER)) {
					await vscode.commands.executeCommand(INTEGRATED_BROWSER, browserTab(page, fragment ?? undefined));
				}
			}),
		);
	}

	/**
	 * Renders the slides of the file at `uri`, if they are not yet, and shows their page:
	 * as they are in the deck that imports them, if one does, in its theme and shape.
	 */
	async show(uri: vscode.Uri) {
		// Where the cursor is, if the file is in a text editor, which a tab that shows the
		// page already goes to rather than back to the first slide.
		const editor = [vscode.window.activeTextEditor, ...vscode.window.visibleTextEditors].find((editor) => editor?.document.uri.toString() === uri.toString());
		try {
			const shown = await this.server.show(uri, editor?.selection.active.line, await this.following(uri));
			this.shown.add(uri.toString());
			await open(shown);
		} catch (error) {
			const message = `Could not render ${vscode.workspace.asRelativePath(uri)}: ${error instanceof Error ? error.message : error}`;
			this.log.error(message);
			void vscode.window.showErrorMessage(message);
		}
	}

	/** Whether the page of the file at `uri` follows its cursor, which only the
	 * integrated browser opens it to. */
	private async following(uri: vscode.Uri): Promise<boolean> {
		const document = vscode.workspace.textDocuments.find((document) => document.uri.toString() === uri.toString());
		return (document === undefined || previewFollowsCursor(document))
			&& (await vscode.commands.getCommands(true)).includes(INTEGRATED_BROWSER);
	}

	/** Tells the server where the cursor of a file is, once it rests there, while a page is
	 * shown: of the file, or of a deck that imports it, which the server finds. */
	private moved(editor: vscode.TextEditor) {
		const { document } = editor;
		const key = document.uri.toString();
		if (this.shown.size === 0 || !SELECTOR.some(({ language }) => language === document.languageId)) {
			return;
		}
		clearTimeout(this.timers.get(key));
		this.timers.set(key, setTimeout(() => {
			this.timers.delete(key);
			this.server.cursor(document.uri, editor.selection.active.line, previewFollowsCursor(document));
		}, FOLLOW_DELAY));
	}

	/** Stops rendering every deck. */
	stop() {
		this.shown.clear();
		void this.server.stop();
	}

	dispose() {
		this.stop();
		this.timers.forEach((timer) => clearTimeout(timer));
		this.disposables.forEach((disposable) => disposable.dispose());
	}
}

/** Opens the page in the integrated browser, at `#slide.step` if given, and with the
 * query, in the tab that shows it already if one does, or else in the default browser. */
async function open({ page, fragment, query }: Shown) {
	const commands = await vscode.commands.getCommands(true);
	if (commands.includes(INTEGRATED_BROWSER)) {
		await vscode.commands.executeCommand(INTEGRATED_BROWSER, browserTab(page, fragment ?? undefined, query ?? undefined));
	} else {
		await vscode.env.openExternal(vscode.Uri.file(page));
	}
}

/** What the integrated browser is asked to open: the page, at `#slide.step` if given, in
 * the tab that shows it already, whatever slide that is at and whatever its query, which
 * then goes there. */
function browserTab(page: string, fragment?: string, query?: string) {
	const uri = vscode.Uri.file(page);
	// Written out, as a uri would encode the query's `=` and `/`, which the page then
	// does not read.
	const url = `${uri.toString()}${query ? `?${query}` : ''}${fragment ? `#${fragment}` : ''}`;
	return { url, reuseUrlFilter: uri.toString() };
}
