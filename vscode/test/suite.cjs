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

/** The example's slide of figures, copied and rendered with the extension's slides-rs, so
 * that its outputs are saved as the deck saves them. */
function renderedSlide() {
	const deck = path.join(process.env.SLIDES_DECK, 'rendered');
	const file = path.join(deck, 'slide2.md');
	if (!fs.existsSync(file)) {
		fs.mkdirSync(deck, { recursive: true });
		fs.copyFileSync(path.join(repo, 'example', 'sections', 'slide2.md'), file);
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
		const listener = messaging.onDidReceiveMessage(({ message }) => rendered.push(message));
		const notebook = await open(renderedSlide());
		await vscode.window.showNotebookDocument(notebook);
		// The figure of lines drawn one step after another, and the two drawn at once.
		await until(() => rendered.length >= 3, 30000);
		const steps = rendered.map((message) => message.steps).sort((a, b) => a - b);
		assert.deepStrictEqual(steps.slice(0, 2), [1, 1]);
		assert.ok(steps[2] > 2, `the figure of steps has ${steps[2]}`);
		listener.dispose();
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
