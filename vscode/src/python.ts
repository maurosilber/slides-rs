// The Python of a deck's code cells in the markdown text editor: its completions, hovers,
// signature help, definitions and references, from the Python language server lsp.ts
// starts in the environment the cells run in, or, without one, from whichever Python
// language server VS Code has, as it has them of a Python file. For that one, the cells
// are written, in order, as the kernel runs them, to a file of their own, next to the
// deck, so that the server resolves its imports as the deck's, each line where it is in
// the markdown and the rest blank. Asked of that file, the server's answers hold for the
// markdown as they are. A notebook's cells are Python documents already, which the
// servers have themselves.

import * as vscode from 'vscode';
import { SELECTOR } from './decks';
import type { Found, PythonServers } from './lsp';

type Cells = { start: number; end: number }[];

/** Registers what the cells have, from `servers` where they have a server for the file,
 * or else from the server VS Code has. */
export function registerPython(servers: Promise<PythonServers | undefined>): vscode.Disposable {
	const selector = SELECTOR.map((filter) => ({ ...filter, scheme: 'file' }));
	/** What a server says at a position: the one started for the document, or else, with
	 * `forwarded`, the one VS Code has. */
	const ask = async <T>(
		document: vscode.TextDocument,
		position: vscode.Position,
		token: vscode.CancellationToken,
		started: (servers: PythonServers, found: Found) => Promise<T>,
		forwarded: (file: vscode.Uri, cells: Cells) => Thenable<T>,
	): Promise<T | undefined> => {
		const found = await (await servers)?.serverFor(document);
		if (found) {
			return inCells(found.shadow.cells, position) ? started((await servers)!, found) : undefined;
		}
		return forward(document, position, token, forwarded);
	};
	/** A location in the hidden file is in the document. */
	const fix = (file: vscode.Uri, document: vscode.TextDocument) => (uri: vscode.Uri) => (uri.toString() === file.toString() ? document.uri : uri);
	return vscode.Disposable.from(
		vscode.languages.registerCompletionItemProvider(
			selector,
			{
				provideCompletionItems: (document, position, token, context) =>
					ask(
						document,
						position,
						token,
						(servers, found) => servers.completion(found, position, token, context),
						async (file, cells) => {
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
						},
					),
				async resolveCompletionItem(item, token) {
					return (await servers)?.resolve(item, token) ?? item;
				},
			},
			'.',
		),
		vscode.languages.registerHoverProvider(selector, {
			provideHover: (document, position, token) =>
				ask(
					document,
					position,
					token,
					(servers, found) => servers.hover(found, position, token),
					async (file) => {
						const hovers = await vscode.commands.executeCommand<vscode.Hover[]>('vscode.executeHoverProvider', file, position);
						return hovers.length ? new vscode.Hover(hovers.flatMap((hover) => hover.contents)) : undefined;
					},
				),
		}),
		vscode.languages.registerSignatureHelpProvider(
			selector,
			{
				provideSignatureHelp: (document, position, token, context) =>
					ask(
						document,
						position,
						token,
						(servers, found) => servers.signatureHelp(found, position, token, context),
						(file) => vscode.commands.executeCommand<vscode.SignatureHelp | undefined>('vscode.executeSignatureHelpProvider', file, position, context.triggerCharacter),
					),
			},
			'(',
			',',
		),
		vscode.languages.registerDefinitionProvider(selector, {
			provideDefinition: (document, position, token) =>
				ask(
					document,
					position,
					token,
					(servers, found) => servers.definition(found, document, position, token),
					async (file) => {
						const links = await vscode.commands.executeCommand<(vscode.Location | vscode.LocationLink)[]>('vscode.executeDefinitionProvider', file, position);
						const moved = fix(file, document);
						return links.map((link) =>
							'targetUri' in link ? { ...link, targetUri: moved(link.targetUri) } : new vscode.Location(moved(link.uri), link.range),
						) as vscode.Location[] | vscode.LocationLink[];
					},
				),
		}),
		vscode.languages.registerReferenceProvider(selector, {
			provideReferences: (document, position, context, token) =>
				ask(
					document,
					position,
					token,
					(servers, found) => servers.references(found, document, position, context, token),
					async (file) => {
						const locations = await vscode.commands.executeCommand<vscode.Location[]>('vscode.executeReferenceProvider', file, position);
						const moved = fix(file, document);
						return locations.map((location) => new vscode.Location(moved(location.uri), location.range));
					},
				),
		}),
	);
}

/** Whether the position is in one of the cells. */
function inCells(cells: Cells, position: vscode.Position): boolean {
	return cells.some(({ start, end }) => start <= position.line && position.line < end);
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

/** Asks the Python language server VS Code has, with `ask`, of a hidden file of the
 * document's cells, if the position is in one, written next to it for the request. */
async function forward<T>(
	document: vscode.TextDocument,
	position: vscode.Position,
	token: vscode.CancellationToken,
	ask: (file: vscode.Uri, cells: Cells) => Thenable<T>,
): Promise<T | undefined> {
	const lines = document.getText().split(/\r?\n/);
	const cells = pythonCells(lines);
	if (!inCells(cells, position)) return undefined;
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
		return await ask(file, cells);
	} finally {
		await vscode.workspace.fs.delete(file).then(undefined, () => {});
	}
}
