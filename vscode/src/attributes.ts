// Completes and describes the attributes the deck reads of a heading's `{ … }` and of an
// HTML element's tag: `steps`, how what a heading holds steps, `step`, the steps an
// element shows in, and `collapse`, that it takes no space while hidden. A `step`'s value
// is completed with the names given to steps on the slide, counted from.

import * as vscode from 'vscode';
import { Decks, SELECTOR } from './decks';

/** A value an attribute takes, and what it does. */
interface Value {
	value: string;
	description: string;
}

/** An attribute the deck reads, and where. */
interface Attribute {
	description: string;
	/** Whether it is read of a heading alone. */
	heading?: boolean;
	/** Whether it is written with no value, as `collapse` is. */
	bare?: boolean;
	/** Whether it can be written in a heading's `{ … }`. */
	block?: boolean;
	values?: Value[];
}

const ATTRIBUTES: Record<string, Attribute> = {
	steps: {
		description: 'How what the heading holds steps, up to the next heading of its level or above.',
		heading: true,
		block: true,
		values: [
			{ value: 'false', description: 'It shows all at once, with the heading.' },
			{ value: 'true', description: 'It steps, though the file\'s frontmatter, or a heading above, says `steps: false`.' },
			{ value: 'parallel', description: 'The `###` columns under it step together: the first step of each at once, then the second of each. Inside a column, a range counts that column\'s steps, its heading 0.' },
			{ value: 'interleave', description: 'The `###` columns under it take turns: the first step of each column, one after another, then the second of each. Inside a column, a range counts that column\'s steps, its heading 0.' },
		],
	},
	step: {
		description: 'The steps the element shows in: with no value, one after the latest; `3` or `3..` from the third on; `2..4` from the second up to the fourth; `..3` from its own up to the third; `+1` one after the step of the element before it that steps, or of the one it is inside; a name, as `a`, a step of its own named so; and `a+1` one after the step named `a`.',
		block: true,
	},
	collapse: {
		description: 'Hidden, the element takes no space, so that two can show in turns in the same place.',
		bare: true,
	},
};

/** Where an attribute is being written: in a heading's `{ … }` or in an HTML tag, of
 * which `tag` is the name, and what of it is written before the cursor. */
interface Context {
	block: boolean;
	tag?: string;
	/** What of the block or the tag is written, from its start. */
	text: string;
	/** What of the block or the tag is written after the cursor, on its line. */
	rest: string;
}

/** How many lines above the cursor a tag may start. */
const TAG_LINES = 10;

function contextAt(document: vscode.TextDocument, position: vscode.Position): Context | undefined {
	const line = document.lineAt(position.line).text;
	const before = line.slice(0, position.character);
	const after = line.slice(position.character);
	const block = /^ {0,3}#{1,6}\s.*\{([^{}]*)$/.exec(before);
	if (block) {
		return { block: true, text: block[1], rest: after.split('}')[0] };
	}
	// A tag, which may span lines, as `<p step="..3"\n  collapse>` does.
	const start = new vscode.Position(Math.max(0, position.line - TAG_LINES), 0);
	const text = document.getText(new vscode.Range(start, position));
	const tag = /<([a-zA-Z][\w-]*)(\s[^<>]*)$/.exec(text);
	if (tag) {
		return { block: false, tag: tag[1].toLowerCase(), text: tag[2], rest: after.split('>')[0] };
	}
	return undefined;
}

/** The attributes the deck reads where `context` is. */
function attributesOf(context: Context): [string, Attribute][] {
	const heading = context.tag !== undefined && /^h[1-6]$/.test(context.tag);
	return Object.entries(ATTRIBUTES).filter(([, attribute]) =>
		context.block ? attribute.block : !attribute.heading || heading);
}

function documentation(name: string, attribute: Attribute, value?: Value): vscode.MarkdownString {
	const text = new vscode.MarkdownString(`**${name}**\n\n${attribute.description}`);
	if (value) text.appendMarkdown(`\n\n\`${name}=${value.value}\`: ${value.description}`);
	return text;
}

/** The names given to steps on the slide the line is in, between the rules around it,
 * as `step="a"`, `{ step=a }`, `\step[a]` or `gid="step=a"` give them. */
function namesOn(document: vscode.TextDocument, line: number): string[] {
	const rule = (at: number) => /^ {0,3}(-{3,}|\*{3,}|_{3,})\s*$/.test(document.lineAt(at).text);
	let first = line;
	while (first > 0 && !rule(first - 1)) first--;
	let last = line;
	while (last < document.lineCount - 1 && !rule(last + 1)) last++;
	const text = document.getText(new vscode.Range(first, 0, last, document.lineAt(last).text.length));
	const names = new Set<string>();
	const name = String.raw`([A-Za-z_]\w*)(?![\w+-])`;
	for (const pattern of [String.raw`\bstep\*?=["']?${name}`, String.raw`\\step\*?\[${name}\]`]) {
		for (const match of text.matchAll(new RegExp(pattern, 'g'))) names.add(match[1]);
	}
	return [...names];
}

export function registerAttributes(decks: Decks): vscode.Disposable {
	return vscode.Disposable.from(
		vscode.languages.registerCompletionItemProvider(SELECTOR, {
			provideCompletionItems: (document, position) =>
				decks.isDeck(document) ? provideCompletionItems(document, position) : undefined,
		}, '{', ' ', '=', '"'),
		vscode.languages.registerHoverProvider(SELECTOR, {
			provideHover: (document, position) => (decks.isDeck(document) ? provideHover(document, position) : undefined),
		}),
	);
}

function provideCompletionItems(document: vscode.TextDocument, position: vscode.Position): vscode.CompletionItem[] | undefined {
	const context = contextAt(document, position);
	if (!context) return undefined;

	// A value, after its attribute's `=`, and its quote in a tag.
	const value = /(?:^|\s)([\w-]+)=(["']?)([^\s"']*)$/.exec(context.text);
	if (value) {
		const [, name, quote, typed] = value;
		const attribute = attributesOf(context).find(([each]) => each === name)?.[1];
		if (!attribute) return undefined;
		const values: Value[] = attribute.values ?? (name === 'step' ? stepValues(document, position.line) : []);
		const range = new vscode.Range(position.translate(0, -typed.length), position);
		// In a tag, a value is quoted, which is closed if it is not yet.
		const [open, close] = context.block || quote ? ['', ''] : ['"', '"'];
		return values.map((each, i) => {
			const item = new vscode.CompletionItem(each.value, vscode.CompletionItemKind.EnumMember);
			item.insertText = `${open}${each.value}${close}`;
			item.range = range;
			item.filterText = `${open}${each.value}`;
			item.documentation = documentation(name, attribute, each);
			item.sortText = String(i).padStart(3, '0');
			return item;
		});
	}

	// An attribute, where one begins, of those not written yet.
	const typed = /(?:^|\s)([\w-]*)$/.exec(context.text);
	if (!typed) return undefined;
	const written = new Set([...`${context.text} ${context.rest}`.matchAll(/(?:^|\s)([\w-]+)(?==|\s|$)/g)].map((match) => match[1]));
	written.delete(typed[1]);
	const range = new vscode.Range(position.translate(0, -typed[1].length), position);
	return attributesOf(context)
		.filter(([name]) => !written.has(name))
		.map(([name, attribute]) => {
			const item = new vscode.CompletionItem(name, vscode.CompletionItemKind.Property);
			item.range = range;
			item.documentation = documentation(name, attribute);
			if (!attribute.bare) {
				item.insertText = new vscode.SnippetString(context.block ? `${name}=$0` : `${name}="$0"`);
				item.command = { command: 'editor.action.triggerSuggest', title: 'Suggest' };
			}
			return item;
		});
}

/** The values a `step` is completed with: one after the step before, with it, and with
 * or after each step named on the slide. */
function stepValues(document: vscode.TextDocument, line: number): Value[] {
	return [
		{ value: '+1', description: 'One step after the element before it that steps, or the one it is inside.' },
		{ value: '+0', description: 'With the element before it that steps, or the one it is inside.' },
		...namesOn(document, line).flatMap((name) => [
			{ value: `${name}+0`, description: `With the step named \`${name}\`.` },
			{ value: `${name}+1`, description: `One step after the step named \`${name}\`.` },
		]),
	];
}

function provideHover(document: vscode.TextDocument, position: vscode.Position): vscode.Hover | undefined {
	const range = document.getWordRangeAtPosition(position, /[\w-]+/);
	if (!range) return undefined;
	const name = document.getText(range);
	const context = contextAt(document, range.end);
	if (!context || !/(?:^|\s)[\w-]+$/.test(context.text)) return undefined;
	const attribute = attributesOf(context).find(([each]) => each === name)?.[1];
	if (!attribute) return undefined;
	const value = attribute.values?.find((each) => document.lineAt(range.end.line).text.slice(range.end.character).match(/^=["']?([\w-]*)/)?.[1] === each.value);
	return new vscode.Hover(documentation(name, attribute, value), range);
}
