// The `slides.editor` settings: which of the marks of the slides the text editor shows,
// in what colors and fonts, how far apart, and with what text. Each is an object, of
// which a key left out is its default. A color is a CSS color, or the id of a theme's
// color, as `slides.title`, which `workbench.colorCustomizations` sets too.

import * as vscode from 'vscode';

/** A color, as the editor takes it, or none. */
export type Color = string | vscode.ThemeColor | undefined;

/** The colors of the marks, of which the slides' and the columns' are each a list, taken
 * by turns. */
export interface Colors {
	slideBreak: Color;
	slideLabel: Color;
	slides: Color[];
	title: Color;
	subtitle: Color;
	columnHeading: Color;
	columns: Color[];
	stepColumn: Color;
	step: Color;
}

const COLORS = {
	slideBreak: 'slides.slideBreak',
	slideLabel: 'editorCodeLens.foreground',
	slides: ['', 'slides.alternateSlide'],
	title: 'slides.title',
	subtitle: 'slides.subtitle',
	columnHeading: 'slides.columnHeading',
	columns: ['slides.column', 'slides.alternateColumn'],
	stepColumn: 'slides.stepColumn',
	step: 'slides.step',
};

/** The font of a mark: its weight and its style. */
export interface Font {
	fontWeight: string;
	fontStyle: string;
}

const FONTS = {
	slideLabel: 'italic',
	title: 'bold',
	subtitle: 'bold',
	columnHeading: 'bold',
};

/** How a heading of one level is marked. */
export interface HeadingStyle extends Font {
	color: Color;
}

/** The buttons above each slide, each shown or not. */
export interface Buttons {
	moveUp: boolean;
	moveDown: boolean;
	moveTo: boolean;
	newSlideAbove: boolean;
	newSlideBelow: boolean;
}

const BUTTONS: Buttons = {
	moveUp: true,
	moveDown: true,
	moveTo: true,
	newSlideAbove: true,
	newSlideBelow: true,
};

/** Which marks are shown. */
export interface Show {
	slideBreaks: boolean;
	slideLabels: boolean;
	overviewRuler: boolean;
	alternateSlides: boolean;
	headings: boolean;
	columns: boolean;
	steps: boolean;
	stepCounts: boolean;
}

const SHOW: Show = {
	slideBreaks: true,
	slideLabels: true,
	overviewRuler: true,
	alternateSlides: true,
	headings: true,
	columns: true,
	steps: true,
	stepCounts: true,
};

/** What is written after a break. */
export interface Labels {
	slide: string;
	end: string;
	import: string;
	titleLength: number;
}

const LABELS: Labels = {
	slide: 'slide ${number}: ${title}',
	end: 'end',
	import: 'its slides, imported',
	titleLength: 50,
};

/** What is written in the steps' column. */
export interface StepMarks {
	separator: string;
	range: string;
	more: string;
	noSpace: string;
	collapsed: string;
}

const STEP_MARKS: StepMarks = {
	separator: ' · ',
	range: '..',
	more: ' +${more}',
	noSpace: '*',
	collapsed: '•',
};

/** The spaces of the marks, in characters but for `slideLabel`, in `em`, and
 * `columnBarWidth`, in pixels. */
export interface Spacing {
	slideLabel: number;
	stepPadding: number;
	columnIndent: number;
	columnBarWidth: number;
	beforeText: number;
}

const SPACING: Spacing = {
	slideLabel: 2,
	stepPadding: 1,
	columnIndent: 2,
	columnBarWidth: 3,
	beforeText: 1,
};

/** What the marks of the slides look like, the same in every document, as the editor
 * styles every line of a mark alike. */
export interface Style {
	colors: Colors;
	overviewRuler: boolean;
	slideLabelSpacing: number;
	slideLabelFont: Font;
	headings: [HeadingStyle, HeadingStyle, HeadingStyle];
}

/** Which marks a document shows, and with what text and spacing. */
export interface Shown extends Show {
	buttons: Buttons;
	labels: Labels;
	stepMarks: StepMarks;
	spacing: Spacing;
}

/** The setting `name`, an object, over its `defaults`, key by key. */
function group<T extends object>(settings: vscode.WorkspaceConfiguration, name: string, defaults: T): T {
	const value = settings.get<Partial<T>>(name);
	return { ...defaults, ...(typeof value === 'object' && value !== null ? value : {}) };
}

/** A color of the settings: none if empty, the theme's if it is the id of one, as
 * `slides.title`, which no CSS color is, or else as it is written. */
function color(value: unknown): Color {
	if (typeof value !== 'string' || value.trim() === '') {
		return undefined;
	}
	const text = value.trim();
	return /^[a-z][\w-]*(\.[\w-]+)+$/i.test(text) ? new vscode.ThemeColor(text) : text;
}

/** The colors of a list, or a single one, taken by turns. */
function colorList(value: unknown): Color[] {
	return (Array.isArray(value) ? value : [value]).map(color);
}

/** The font of words such as `bold italic` or `600`, `normal` where none says. */
function font(value: unknown): Font {
	const result = { fontWeight: 'normal', fontStyle: 'normal' };
	for (const word of String(value ?? '').split(/\s+/).filter(Boolean)) {
		if (word === 'italic' || word === 'oblique') {
			result.fontStyle = word;
		} else if (word !== 'normal') {
			result.fontWeight = word;
		}
	}
	return result;
}

/** The style of the marks, as the user and the workspace set it. */
export function style(): Style {
	const settings = vscode.workspace.getConfiguration('slides.editor');
	const values = group(settings, 'colors', COLORS);
	const colors: Colors = {
		slideBreak: color(values.slideBreak),
		slideLabel: color(values.slideLabel),
		slides: colorList(values.slides),
		title: color(values.title),
		subtitle: color(values.subtitle),
		columnHeading: color(values.columnHeading),
		columns: colorList(values.columns),
		stepColumn: color(values.stepColumn),
		step: color(values.step),
	};
	const fonts = group(settings, 'fonts', FONTS);
	const heading = (level: 'title' | 'subtitle' | 'columnHeading'): HeadingStyle => ({
		color: colors[level],
		...font(fonts[level]),
	});
	return {
		colors,
		overviewRuler: group(settings, 'show', SHOW).overviewRuler,
		slideLabelSpacing: Math.max(0, group(settings, 'spacing', SPACING).slideLabel),
		slideLabelFont: font(fonts.slideLabel),
		headings: [heading('title'), heading('subtitle'), heading('columnHeading')],
	};
}

/** The marks the document shows, as the settings of its folder have them. */
export function shown(document: vscode.TextDocument): Shown {
	const settings = vscode.workspace.getConfiguration('slides.editor', document);
	const labels = group(settings, 'labels', LABELS);
	const spacing = group(settings, 'spacing', SPACING);
	return {
		...group(settings, 'show', SHOW),
		buttons: group(settings, 'buttons', BUTTONS),
		labels: { ...labels, titleLength: Math.max(1, labels.titleLength) },
		stepMarks: group(settings, 'stepMarks', STEP_MARKS),
		spacing: {
			slideLabel: Math.max(0, spacing.slideLabel),
			stepPadding: Math.max(0, spacing.stepPadding),
			columnIndent: Math.max(0, spacing.columnIndent),
			columnBarWidth: Math.max(0, spacing.columnBarWidth),
			beforeText: Math.max(0, spacing.beforeText),
		},
	};
}

/** Whether the page of the slides goes, as the cursor moves, to the slide and step it is at. */
export function previewFollowsCursor(document: vscode.TextDocument): boolean {
	return vscode.workspace.getConfiguration('slides.editor', document).get('previewFollowsCursor', true);
}

/** `template` with each `${name}` written as `values` has it. */
export function fill(template: string, values: Record<string, string | number>): string {
	return template.replace(/\$\{(\w+)\}/g, (whole, name: string) => (name in values ? String(values[name]) : whole));
}
