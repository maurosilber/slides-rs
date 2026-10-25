// Which markdown files are decks, or parts of one, as the language server finds them, so
// that the deck's commands show on them alone: a file with what only a deck has, as
// slides-rs frontmatter, an import or a code cell, and a file another one imports. VS Code
// reads their paths from the `slides.decks` context key. A file in the slides language, as
// `*.slides.md` is, is one whatever it holds, which the menus' `when` clauses say themselves.

import * as vscode from 'vscode';
import type { SlidesServer } from './server';

/** The language of a file named `*.slides.md`, which is markdown to the deck. */
export const LANGUAGE = 'slides';

export class Decks implements vscode.Disposable {
	/** The paths of the files that are decks, or parts of one. */
	private decks = new Set<string>();
	private readonly changed = new vscode.EventEmitter<void>();
	/** Fires as the files that are decks change. */
	readonly onDidChange = this.changed.event;
	private readonly disposables: vscode.Disposable[] = [];

	constructor(server: SlidesServer) {
		this.disposables.push(
			this.changed,
			server.onDecks((paths) => {
				this.decks = new Set(paths);
				void vscode.commands.executeCommand('setContext', 'slides.decks', paths);
				this.changed.fire();
			}),
		);
	}

	/** Whether the document is a deck, or part of one. */
	isDeck(document: vscode.TextDocument): boolean {
		return document.languageId === LANGUAGE || (document.languageId === 'markdown' && this.decks.has(document.uri.fsPath));
	}

	dispose() {
		this.disposables.forEach((disposable) => disposable.dispose());
	}
}
