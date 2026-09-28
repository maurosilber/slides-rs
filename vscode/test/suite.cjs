// Runs inside VS Code, with the extension loaded.
const assert = require('node:assert');
const { spawnSync } = require('node:child_process');
const fs = require('node:fs');
const path = require('node:path');
const vscode = require('vscode');

const NOTEBOOK = 'slides-notebook';
const FIGURE = 'application/vnd.slides-rs.svg+xml';
const repo = vscode.workspace.workspaceFolders[0].uri.fsPath;

async function open(file) {
	const uri = vscode.Uri.file(file);
	await vscode.commands.executeCommand('vscode.openWith', uri, NOTEBOOK);
	return until(() => vscode.workspace.notebookDocuments.find((notebook) => notebook.uri.fsPath === uri.fsPath));
}

/** One of the example's slides of figures, copied and rendered with the extension's
 * slides-rs, so that its outputs are saved as the deck saves them. */
function renderedSlide(name = 'slide2.md') {
	const deck = path.join(process.env.SLIDES_DECK, 'rendered');
	const file = path.join(deck, name);
	if (!fs.existsSync(file)) {
		fs.mkdirSync(deck, { recursive: true });
		fs.copyFileSync(path.join(repo, 'example', 'sections', name), file);
		const slidesRs = path.join(repo, 'vscode', 'dist', 'slides-rs');
		const render = spawnSync(slidesRs, [file], { encoding: 'utf8' });
		assert.strictEqual(render.status, 0, render.stderr);
	}
	return file;
}

async function until(value, timeout = 20000) {
	const start = Date.now();
	for (;;) {
		const result = value();
		if (result) return result;
		if (Date.now() - start > timeout) throw new Error(`timed out waiting for ${value}`);
		await new Promise((resolve) => setTimeout(resolve, 100));
	}
}

const tests = {
	async 'the frontmatter completes its keys and values, and describes them'() {
		const content = '---\ntheme: dark\n\nfigures:\n  fade: \n---\n\n# Slide\n';
		const document = await vscode.workspace.openTextDocument({ language: 'markdown', content });
		// Opening a markdown file activates the extension, which may not have finished.
		await vscode.extensions.getExtension('maurosilber.slides-notebook').activate();
		const labels = async (line, character) => {
			const list = await vscode.commands.executeCommand('vscode.executeCompletionItemProvider', document.uri, new vscode.Position(line, character));
			// Its keys and values, rather than the words of the document VS Code suggests too.
			const ours = [vscode.CompletionItemKind.Property, vscode.CompletionItemKind.Value];
			return list.items.filter((item) => ours.includes(item.kind)).map((item) => item.label).sort();
		};
		// Of the keys at the top, those not set yet, and of those under figures, those
		// not set but on the line being written.
		assert.deepStrictEqual(await labels(2, 0), ['aspect-ratio', 'steps']);
		assert.deepStrictEqual(await labels(4, 2), ['fade', 'invert', 'rush']);
		assert.deepStrictEqual(await labels(4, 8), ['false', 'true']);
		// Past the frontmatter, it has nothing to say.
		assert.deepStrictEqual(await labels(7, 0), []);
		const [hover] = await vscode.commands.executeCommand('vscode.executeHoverProvider', document.uri, new vscode.Position(4, 3));
		assert.match(hover.contents[0].value, /^\*\*fade\*\* \(boolean\)/);
	},

	async 'the code cells complete as a Python file of their own would'() {
		const dir = path.join(process.env.SLIDES_DECK, 'python');
		fs.mkdirSync(dir, { recursive: true });
		const file = path.join(dir, 'deck.md');
		const content = [
			'---', 'theme: dark', '---', '', '# Slide', '',
			'```python', 'not_a_cell = 1', '```', '',
			'~~~python', 'import numpy as np', 'np.', '~~~', '',
			'~~~', 'x = 1', '~~~', '',
			'~~~rust', 'let y = 2;', '~~~', '',
		].join('\n');
		fs.writeFileSync(file, content);
		// Stands in for a Python language server, answering with what it was asked of.
		let asked;
		const server = vscode.languages.registerCompletionItemProvider({ language: 'python', scheme: 'file' }, {
			provideCompletionItems(document, position) {
				asked = { name: path.basename(document.fileName), text: document.getText() };
				const item = new vscode.CompletionItem(document.lineAt(position.line).text, vscode.CompletionItemKind.Event);
				item.additionalTextEdits = [vscode.TextEdit.insert(new vscode.Position(0, 0), 'import os\n'), vscode.TextEdit.insert(new vscode.Position(4, 0), 'no')];
				return [item];
			},
		}, '.');
		try {
			const document = await vscode.workspace.openTextDocument(file);
			await vscode.extensions.getExtension('maurosilber.slides-notebook').activate();
			const items = async (line, character) => {
				const list = await vscode.commands.executeCommand('vscode.executeCompletionItemProvider', document.uri, new vscode.Position(line, character), '.');
				return list.items.filter((item) => item.kind === vscode.CompletionItemKind.Event);
			};
			const [item] = await items(12, 3);
			assert.strictEqual(item.label, 'np.');
			// The Python cells, each line in its place, and nothing else.
			const lines = content.split('\n').map((text, i) => ([11, 12, 16].includes(i) ? text : ''));
			assert.strictEqual(asked.text, lines.join('\n'));
			assert.match(asked.name, /^\.deck\.md\.\w+\.py$/);
			// The import goes at the top of the first cell, and the edit in the markdown not at all.
			assert.deepStrictEqual(item.additionalTextEdits.map((edit) => [edit.range.start.line, edit.newText]), [[11, 'import os\n']]);
			// A cell whose fence names no language is in Python.
			assert.deepStrictEqual((await items(16, 1)).map((item) => item.label), ['x = 1']);
			// Outside the Python cells, it has nothing to say.
			for (const [line, character] of [[4, 2], [7, 3], [21, 3]]) assert.deepStrictEqual(await items(line, character), []);
			assert.deepStrictEqual(fs.readdirSync(dir), ['deck.md']);
		} finally {
			server.dispose();
		}
	},

	async 'a deck opens as its cells, with the outputs the deck saved'() {
		const file = renderedSlide();
		const before = fs.readFileSync(file);
		const notebook = await open(file);
		const kinds = notebook.getCells().map((cell) => (cell.kind === vscode.NotebookCellKind.Code ? 'code' : 'markdown'));
		assert.deepStrictEqual(kinds, ['code', 'markdown', 'code', 'markdown', 'code', 'markdown', 'code']);
		assert.strictEqual(notebook.cellAt(0).document.languageId, 'python');
		assert.strictEqual(notebook.cellAt(1).document.getText(), '# Figures with steps');
		const code = notebook.getCells().filter((cell) => cell.kind === vscode.NotebookCellKind.Code);
		await until(() => code[1].outputs.length > 0);
		const mimes = code.map((cell) => cell.outputs.flatMap((output) => output.items.map((item) => item.mime)));
		assert.deepStrictEqual(mimes, [[], [FIGURE], [FIGURE], [FIGURE]]);
		assert.ok(new TextDecoder().decode(code[1].outputs[0].items[0].data).startsWith('\n\n<svg'));
		await until(() => !notebook.isDirty);
		assert.ok(fs.readFileSync(file).equals(before), 'the file is written back as it was');
		await vscode.commands.executeCommand('workbench.action.closeAllEditors');
	},

	async 'a deck opens with outputs larger than the module answers at once'() {
		const deck = path.join(process.env.SLIDES_DECK, 'large');
		fs.mkdirSync(deck, { recursive: true });
		const file = path.join(deck, 'slide.md');
		fs.writeFileSync(file, '~~~python\nprint("x" * 3_000_000)\n~~~\n');
		const render = spawnSync(path.join(repo, 'vscode', 'dist', 'slides-rs'), [file], { encoding: 'utf8' });
		assert.strictEqual(render.status, 0, render.stderr);
		const notebook = await open(file);
		await until(() => notebook.cellAt(0).outputs.length > 0);
		assert.strictEqual(notebook.cellAt(0).outputs[0].items[0].data.length, 3_000_001);
		await vscode.commands.executeCommand('workbench.action.closeAllEditors');
	},

	async 'outputs are read while the extension host is busy, as when a window opens'() {
		const deck = path.join(process.env.SLIDES_DECK, 'busy');
		fs.mkdirSync(deck, { recursive: true });
		const files = [0, 1, 2, 3, 4, 5].map((n) => path.join(deck, `slide${n}.md`));
		for (const [n, file] of files.entries()) {
			fs.writeFileSync(file, `~~~python\nprint("${n}" * 40_000)\n~~~\n`);
		}
		for (const file of files) {
			const render = spawnSync(path.join(repo, 'vscode', 'dist', 'slides-rs'), [file], { encoding: 'utf8' });
			assert.strictEqual(render.status, 0, render.stderr);
		}
		// Other extensions starting up keep the host from handling the module's messages as
		// they come, which is when it dropped the end of an answer.
		let busy = true;
		const block = () => {
			const end = Date.now() + 30;
			while (Date.now() < end) {}
			if (busy) setTimeout(block, 1);
		};
		block();
		try {
			const notebooks = await Promise.all(files.map(open));
			for (const notebook of notebooks) {
				await until(() => notebook.cellAt(0).outputs.length > 0, 30000);
				assert.strictEqual(notebook.cellAt(0).outputs[0].items[0].data.length, 40_001);
			}
		} finally {
			busy = false;
		}
		await vscode.commands.executeCommand('workbench.action.closeAllEditors');
	},

	async 'outputs are saved where the deck reads them, and read back'() {
		const deck = process.env.SLIDES_DECK;
		fs.mkdirSync(path.join(deck, 'sections'), { recursive: true });
		fs.mkdirSync(path.join(deck, '_outputs'));
		const file = path.join(deck, 'sections', 'slide.md');
		const markdown = '# Title\n\n~~~python\nprint(1)\n~~~\n';
		fs.writeFileSync(file, markdown);
		let notebook = await open(file);

		// Stands in for another extension's kernel, whose outputs are saved on save.
		const figure = `<svg xmlns="http://www.w3.org/2000/svg">${'<path d="M0 0L1 1"/>'.repeat(1000)}</svg>`;
		const controller = vscode.notebooks.createNotebookController('slides-test', NOTEBOOK, 'Test');
		controller.executeHandler = async (cells) => {
			for (const cell of cells) {
				const execution = controller.createNotebookCellExecution(cell);
				execution.start(Date.now());
				await execution.replaceOutput([
					new vscode.NotebookCellOutput([vscode.NotebookCellOutputItem.stdout('1\n')]),
					new vscode.NotebookCellOutput([new vscode.NotebookCellOutputItem(new TextEncoder().encode(figure), 'image/svg+xml')]),
				]);
				execution.end(true, Date.now());
			}
		};
		await vscode.window.showNotebookDocument(notebook);
		await vscode.commands.executeCommand('notebook.selectKernel', { id: 'slides-test', extension: 'maurosilber.slides-notebook' });
		await vscode.commands.executeCommand('notebook.cell.execute', { ranges: [{ start: 1, end: 2 }], document: notebook.uri });
		await until(() => notebook.cellAt(1).outputs.length > 0);
		await notebook.save();
		const saved = await until(() => fs.readdirSync(path.join(deck, '_outputs')).find((name) => /^[0-9a-f]{16}$/.test(name)));
		const names = fs.readFileSync(path.join(deck, '_outputs', saved, 'outputs.txt'), 'utf8').trim().split('\n');
		assert.deepStrictEqual(names.map((name) => name.split('.')[1]), ['txt', 'svg']);
		assert.strictEqual(fs.readFileSync(path.join(deck, '_outputs', names[1]), 'utf8'), figure);
		// Each written aside and then moved into place, with nothing left aside.
		assert.deepStrictEqual(fs.readdirSync(path.join(deck, '_outputs')).filter((name) => name.startsWith('.')), ['.gitignore']);
		assert.ok(!fs.existsSync(path.join(deck, 'sections', '_outputs')));
		assert.strictEqual(fs.readFileSync(file, 'utf8'), markdown);
		await vscode.commands.executeCommand('workbench.action.closeAllEditors');
		controller.dispose();
		await until(() => notebook.isClosed);

		notebook = await open(file);
		const cell = notebook.cellAt(1);
		await until(() => cell.outputs.length > 0 || notebook.cellAt(1).outputs.length > 0);
		const output = notebook.cellAt(1).outputs[0];
		assert.strictEqual(output.items[0].mime, 'text/plain');
		assert.strictEqual(new TextDecoder().decode(output.items[0].data), '1\n');
		assert.ok(output.metadata['slides.savedAs'].endsWith('.txt'));
		await vscode.commands.executeCommand('workbench.action.closeAllEditors');
	},

	async 'cells run as the deck runs them, and their outputs are saved as they run'() {
		const deck = path.join(process.env.SLIDES_DECK, 'run');
		fs.mkdirSync(path.join(deck, '_outputs'), { recursive: true });
		const file = path.join(deck, 'slide.md');
		const cells = ['import os\nx = 1\nprint(os.getcwd())', 'x + 1', '1 / 0', 'x'];
		const markdown = '# Title\n' + cells.map((code) => `\n~~~python\n${code}\n~~~\n`).join('');
		fs.writeFileSync(file, markdown);
		const notebook = await open(file);
		await vscode.window.showNotebookDocument(notebook);
		await vscode.commands.executeCommand('notebook.selectKernel', { id: 'slides-rs', extension: 'maurosilber.slides-notebook' });
		await vscode.commands.executeCommand('notebook.execute');
		const code = notebook.getCells().filter((cell) => cell.kind === vscode.NotebookCellKind.Code);
		await until(() => code[2].executionSummary?.success === false, 60000);
		const text = (cell) => cell.outputs.flatMap((output) => output.items.map((item) => new TextDecoder().decode(item.data)));

		// Next to the file, with each cell seeing what the ones before it defined.
		assert.deepStrictEqual(text(code[0]), [fs.realpathSync(deck) + '\n']);
		assert.deepStrictEqual(text(code[1]), ['2']);
		assert.deepStrictEqual(code.slice(0, 3).map((cell) => cell.executionSummary.executionOrder), [1, 2, 3]);
		assert.match(text(code[2])[0], /ZeroDivisionError/);
		// Running all stops at the cell that raised.
		assert.deepStrictEqual(code[3].outputs, []);

		// Saved without saving the notebook, with the files each cell read, as they ran in order.
		const root = path.join(deck, '_outputs');
		const dirs = () => fs.readdirSync(root).filter((name) => /^[0-9a-f]{16}$/.test(name));
		const saved = dirs();
		assert.strictEqual(saved.length, 3);
		assert.ok(saved.every((dir) => fs.existsSync(path.join(root, dir, 'inputs.txt'))));
		const outputs = saved.flatMap((dir) =>
			fs.readFileSync(path.join(root, dir, 'outputs.txt'), 'utf8').trim().split('\n').map((name) => fs.readFileSync(path.join(root, name), 'utf8')),
		);
		assert.ok(outputs.includes('2'));
		assert.ok(outputs.some((output) => output.includes('ZeroDivisionError')));

		// Run again after the others, a cell's outputs are saved, but not trusted.
		const before = new Set(saved);
		await vscode.commands.executeCommand('notebook.cell.execute', { ranges: [{ start: code[1].index, end: code[1].index + 1 }], document: notebook.uri });
		await until(() => code[1].executionSummary?.executionOrder === 4);
		const again = saved.find((dir) => !fs.existsSync(path.join(root, dir, 'inputs.txt')));
		assert.ok(again && before.has(again), 'the cell is saved under its address, without the files it read');
		assert.strictEqual(dirs().length, 3);

		await notebook.save();
		assert.strictEqual(fs.readFileSync(file, 'utf8'), markdown);
		await vscode.commands.executeCommand('workbench.action.closeAllEditors');
	},

	async 'a figure steps as on its slide'() {
		const messaging = vscode.notebooks.createRendererMessaging('slides-figure');
		const rendered = [];
		const listener = messaging.onDidReceiveMessage(({ message }) => rendered.push(message.steps));
		const notebook = await open(renderedSlide());
		await vscode.window.showNotebookDocument(notebook);
		// The figure of lines drawn one step after another, and one drawn at once. The
		// notebook renders only the outputs in view, which the last may not be.
		await until(() => rendered.includes(1) && rendered.some((steps) => steps > 2), 30000);
		listener.dispose();
		await vscode.commands.executeCommand('workbench.action.closeAllEditors');
	},

	async 'the slides are shown as rendered, and rendered again on every save'() {
		const deck = path.join(process.env.SLIDES_DECK, 'show');
		// Its own, rather than the one another test leaves above it.
		fs.mkdirSync(path.join(deck, '_outputs'), { recursive: true });
		const file = path.join(deck, 'talk.md');
		fs.writeFileSync(file, '# First\n');
		await vscode.commands.executeCommand('slides.showSlides', vscode.Uri.file(file));
		const page = path.join(deck, '_outputs', 'talk.html');
		assert.match(fs.readFileSync(page, 'utf8'), />First</);

		fs.writeFileSync(file, '# Second\n');
		await until(() => fs.readFileSync(page, 'utf8').includes('>Second<'));
		await vscode.commands.executeCommand('slides.stopSlides');
		await vscode.commands.executeCommand('workbench.action.closeAllEditors');
	},

	async 'an interrupted cell stops, and the next one runs'() {
		const deck = path.join(process.env.SLIDES_DECK, 'interrupt');
		fs.mkdirSync(deck, { recursive: true });
		const file = path.join(deck, 'slide.md');
		fs.writeFileSync(file, '~~~python\nimport time\ntime.sleep(60)\n~~~\n\n~~~python\n1 + 1\n~~~\n');
		const notebook = await open(file);
		await vscode.window.showNotebookDocument(notebook);
		await vscode.commands.executeCommand('notebook.selectKernel', { id: 'slides-rs', extension: 'maurosilber.slides-notebook' });
		const run = (index) => vscode.commands.executeCommand('notebook.cell.execute', { ranges: [{ start: index, end: index + 1 }], document: notebook.uri });
		void run(0);
		// Long enough for the kernel to start and the cell to be sleeping.
		await new Promise((resolve) => setTimeout(resolve, 5000));
		const start = Date.now();
		await vscode.commands.executeCommand('notebook.cancelExecution');
		await until(() => notebook.cellAt(0).executionSummary?.success === false);
		assert.ok(Date.now() - start < 10000, 'the cell stops before it would have ended');
		await run(1);
		await until(() => notebook.cellAt(1).outputs.length > 0, 60000);
		assert.strictEqual(new TextDecoder().decode(notebook.cellAt(1).outputs[0].items[0].data), '2');
		await vscode.commands.executeCommand('workbench.action.closeAllEditors');
	},
};

exports.run = async () => {
	const failures = [];
	for (const [name, test] of Object.entries(tests)) {
		try {
			await test();
			console.log(`  ok  ${name}`);
		} catch (error) {
			console.log(`  FAIL  ${name}\n${error.stack}`);
			failures.push(name);
		}
	}
	if (failures.length) throw new Error(`${failures.length} test(s) failed`);
};
