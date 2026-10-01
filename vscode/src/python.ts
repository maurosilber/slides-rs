// Completes the Python of a deck's code cells in the markdown text editor: with the
// Python language server lsp.ts starts in the environment the cells run in, or, without
// one, as whichever Python language server VS Code has completes a Python file. For
// that one, the cells are written, in order, as the kernel runs them, to a file of
// their own, next to the deck, so that the server resolves its imports as the deck's,
// each line where it is in the markdown and the rest blank. Asked of that file, the
// server's completions hold for the markdown as they are. A notebook's cells are
// Python documents already, which the servers complete themselves.

import * as vscode from 'vscode';
import { SELECTOR } from './decks';
import type { PythonServers } from './lsp';

/** Registers the completions, from `servers` where they have a server for the file. */
export function registerPython(servers: Promise<PythonServers | undefined>): vscode.Disposable {
	return vscode.languages.registerCompletionItemProvider(
		SELECTOR.map((filter) => ({ ...filter, scheme: 'file' })),
		{
			async provideCompletionItems(document, position, token, context) {
				const fromServer = await (await servers)?.completion(document, position, token, context);
				return fromServer ?? provideCompletionItems(document, position, token, context);
			},
			async resolveCompletionItem(item, token) {
				return (await servers)?.resolve(item, token) ?? item;
			},
		},
		'.',
	);
}

/** The edits of a completion that are in the cells: an import added at the top of the
 * file goes at the top of the first cell, and any other edit, where the markdown is, is
 * left out. */
export function keepInCells(
	edits: vscode.TextEdit[] | undefined,
	shadow: { cells: { start: number; end: number }[] },
	first: vscode.Position,
): vscode.TextEdit[] | undefined {
	return edits?.flatMap((edit) => {
		if (edit.range.isEmpty && edit.range.start.line === 0) {
			return [new vscode.TextEdit(new vscode.Range(first, first), edit.newText)];
		}
		const inside = shadow.cells.some(({ start, end }) => start <= edit.range.start.line && edit.range.end.line < end);
		return inside ? [edit] : [];
	});
}

/** The lines of each code cell's code, from the one after its opening fence up to its
 * closing one, excluded, as the deck finds them: tilde-fenced, which a backtick fence
 * is not, in Python unless its fence names another language. */
export function pythonCells(lines: string[]): { start: number; end: number }[] {
	const cells: { start: number; end: number }[] = [];
	let line = 0;
	// The frontmatter holds no cell.
	if (lines[0]?.trimEnd() === '---') {
		const end = lines.findIndex((text, i) => i > 0 && ['---', '...'].includes(text.trimEnd()));
		if (end > 0) line = end + 1;
	}
	for (; line < lines.length; line++) {
		const open = /^ {0,3}(~{3,}|`{3,})\s*(\S*)/.exec(lines[line]);
		if (!open) continue;
		const fence = open[1];
		const close = new RegExp(`^ {0,3}${fence[0]}{${fence.length},}\\s*$`);
		let end = line + 1;
		while (end < lines.length && !close.test(lines[end])) end++;
		const language = open[2].replace(/^\{\.?|\}$/g, '');
		if (fence[0] === '~' && (language === '' || language === 'python')) cells.push({ start: line + 1, end });
		line = end;
	}
	return cells;
}

async function provideCompletionItems(
	document: vscode.TextDocument,
	position: vscode.Position,
	token: vscode.CancellationToken,
	context: vscode.CompletionContext,
): Promise<vscode.CompletionList | undefined> {
	const lines = document.getText().split(/\r?\n/);
	const cells = pythonCells(lines);
	if (!cells.some(({ start, end }) => start <= position.line && position.line < end)) return undefined;
	const python = lines.map(() => '');
	for (const { start, end } of cells) {
		for (let line = start; line < end; line++) python[line] = lines[line];
	}
	// Hidden, and a new one for each request, as a server may still hold the text it
	// read of an earlier one.
	const name = document.uri.path.split('/').pop();
	const file = vscode.Uri.joinPath(document.uri, '..', `.${name}.${crypto.randomUUID().slice(0, 8)}.py`);
	await vscode.workspace.fs.writeFile(file, new TextEncoder().encode(python.join('\n')));
	try {
		await vscode.workspace.openTextDocument(file);
		if (token.isCancellationRequested) return undefined;
		const list = await vscode.commands.executeCommand<vscode.CompletionList>(
			'vscode.executeCompletionItemProvider',
			file,
			position,
			context.triggerCharacter,
			// Those shown first, with the documentation resolving them gives.
			20,
		);
		const first = new vscode.Position(cells[0].start, 0);
		for (const item of list.items) {
			item.additionalTextEdits = keepInCells(item.additionalTextEdits, { cells }, first);
		}
		return list;
	} finally {
		await vscode.workspace.fs.delete(file).then(undefined, () => {});
	}
}
