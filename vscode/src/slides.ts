// The slides of a deck's markdown in the text editor: where each one begins, marked on
// the rule or the import that breaks it from the one before, and buttons above each to
// move it, with all its markdown, before another one, or to add one after it. Where
// they break is the deck's own reading of the markdown, which the module answers.

import * as vscode from 'vscode';
import { Decks, SELECTOR } from './decks';
import { Breaks, Module } from './wasm';

/** How long to wait, after an edit, before marking the slides again. */
const DELAY = 150;

/** A slide of the markdown, by its first and last lines that are not blank. */
interface Slide {
	first: number;
	last: number;
	/** Its heading, or else its first line, to tell it apart. */
	title: string;
}

/** The slides of a version of a document. */
interface Layout {
	version: number;
	breaks: Breaks;
	/** The slides that are not empty, which are the ones the deck shows. */
	slides: Slide[];
}

export class SlideEditor implements vscode.Disposable {
	/** The layout of the last version asked for of each document, by uri. */
	private readonly layouts = new Map<string, Promise<Layout>>();
	private readonly lensesChanged = new vscode.EventEmitter<void>();
	private readonly timers = new Map<string, ReturnType<typeof setTimeout>>();
	private readonly disposables: vscode.Disposable[] = [];
	/** Each break, on its line, with what follows it written after. */
	private readonly decoration = vscode.window.createTextEditorDecorationType({
		isWholeLine: true,
		backgroundColor: new vscode.ThemeColor('slides.slideBreak'),
		overviewRulerColor: new vscode.ThemeColor('slides.slideBreak'),
		overviewRulerLane: vscode.OverviewRulerLane.Center,
		after: {
			color: new vscode.ThemeColor('editorCodeLens.foreground'),
			fontStyle: 'italic',
			margin: '0 0 0 2em',
		},
	});

	constructor(
		private readonly module: Module,
		private readonly decks: Decks,
		private readonly log: vscode.LogOutputChannel,
	) {
		this.disposables.push(
			this.decoration,
			this.lensesChanged,
			vscode.languages.registerCodeLensProvider(SELECTOR, {
				onDidChangeCodeLenses: this.lensesChanged.event,
				provideCodeLenses: (document) => this.codeLenses(document),
			}),
			vscode.commands.registerCommand('slides.moveSlideUp', (uri?: vscode.Uri, index?: number) =>
				this.run(uri, index, (editor, layout, i) => this.move(editor, layout, i, i - 1))),
			vscode.commands.registerCommand('slides.moveSlideDown', (uri?: vscode.Uri, index?: number) =>
				this.run(uri, index, (editor, layout, i) => this.move(editor, layout, i, i + 2))),
			vscode.commands.registerCommand('slides.moveSlideTo', (uri?: vscode.Uri, index?: number) =>
				this.run(uri, index, (editor, layout, i) => this.moveTo(editor, layout, i))),
			vscode.commands.registerCommand('slides.newSlide', (uri?: vscode.Uri, index?: number) =>
				this.run(uri, index, (editor, layout, i) => this.add(editor, layout, i))),
			vscode.window.onDidChangeVisibleTextEditors((editors) => editors.forEach((editor) => void this.decorate(editor))),
			vscode.workspace.onDidChangeTextDocument(({ document }) => this.later(document)),
			vscode.workspace.onDidCloseTextDocument((document) => this.layouts.delete(document.uri.toString())),
			this.decks.onDidChange(() => {
				vscode.window.visibleTextEditors.forEach((editor) => void this.decorate(editor));
				this.lensesChanged.fire();
			}),
		);
		vscode.window.visibleTextEditors.forEach((editor) => void this.decorate(editor));
	}

	/** The slides of the document as it is now. */
	private layout(document: vscode.TextDocument): Promise<Layout> {
		const key = document.uri.toString();
		const version = document.version;
		const cached = this.layouts.get(key);
		if (cached) {
			return cached.then((layout) => (layout.version === version ? layout : this.compute(document)));
		}
		return this.compute(document);
	}

	private compute(document: vscode.TextDocument): Promise<Layout> {
		const version = document.version;
		const text = document.getText();
		const layout = this.module.slides(text).then((breaks) => ({ version, breaks, slides: slidesOf(text.split(/\r?\n/), breaks) }));
		this.layouts.set(document.uri.toString(), layout);
		layout.catch((error) => {
			this.log.error(`${vscode.workspace.asRelativePath(document.uri)}: could not find its slides: ${error}`);
			this.layouts.delete(document.uri.toString());
		});
		return layout;
	}

	/** Marks the slides of the document again, once it has not changed for a moment. */
	private later(document: vscode.TextDocument) {
		const key = document.uri.toString();
		clearTimeout(this.timers.get(key));
		this.timers.set(key, setTimeout(() => {
			this.timers.delete(key);
			vscode.window.visibleTextEditors
				.filter((editor) => editor.document === document)
				.forEach((editor) => void this.decorate(editor));
		}, DELAY));
	}

	/** Marks where each slide of the editor's document begins. */
	private async decorate(editor: vscode.TextEditor) {
		const document = editor.document;
		if (!this.decks.isDeck(document)) {
			editor.setDecorations(this.decoration, []);
			return;
		}
		let layout: Layout;
		try {
			layout = await this.layout(document);
		} catch {
			return;
		}
		if (layout.version !== document.version) {
			return;
		}
		const mark = (line: number, text: string): vscode.DecorationOptions => ({
			range: new vscode.Range(line, 0, line, 0),
			renderOptions: { after: { contentText: text } },
		});
		const rules = layout.breaks.rules.map((line) => {
			const next = layout.slides.findIndex((slide) => slide.first > line);
			return mark(line, next < 0 ? 'end' : `slide ${next + 1}: ${layout.slides[next].title}`);
		});
		const imports = layout.breaks.imports.map((line) => mark(line, 'its slides, imported'));
		editor.setDecorations(this.decoration, [...rules, ...imports]);
	}

	/** Buttons above each slide, to move it or to add one after it. */
	private async codeLenses(document: vscode.TextDocument): Promise<vscode.CodeLens[]> {
		if (!this.decks.isDeck(document)) {
			return [];
		}
		const { slides } = await this.layout(document);
		const uri = document.uri;
		return slides.flatMap((slide, i) => {
			const range = new vscode.Range(slide.first, 0, slide.first, 0);
			const lens = (title: string, command: string, tooltip: string) =>
				new vscode.CodeLens(range, { title, command, tooltip, arguments: [uri, i] });
			return [
				...(i > 0 ? [lens('$(arrow-up) Move up', 'slides.moveSlideUp', 'Move this slide before the one above it')] : []),
				...(i < slides.length - 1 ? [lens('$(arrow-down) Move down', 'slides.moveSlideDown', 'Move this slide after the one below it')] : []),
				...(slides.length > 1 ? [lens('$(list-ordered) Move to…', 'slides.moveSlideTo', 'Move this slide before another one')] : []),
				lens('$(add) New slide', 'slides.newSlide', 'Add a slide after this one'),
			];
		});
	}

	/** Runs a command on the slide at `index` of the file at `uri`, from its buttons, or
	 * else on the slide the cursor is in, in the active editor. */
	private async run(
		uri: vscode.Uri | undefined,
		index: number | undefined,
		command: (editor: vscode.TextEditor, layout: Layout, index: number) => Thenable<unknown>,
	) {
		const active = vscode.window.activeTextEditor;
		const editor = uri && active?.document.uri.toString() !== uri.toString()
			? await vscode.window.showTextDocument(uri)
			: active;
		if (!editor) {
			return;
		}
		const layout = await this.layout(editor.document);
		if (layout.version !== editor.document.version) {
			return;
		}
		if (index === undefined) {
			const line = editor.selection.active.line;
			index = Math.max(0, layout.slides.filter((slide) => slide.first <= line).length - 1);
		}
		await command(editor, layout, index);
	}

	/** Asks which slide to move the one at `from` before, and moves it. */
	private async moveTo(editor: vscode.TextEditor, layout: Layout, from: number) {
		const items = layout.slides
			.map((slide, to) => ({ label: `Before slide ${to + 1}`, description: slide.title, to }))
			.filter(({ to }) => to !== from && to !== from + 1);
		if (from !== layout.slides.length - 1) {
			items.push({ label: 'At the end', description: '', to: layout.slides.length });
		}
		const picked = await vscode.window.showQuickPick(items, { title: `Move slide ${from + 1}: ${layout.slides[from].title}` });
		if (picked) {
			await this.move(editor, layout, from, picked.to);
		}
	}

	/**
	 * Moves the slide at `from`, with all its markdown, before the one at `to`, or after
	 * the last with `to` the number of slides. What is between two slides, as the rule and
	 * the blank lines around it, stays where it is, and the slides move around it.
	 */
	private async move(editor: vscode.TextEditor, layout: Layout, from: number, to: number) {
		const { slides } = layout;
		if (from < 0 || from >= slides.length || to < 0 || to > slides.length || to === from || to === from + 1) {
			return;
		}
		const order = slides.map((_, i) => i);
		order.splice(from, 1);
		order.splice(to > from ? to - 1 : to, 0, from);
		const low = Math.min(from, to);
		const high = Math.max(from, to - 1);
		const document = editor.document;
		const end = (line: number) => document.lineAt(line).range.end;
		const content = (i: number) => document.getText(new vscode.Range(new vscode.Position(slides[i].first, 0), end(slides[i].last)));
		const gap = (i: number) => document.getText(new vscode.Range(end(slides[i].last), new vscode.Position(slides[i + 1].first, 0)));
		let text = '';
		for (let i = low; i <= high; i++) {
			text += content(order[i]);
			if (i < high) {
				text += gap(i);
			}
		}
		const region = new vscode.Range(new vscode.Position(slides[low].first, 0), end(slides[high].last));
		if (!(await editor.edit((edit) => edit.replace(region, text)))) {
			return;
		}
		const moved = (await this.layout(document)).slides[order.indexOf(from)];
		if (moved) {
			const start = new vscode.Position(moved.first, 0);
			editor.selection = new vscode.Selection(start, start);
			editor.revealRange(new vscode.Range(start, start), vscode.TextEditorRevealType.InCenterIfOutsideViewport);
		}
	}

	/** Adds a slide after the one at `index`, with a heading to fill in. */
	private async add(editor: vscode.TextEditor, layout: Layout, index: number) {
		const slide = layout.slides[index];
		if (slide) {
			const end = editor.document.lineAt(slide.last).range.end;
			await editor.insertSnippet(new vscode.SnippetString('\n\n---\n\n# ${1:Title}'), end);
		} else {
			await editor.insertSnippet(new vscode.SnippetString('# ${1:Title}\n'), new vscode.Position(layout.breaks.start, 0));
		}
	}

	dispose() {
		this.timers.forEach((timer) => clearTimeout(timer));
		this.disposables.forEach((disposable) => disposable.dispose());
	}
}

/** The slides between the breaks that are not empty, as the deck leaves the others out. */
function slidesOf(lines: string[], breaks: Breaks): Slide[] {
	const bounds = [breaks.start - 1, ...breaks.rules, lines.length];
	const blank = (line: number) => /^\s*$/.test(lines[line]);
	const slides: Slide[] = [];
	for (let i = 0; i + 1 < bounds.length; i++) {
		let first = bounds[i] + 1;
		let last = bounds[i + 1] - 1;
		while (first <= last && blank(first)) first++;
		while (last >= first && blank(last)) last--;
		if (first <= last) {
			slides.push({ first, last, title: titleOf(lines.slice(first, last + 1)) });
		}
	}
	return slides;
}

/** The text of a slide's first heading, without its attributes, or else its first line.
 * A line of a fenced block is code, even if it starts with a `#`. */
function titleOf(lines: string[]): string {
	let fence: string | undefined;
	let heading: string | undefined;
	for (const line of lines) {
		const opening = /^ {0,3}(`{3,}|~{3,})/.exec(line)?.[1][0];
		if (opening) {
			fence = fence === undefined ? opening : fence === opening ? undefined : fence;
		} else if (fence === undefined) {
			heading = /^#{1,6}\s+(.*?)(\s*\{[^}]*\})?\s*$/.exec(line)?.[1];
			if (heading) {
				break;
			}
		}
	}
	const title = heading ?? lines[0].trim();
	return title.length > 50 ? `${title.slice(0, 49)}…` : title;
}
