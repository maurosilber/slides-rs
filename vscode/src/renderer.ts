// Shows a figure in the notebook as a slide shows it, stepping through the
// parts matplotlib's `gid` marks as steps: the deck's own steps.js numbers
// them, as it does on the page.

import type { ActivationFunction } from 'vscode-notebook-renderer';
import { stepsOf } from '../../src/static/steps.js';

/** As slides.css fades the steps in and out, within the figure alone. */
const STYLE = `
.slides-figure .step {
	transition: opacity 0.25s ease-out, visibility 0.25s ease-out;
}
.slides-figure .step-hidden {
	opacity: 0;
	visibility: hidden;
}
.slides-figure .step-collapse.step-hidden {
	display: none;
}
.slides-figure.steps-instant .step {
	transition: none;
}
.slides-figure svg {
	max-width: 100%;
	height: auto;
}
.slides-figure:focus {
	outline: none;
}
/* Drawn for a light page, as the deck's dark theme inverts them. */
.vscode-dark .slides-figure svg,
.vscode-high-contrast:not(.vscode-high-contrast-light) .slides-figure svg {
	filter: invert(1) hue-rotate(180deg);
}
.slides-steps {
	display: flex;
	gap: 0.5em;
	align-items: center;
	font-family: var(--vscode-font-family);
	font-size: var(--vscode-font-size);
	color: var(--vscode-foreground);
	user-select: none;
}
.slides-steps button {
	background: var(--vscode-button-secondaryBackground);
	color: var(--vscode-button-secondaryForeground);
	border: none;
	border-radius: 2px;
	padding: 0 0.6em;
	cursor: pointer;
}
.slides-steps button:disabled {
	opacity: 0.5;
	cursor: default;
}
`;

interface Steps {
	count: number;
	elements: { element: Element; from: number; to: number; collapse: boolean }[];
}

export const activate: ActivationFunction = (context) => {
	const style = document.createElement('style');
	style.textContent = STYLE;
	document.head.append(style);
	return {
		renderOutputItem(item, element) {
			const figure = document.createElement('div');
			figure.className = 'slides-figure steps-instant';
			figure.innerHTML = item.text();
			// stepsOf reads a slide, which here holds the figure alone.
			const slide = document.createElement('div');
			slide.append(figure);
			const { count, elements } = stepsOf(slide) as Steps;
			for (const { element, collapse } of elements) {
				element.classList.add('step');
				element.classList.toggle('step-collapse', collapse);
			}
			element.replaceChildren(slide);

			let current = 1;
			const controls = document.createElement('div');
			controls.className = 'slides-steps';
			const previous = button('‹', 'Previous step', () => show(current - 1));
			const next = button('›', 'Next step', () => show(current + 1));
			const label = document.createElement('span');
			controls.append(previous, label, next);
			const show = (step: number) => {
				if (step < 1 || step > count) {
					return;
				}
				current = step;
				for (const { element, from, to } of elements) {
					element.classList.toggle('step-hidden', !(from <= step && step < to));
				}
				label.textContent = `Step ${step} of ${count}`;
				previous.disabled = step === 1;
				next.disabled = step === count;
			};
			// A figure opens at its first step, as on its slide.
			show(1);
			figure.getBoundingClientRect();
			figure.classList.remove('steps-instant');
			if (count > 1) {
				slide.append(controls);
				figure.tabIndex = 0;
				figure.addEventListener('click', () => show(current < count ? current + 1 : 1));
				figure.addEventListener('keydown', (event) => {
					const step = { ArrowRight: current + 1, ArrowLeft: current - 1, ArrowDown: count, ArrowUp: 1 }[event.key];
					if (step !== undefined) {
						event.preventDefault();
						show(step);
					}
				});
			}
			// Which the tests listen for.
			context.postMessage?.({ rendered: item.id, steps: count });
		},
	};
};

function button(text: string, title: string, onClick: () => void): HTMLButtonElement {
	const button = document.createElement('button');
	button.textContent = text;
	button.title = title;
	button.addEventListener('click', onClick);
	return button;
}
