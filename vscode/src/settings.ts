// The `slides.editor` settings: which of the marks of the slides the text editor shows,
// in what colors, how far apart, and with what text. A color left empty is the theme's,
// by the id the extension contributes for it, which `workbench.colorCustomizations` sets
// too.

import * as vscode from 'vscode';

/** The colors of the marks, each the theme's color of the same id under `slides.` if
 * its setting is empty. */
const COLORS = [
	'slideBreak',
	'slideLabel',
	'alternateSlide',
	'title',
	'subtitle',
	'columnHeading',
	'column',
	'alternateColumn',
	'stepColumn',
	'step',
] as const;

export type ColorName = (typeof COLORS)[number];

/** The theme's color for each of `COLORS` that is not a contributed one. */
const THEME: Partial<Record<ColorName, string>> = {
	slideLabel: 'editorCodeLens.foreground',
};

/** How a heading of one level is marked. */
export interface HeadingStyle {
	color: string | vscode.ThemeColor;
	fontWeight: string;
	fontStyle: string;
}

/** The buttons above each slide, each shown or not. */
export interface Buttons {
	moveUp: boolean;
	moveDown: boolean;
	moveTo: boolean;
	newSlideAbove: boolean;
	newSlideBelow: boolean;
}

/** What the marks of the slides look like, the same in every document, as the editor
 * styles every line of a mark alike. */
export interface Style {
	colors: Record<ColorName, string | vscode.ThemeColor>;
	overviewRuler: boolean;
	slideLabelSpacing: number;
	slideLabelFontStyle: string;
	headings: [HeadingStyle, HeadingStyle, HeadingStyle];
}

/** Which marks a document shows, and with what text and spacing. */
export interface Shown {
	slideBreaks: boolean;
	slideLabels: boolean;
	alternateSlides: boolean;
	headings: boolean;
	columns: boolean;
	steps: boolean;
	stepCounts: boolean;
	buttons: Buttons;
	slideLabel: string;
	endLabel: string;
	importLabel: string;
	titleLength: number;
	columnIndent: number;
	columnBarWidth: number;
	stepPadding: number;
	gutterSpacing: number;
	stepSeparator: string;
	stepRangeSeparator: string;
	noSpaceMark: string;
	collapsedStepMark: string;
}

/** The style of the marks, as the user and the workspace set it. */
export function style(): Style {
	const settings = vscode.workspace.getConfiguration('slides.editor');
	const color = (name: ColorName) => {
		const value = settings.get<string>(`${name}Color`, '').trim();
		return value === '' ? new vscode.ThemeColor(THEME[name] ?? `slides.${name}`) : value;
	};
	const colors = Object.fromEntries(COLORS.map((name) => [name, color(name)])) as Style['colors'];
	const heading = (level: 'title' | 'subtitle' | 'columnHeading'): HeadingStyle => ({
		color: colors[level],
		fontWeight: settings.get(`${level}FontWeight`, 'bold'),
		fontStyle: settings.get(`${level}FontStyle`, 'normal'),
	});
	return {
		colors,
		overviewRuler: settings.get('overviewRuler', true),
		slideLabelSpacing: settings.get('slideLabelSpacing', 2),
		slideLabelFontStyle: settings.get('slideLabelFontStyle', 'italic'),
		headings: [heading('title'), heading('subtitle'), heading('columnHeading')],
	};
}

/** The marks the document shows, as the settings of its folder have them. */
export function shown(document: vscode.TextDocument): Shown {
	const settings = vscode.workspace.getConfiguration('slides.editor', document);
	const buttons = settings.get<Partial<Buttons>>('buttons', {});
	return {
		slideBreaks: settings.get('slideBreaks', true),
		slideLabels: settings.get('slideLabels', true),
		alternateSlides: settings.get('alternateSlides', true),
		headings: settings.get('headings', true),
		columns: settings.get('columns', true),
		steps: settings.get('steps', true),
		stepCounts: settings.get('stepCounts', true),
		buttons: {
			moveUp: buttons.moveUp ?? true,
			moveDown: buttons.moveDown ?? true,
			moveTo: buttons.moveTo ?? true,
			newSlideAbove: buttons.newSlideAbove ?? true,
			newSlideBelow: buttons.newSlideBelow ?? true,
		},
		slideLabel: settings.get('slideLabel', 'slide ${number}: ${title}'),
		endLabel: settings.get('endLabel', 'end'),
		importLabel: settings.get('importLabel', 'its slides, imported'),
		titleLength: Math.max(1, settings.get('titleLength', 50)),
		columnIndent: Math.max(0, settings.get('columnIndent', 2)),
		columnBarWidth: Math.max(0, settings.get('columnBarWidth', 3)),
		stepPadding: Math.max(0, settings.get('stepPadding', 1)),
		gutterSpacing: Math.max(0, settings.get('gutterSpacing', 1)),
		stepSeparator: settings.get('stepSeparator', ' · '),
		stepRangeSeparator: settings.get('stepRangeSeparator', '..'),
		noSpaceMark: settings.get('noSpaceMark', '*'),
		collapsedStepMark: settings.get('collapsedStepMark', '•'),
	};
}

/** `template` with each `${name}` written as `values` has it. */
export function fill(template: string, values: Record<string, string | number>): string {
	return template.replace(/\$\{(\w+)\}/g, (whole, name: string) => (name in values ? String(values[name]) : whole));
}
