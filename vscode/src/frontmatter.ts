// Completes and describes the keys of a deck's YAML frontmatter, and their values, as
// the deck's own schema, src/frontmatter.schema.json, has them. VS Code applies a JSON
// schema to YAML files, but not to the frontmatter of a markdown file.

import * as vscode from 'vscode';
import { SELECTOR } from './decks';
import schema from '../../src/frontmatter.schema.json';

/** What the schema, which schemars writes from the deck's Rust, has for a key. */
interface Schema {
	description?: string;
	type?: string | string[];
	anyOf?: Schema[];
	properties?: Record<string, Schema>;
	examples?: unknown[];
	minimum?: number;
}

export function registerFrontmatter(): vscode.Disposable {
	// A deck's markdown, or a notebook's markdown cell, the first of which holds the
	// frontmatter of a deck opened as one.
	return vscode.Disposable.from(
		// Keys, once asked for, as markdown asks for no suggestions as it is typed, and
		// values after the space that follows a key.
		vscode.languages.registerCompletionItemProvider(SELECTOR, { provideCompletionItems }, ' '),
		vscode.languages.registerHoverProvider(SELECTOR, { provideHover }),
	);
}

/** The lines of the frontmatter, between the `---` that opens the document and the
 * one that closes it, if the document has one. */
function frontmatter(document: vscode.TextDocument): { start: number; end: number } | undefined {
	if (document.lineCount < 2 || document.lineAt(0).text.trimEnd() !== '---') return undefined;
	for (let line = 1; line < document.lineCount; line++) {
		const text = document.lineAt(line).text.trimEnd();
		if (text === '---' || text === '...') return { start: 1, end: line };
	}
	return undefined;
}

const KEY = /^(\s*)([\w-]+):(.*)$/;

function indentation(text: string): number {
	return text.length - text.trimStart().length;
}

/** The keys that hold a line indented `indent`, outermost first, as the lines above it
 * that are indented less say. */
function parents(document: vscode.TextDocument, start: number, line: number, indent: number): string[] {
	const keys: string[] = [];
	for (let above = line - 1; above >= start && indent > 0; above--) {
		const match = KEY.exec(document.lineAt(above).text);
		if (match && match[1].length < indent && !match[3].trim()) {
			keys.unshift(match[2]);
			indent = match[1].length;
		}
	}
	return keys;
}

/** What the schema has for the keys, in turn. */
function at(keys: string[]): Schema | undefined {
	let node: Schema | undefined = schema as Schema;
	for (const key of keys) node = node?.properties?.[key];
	return node;
}

function isObject(node: Schema): boolean {
	return node.type === 'object' || node.properties !== undefined;
}

/** The types a key can hold, as its own or those of any of its alternatives, but for
 * null, which leaving it unset is. */
function typesOf(node: Schema): string[] {
	const types = [node.type ?? [], ...(node.anyOf ?? []).map(typesOf)].flat();
	return [...new Set(types)].filter((type) => type !== 'null');
}

function documentation(key: string, node: Schema): vscode.MarkdownString {
	const types = typesOf(node).join(' or ');
	const text = new vscode.MarkdownString(`**${key}**${types ? ` (${types})` : ''}\n\n${node.description ?? ''}`);
	if (node.examples?.length) text.appendMarkdown(`\n\nFor example: ${node.examples.map((value) => `\`${value}\``).join(', ')}`);
	return text;
}

function provideCompletionItems(document: vscode.TextDocument, position: vscode.Position): vscode.CompletionItem[] | undefined {
	const lines = frontmatter(document);
	if (!lines || position.line < lines.start || position.line >= lines.end) return undefined;
	const before = document.lineAt(position.line).text.slice(0, position.character);

	// A value, after its key.
	const value = KEY.exec(before);
	if (value) {
		const keys = [...parents(document, lines.start, position.line, value[1].length), value[2]];
		const node = at(keys);
		if (!node || isObject(node)) return undefined;
		const types = typesOf(node);
		const values = [...(types.includes('boolean') ? [true, false] : []), ...(node.examples ?? [])];
		const leading = value[3].startsWith(' ') ? '' : ' ';
		return values.map((each, i) => {
			const item = new vscode.CompletionItem(String(each), vscode.CompletionItemKind.Value);
			item.insertText = leading + String(each);
			item.range = new vscode.Range(position.line, before.length - value[3].trimStart().length, position.line, position.character);
			item.documentation = documentation(value[2], node);
			item.sortText = String(i).padStart(3, '0');
			return item;
		});
	}

	// A key, where one begins, of those its mapping has not set yet.
	const key = /^(\s*)([\w-]*)$/.exec(before);
	if (!key) return undefined;
	const indent = key[1].length;
	const keys = parents(document, lines.start, position.line, indent);
	const properties = at(keys)?.properties;
	if (!properties) return undefined;
	const set = new Set<string>();
	for (let line = lines.start; line < lines.end; line++) {
		const match = KEY.exec(document.lineAt(line).text);
		if (line !== position.line && match && match[1].length === indent && parents(document, lines.start, line, indent).join('.') === keys.join('.')) {
			set.add(match[2]);
		}
	}
	return Object.entries(properties)
		.filter(([name]) => !set.has(name))
		.map(([name, node]) => {
			const item = new vscode.CompletionItem(name, vscode.CompletionItemKind.Property);
			item.documentation = documentation(name, node);
			if (isObject(node)) {
				// The keys it holds come next, indented under it, as the snippet indents
				// its lines as the one it is on, and a tab as the editor does.
				item.insertText = new vscode.SnippetString(`${name}:\n\t$0`);
			} else {
				item.insertText = `${name}: `;
			}
			item.command = { command: 'editor.action.triggerSuggest', title: 'Suggest' };
			return item;
		});
}

function provideHover(document: vscode.TextDocument, position: vscode.Position): vscode.Hover | undefined {
	const lines = frontmatter(document);
	if (!lines || position.line < lines.start || position.line >= lines.end) return undefined;
	const match = KEY.exec(document.lineAt(position.line).text);
	if (!match) return undefined;
	const start = match[1].length;
	const end = start + match[2].length;
	if (position.character < start || position.character > end) return undefined;
	const node = at([...parents(document, lines.start, position.line, start), match[2]]);
	if (!node) return undefined;
	return new vscode.Hover(documentation(match[2], node), new vscode.Range(position.line, start, position.line, end));
}
