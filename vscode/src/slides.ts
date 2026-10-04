// The slides of a deck's markdown in the text editor: where each one begins, marked on
// the rule or the import that breaks it from the one before, every other slide shaded,
// its headings by their level, its columns, the step each part shows in, and buttons
// under the break, or under the frontmatter for the first, to move it, with all its
// markdown, before another one, or to add one before or after it. Where they break,
// and how they step, is the deck's own reading of the markdown, which the module
// answers, along with what is wrong with how they step, as warnings on their lines. The
// `slides.editor` settings turn each of the marks off.

import * as vscode from 'vscode';
import { Decks, SELECTOR } from './decks';
import { Breaks, Module, SlideSteps } from './wasm';

/** How long to wait, after an edit, before marking the slides again. */
const DELAY = 150;

/** A slide of the markdown, by its first and last lines that are not blank. */
interface Slide {
	/** The lines between the break before it and the one after, blank ones too. */
	start: number;
	end: number;
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
	/** The steps of each slide. */
	steps: SlideSteps[];
}

/** Which of the marks the `slides.editor` settings show. */
interface Shown {
	alternateSlides: boolean;
	headings: boolean;
	columns: boolean;
	steps: boolean;
}

function shown(document: vscode.TextDocument): Shown {
	const settings = vscode.workspace.getConfiguration('slides.editor', document);
	return {
		alternateSlides: settings.get('alternateSlides', true),
		headings: settings.get('headings', true),
		columns: settings.get('columns', true),
		steps: settings.get('steps', true),
	};
}

/** The level of the headings that start the columns, as slides.js boxes them. */
const COLUMN = 3;

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
	/** Every other slide, so that two in a row tell apart. */
	private readonly shade = vscode.window.createTextEditorDecorationType({
		isWholeLine: true,
		backgroundColor: new vscode.ThemeColor('slides.alternateSlide'),
	});
	/** The title, the subtitles and the columns' headings, each its own way. */
	private readonly headings = (['slides.title', 'slides.subtitle', 'slides.columnHeading'] as const).map((color) =>
		vscode.window.createTextEditorDecorationType({
			isWholeLine: true,
			backgroundColor: new vscode.ThemeColor(color),
			fontWeight: 'bold',
		}));
	/** Before every line, in a column of their own, the steps it shows in, and the line
	 * down the column of the slide it is in, between them and the text, so that every
	 * line is moved over as much. */
	private readonly gutter = vscode.window.createTextEditorDecorationType({
		before: {
			color: new vscode.ThemeColor('slides.step'),
		},
	});
	/** Whether the steps' column is narrowed to a mark on each line that steps. */
	private collapsed = false;
	/** What is wrong with how the slides of each document step. */
	private readonly diagnostics = vscode.languages.createDiagnosticCollection('slides');

	constructor(
		private readonly module: Module,
		private readonly decks: Decks,
		private readonly log: vscode.LogOutputChannel,
	) {
		this.disposables.push(
			this.decoration,
			this.shade,
			...this.headings,
			this.gutter,
			this.lensesChanged,
			this.diagnostics,
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
			vscode.commands.registerCommand('slides.newSlideAbove', (uri?: vscode.Uri, index?: number) =>
				this.run(uri, index, (editor, layout, i) => this.addBefore(editor, layout, i))),
			vscode.commands.registerCommand('slides.toggleStepColumn', () => {
				this.collapsed = !this.collapsed;
				this.refresh();
			}),
			vscode.window.onDidChangeVisibleTextEditors((editors) => editors.forEach((editor) => void this.decorate(editor))),
			vscode.workspace.onDidChangeTextDocument(({ document }) => this.later(document)),
			vscode.workspace.onDidCloseTextDocument((document) => {
				this.layouts.delete(document.uri.toString());
				this.diagnostics.delete(document.uri);
			}),
			this.decks.onDidChange(() => this.refresh()),
			vscode.workspace.onDidChangeConfiguration((event) => {
				if (event.affectsConfiguration('slides.editor')) {
					this.refresh();
				}
			}),
		);
		vscode.window.visibleTextEditors.forEach((editor) => void this.decorate(editor));
	}

	/** Marks the slides of every editor again, and their buttons. */
	private refresh() {
		vscode.window.visibleTextEditors.forEach((editor) => void this.decorate(editor));
		this.lensesChanged.fire();
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
		const layout = Promise.all([
			this.module.slides(text),
			// Found even while they are not shown, for what is wrong with them.
			this.module.steps(text),
		]).then(([breaks, steps]) => ({ version, breaks, slides: slidesOf(text.split(/\r?\n/), breaks), steps }));
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
		const types = [this.decoration, this.shade, ...this.headings, this.gutter];
		if (!this.decks.isDeck(document)) {
			types.forEach((type) => editor.setDecorations(type, []));
			this.diagnostics.delete(document.uri);
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
		const imports = layout.breaks.imports.map(({ line }) => mark(line, 'its slides, imported'));
		editor.setDecorations(this.decoration, [...rules, ...imports]);
		const show = shown(document);
		editor.setDecorations(this.shade, show.alternateSlides
			? layout.slides.filter((_, i) => i % 2 === 1).map((slide) => new vscode.Range(slide.start, 0, slide.end, 0))
			: []);
		const headings = show.headings ? layout.breaks.headings : [];
		this.headings.forEach((type, i) => editor.setDecorations(type, headings
			.filter((heading) => heading.level === i + 1)
			.map((heading) => new vscode.Range(heading.line, 0, heading.last, 0))));
		const columns = show.columns ? columnsOf(document, layout) : [];
		const steps = show.steps ? stepMarks(layout.steps) : undefined;
		editor.setDecorations(this.gutter, gutterOf(document.lineCount, columns, steps, this.collapsed));
		this.diagnostics.set(document.uri, layout.steps
			.flatMap((slide) => slide.warnings)
			.filter(({ line }) => line < document.lineCount)
			.map(({ line, message }) => {
				const diagnostic = new vscode.Diagnostic(document.lineAt(line).range, message, vscode.DiagnosticSeverity.Warning);
				diagnostic.source = 'slides';
				return diagnostic;
			}));
	}

	/** Buttons at the top of each slide, under its break, to move it or to add one before
	 * or after it. */
	private async codeLenses(document: vscode.TextDocument): Promise<vscode.CodeLens[]> {
		if (!this.decks.isDeck(document)) {
			return [];
		}
		const layout = await this.layout(document);
		const { slides } = layout;
		const counts = shown(document).steps ? countsOf(layout) : [];
		const uri = document.uri;
		return slides.flatMap((slide, i) => {
			const range = new vscode.Range(slide.start, 0, slide.start, 0);
			const lens = (title: string, command: string, tooltip: string) =>
				new vscode.CodeLens(range, { title, command, tooltip, arguments: [uri, i] });
			const count = counts[i];
			return [
				// Only to read, as a lens with no command is.
				...(count === undefined ? [] : [lens(
					`${count} ${count === 1 ? 'step' : 'steps'}`,
					'slides.toggleStepColumn',
					`How many steps the slide shows in. ${this.collapsed ? 'Expand' : 'Collapse'} the steps' column`,
				)]),
				...(i > 0 ? [lens('$(arrow-up) Move up', 'slides.moveSlideUp', 'Move this slide before the one above it')] : []),
				...(i < slides.length - 1 ? [lens('$(arrow-down) Move down', 'slides.moveSlideDown', 'Move this slide after the one below it')] : []),
				...(slides.length > 1 ? [lens('$(list-ordered) Move to…', 'slides.moveSlideTo', 'Move this slide before another one')] : []),
				lens('$(add) New slide above', 'slides.newSlideAbove', 'Add a slide before this one'),
				lens('$(add) New slide below', 'slides.newSlide', 'Add a slide after this one'),
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
			index = Math.max(0, layout.slides.filter((slide) => slide.start <= line).length - 1);
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

	/** Adds a slide before the one at `index`, with a heading to fill in. */
	private async addBefore(editor: vscode.TextEditor, layout: Layout, index: number) {
		const slide = layout.slides[index];
		if (slide) {
			await editor.insertSnippet(new vscode.SnippetString('# ${1:Title}\n\n---\n\n'), new vscode.Position(slide.first, 0));
		} else {
			await this.add(editor, layout, index);
		}
	}

	dispose() {
		this.timers.forEach((timer) => clearTimeout(timer));
		this.disposables.forEach((disposable) => disposable.dispose());
	}
}

/** A column of a slide, from its heading to the last line before the next heading of its
 * level or above, and which of the columns side by side it is, from 0. */
interface Column {
	range: vscode.Range;
	index: number;
}

/** The columns of the slides, as slides.js boxes them: from each heading of their level
 * up to the next one, or to one above it, which ends the row, as the slide's end does. */
function columnsOf(document: vscode.TextDocument, layout: Layout): Column[] {
	const columns: Column[] = [];
	for (const slide of layout.slides) {
		// A heading under the columns' level is in one, rather than its end.
		const headings = layout.breaks.headings.filter((heading) =>
			heading.level <= COLUMN && slide.first <= heading.line && heading.line <= slide.last);
		let index = 0;
		headings.forEach((heading, i) => {
			if (heading.level !== COLUMN) {
				index = 0;
				return;
			}
			let last = (headings[i + 1]?.line ?? slide.last + 1) - 1;
			while (last > heading.last && document.lineAt(last).isEmptyOrWhitespace) last--;
			columns.push({ range: new vscode.Range(heading.line, 0, last, 0), index: index++ });
		});
	}
	return columns;
}

/** The steps written for each line that steps, by line: `3` from the third step on,
 * `2..4` from the second up to the fourth, excluded, as a range of the deck is written,
 * and with a `*` if it takes no space while hidden. */
function stepMarks(slides: SlideSteps[]): Map<number, string> {
	const lines = new Map<number, string[]>();
	for (const { line, from, to, collapse } of slides.flatMap((slide) => slide.steps)) {
		const text = `${to === null ? from : `${from}..${to}`}${collapse ? '*' : ''}`;
		lines.set(line, [...(lines.get(line) ?? []), text]);
	}
	return new Map([...lines].map(([line, steps]) => [line, steps.join(' · ')]));
}

/** A space that is kept, as one at the start of a decoration's text is not. */
const NBSP = '\u00a0';

/** What is written before each line: the steps it shows in, if they are shown, in a
 * column as wide as the widest of them, or a mark if it is collapsed, and the line down
 * the column of the slide it is in, if they are shown, or as much space outside one. The
 * steps and the line are a single decoration, the line its right border, as the editor
 * keeps no order between those of two types before the same line. */
function gutterOf(
	lineCount: number,
	columns: Column[],
	steps: Map<number, string> | undefined,
	collapsed: boolean,
): vscode.DecorationOptions[] {
	if (columns.length === 0 && steps === undefined) {
		return [];
	}
	const bars: (number | undefined)[] = [];
	for (const { range, index } of columns) {
		for (let line = range.start.line; line <= range.end.line; line++) {
			bars[line] = index;
		}
	}
	// A space on each side of the steps, or of the mark.
	const widest = Math.max(1, ...[...(steps?.values() ?? [])].map((text) => text.length));
	const width = steps === undefined ? '0' : `${(collapsed ? 1 : widest) + 2}ch`;
	const background = steps === undefined ? undefined : new vscode.ThemeColor('slides.stepColumn');
	let hover: vscode.MarkdownString | undefined;
	if (steps !== undefined) {
		const [icon, verb] = collapsed ? ['unfold', 'Expand'] : ['fold', 'Collapse'];
		hover = new vscode.MarkdownString(`[$(${icon}) ${verb} the steps' column](command:slides.toggleStepColumn)`, true);
		hover.isTrusted = { enabledCommands: ['slides.toggleStepColumn'] };
	}
	const options: vscode.DecorationOptions[] = [];
	for (let line = 0; line < lineCount; line++) {
		const text = steps?.get(line);
		const bar = bars[line];
		options.push({
			range: new vscode.Range(line, 0, line, 0),
			hoverMessage: hover,
			renderOptions: {
				before: {
					// Never empty, for the editor to write it at all.
					contentText: text === undefined ? NBSP : `${NBSP}${collapsed ? '•' : text}`,
					backgroundColor: background,
					width,
					// The editor takes only a whole border, which this one, after it, narrows
					// to its right side.
					border: columns.length === 0 ? undefined : 'none; border-right: 3px solid',
					borderColor: columns.length === 0 ? undefined : bar === undefined
						? 'transparent'
						: new vscode.ThemeColor(bar % 2 === 0 ? 'slides.column' : 'slides.alternateColumn'),
					margin: '0 1ch 0 0',
				},
			},
		});
	}
	return options;
}

/** How many steps each slide has, by its index, as the deck numbers them. The module's
 * slides open after each break, those of the first at the file's start, and an empty one
 * is none of the editor's. */
function countsOf(layout: Layout): (number | undefined)[] {
	const counts: (number | undefined)[] = layout.slides.map(() => undefined);
	for (const { line, count } of layout.steps) {
		const at = Math.max(line, layout.breaks.start);
		const index = layout.slides.findIndex((slide) => slide.start <= at && at <= slide.end);
		if (index >= 0) {
			counts[index] ??= count;
		}
	}
	return counts;
}

/** The slides between the breaks that are not empty, as the deck leaves the others out. */
function slidesOf(lines: string[], breaks: Breaks): Slide[] {
	const bounds = [breaks.start - 1, ...breaks.rules, lines.length];
	const blank = (line: number) => /^\s*$/.test(lines[line]);
	const slides: Slide[] = [];
	for (let i = 0; i + 1 < bounds.length; i++) {
		const start = bounds[i] + 1;
		const end = bounds[i + 1] - 1;
		let first = start;
		let last = end;
		while (first <= last && blank(first)) first++;
		while (last >= first && blank(last)) last--;
		if (first <= last) {
			slides.push({ start, end, first, last, title: titleOf(lines.slice(first, last + 1)) });
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
