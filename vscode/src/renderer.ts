// Shows a figure in the notebook as a slide shows it, stepping through the
// parts matplotlib's `gid` marks as steps: the deck's own steps.js numbers
// them, as it does on the page.

import type { ActivationFunction } from 'vscode-notebook-renderer';
import { playAnimations, stepsOf } from '../../src/static/steps.js';

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
	margin-bottom: 0.5em;
}
.slides-steps label {
	display: flex;
	gap: 0.3em;
	align-items: center;
	margin-left: 0.5em;
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

/** What the renderer remembers, for every figure. */
interface State {
	/** Whether the figures step, or show whole. */
	animate: boolean;
}

interface Steps {
	count: number;
	elements: { element: Element; from: number; to: number; collapse: boolean }[];
}

export const activate: ActivationFunction<State> = (context) => {
	let animate = context.getState()?.animate ?? true;
	/** How each figure shown updates, when the checkbox of any of them changes. */
	const figures = new Set<{ element: HTMLElement; update: () => void }>();
	const setAnimate = (value: boolean) => {
		animate = value;
		context.setState({ animate });
		for (const figure of figures) {
			if (figure.element.isConnected) {
				figure.update();
			} else {
				figures.delete(figure);
			}
		}
	};
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
			const toggle = document.createElement('label');
			const checkbox = document.createElement('input');
			checkbox.type = 'checkbox';
			checkbox.addEventListener('change', () => setAnimate(checkbox.checked));
			toggle.append(checkbox, 'Animate');
			toggle.title = 'Step through the figures, or show them whole';
			controls.append(previous, label, next, toggle);
			// Not animated, every step shows, even those that are over, as when the deck's
			// steps are off; the step is kept, for animating to resume there.
			const show = (step: number) => {
				if (step < 1 || step > count) {
					return;
				}
				current = step;
				for (const { element, from, to } of elements) {
					element.classList.toggle('step-hidden', animate && !(from <= step && step < to));
				}
				playAnimations(figure);
				checkbox.checked = animate;
				label.textContent = `Step ${step} of ${count}`;
				previous.disabled = !animate || step === 1;
				next.disabled = !animate || step === count;
			};
			const move = (step: number) => animate && show(step);
			// A figure opens at its first step, as on its slide.
			show(1);
			figure.getBoundingClientRect();
			figure.classList.remove('steps-instant');
			if (count > 1) {
				slide.prepend(controls);
				figures.add({ element: slide, update: () => show(current) });
				figure.tabIndex = 0;
				figure.addEventListener('click', () => move(current < count ? current + 1 : 1));
				figure.addEventListener('keydown', (event) => {
					const step = { ArrowRight: current + 1, ArrowLeft: current - 1, ArrowDown: count, ArrowUp: 1 }[event.key];
					if (step !== undefined) {
						event.preventDefault();
						move(step);
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
