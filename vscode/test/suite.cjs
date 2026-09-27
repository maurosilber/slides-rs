// Runs inside VS Code, with the extension loaded.
const assert = require('node:assert');
const fs = require('node:fs');
const path = require('node:path');
const vscode = require('vscode');

const NOTEBOOK = 'slides-notebook';
const repo = vscode.workspace.workspaceFolders[0].uri.fsPath;

async function open(file) {
	const uri = vscode.Uri.file(file);
	await vscode.commands.executeCommand('vscode.openWith', uri, NOTEBOOK);
	return until(() => vscode.workspace.notebookDocuments.find((notebook) => notebook.uri.fsPath === uri.fsPath));
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
		const file = path.join(repo, 'example', 'sections', 'slide2.md');
		const before = fs.readFileSync(file);
		const notebook = await open(file);
		const kinds = notebook.getCells().map((cell) => (cell.kind === vscode.NotebookCellKind.Code ? 'code' : 'markdown'));
		assert.deepStrictEqual(kinds, ['code', 'markdown', 'code', 'markdown', 'code', 'markdown', 'code']);
		assert.strictEqual(notebook.cellAt(0).document.languageId, 'python');
		assert.strictEqual(notebook.cellAt(1).document.getText(), '# Figures with steps');
		const code = notebook.getCells().filter((cell) => cell.kind === vscode.NotebookCellKind.Code);
		await until(() => code[1].outputs.length > 0);
		const mimes = code.map((cell) => cell.outputs.flatMap((output) => output.items.map((item) => item.mime)));
		assert.deepStrictEqual(mimes, [[], ['image/svg+xml'], ['image/svg+xml'], ['image/svg+xml']]);
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

		// Stands in for a kernel, which the extension has none of.
		const controller = vscode.notebooks.createNotebookController('slides-test', NOTEBOOK, 'Test');
		controller.executeHandler = async (cells) => {
			for (const cell of cells) {
				const execution = controller.createNotebookCellExecution(cell);
				execution.start(Date.now());
				await execution.replaceOutput([new vscode.NotebookCellOutput([vscode.NotebookCellOutputItem.stdout('1\n')])]);
				execution.end(true, Date.now());
			}
		};
		await vscode.window.showNotebookDocument(notebook);
		await vscode.commands.executeCommand('notebook.selectKernel', { id: 'slides-test', extension: 'maurosilber.slides-notebook' });
		await vscode.commands.executeCommand('notebook.cell.execute', { ranges: [{ start: 1, end: 2 }], document: notebook.uri });
		await until(() => notebook.cellAt(1).outputs.length > 0);
		await notebook.save();
		const saved = await until(() => fs.readdirSync(path.join(deck, '_outputs')).find((name) => /^[0-9a-f]{16}$/.test(name)));
		assert.strictEqual(fs.readFileSync(path.join(deck, '_outputs', saved, 'outputs.txt'), 'utf8').trim().split('.')[1], 'txt');
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

	async 'cells run as the deck runs them, and their outputs are saved on save'() {
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

		await notebook.save();
		const root = path.join(deck, '_outputs');
		const saved = await until(() => {
			const dirs = fs.readdirSync(root).filter((name) => /^[0-9a-f]{16}$/.test(name));
			return dirs.length === 3 && dirs;
		});
		const outputs = saved.flatMap((dir) =>
			fs.readFileSync(path.join(root, dir, 'outputs.txt'), 'utf8').trim().split('\n').map((name) => fs.readFileSync(path.join(root, name), 'utf8')),
		);
		assert.ok(outputs.includes('2'));
		assert.ok(outputs.some((output) => output.includes('ZeroDivisionError')));
		assert.strictEqual(fs.readFileSync(file, 'utf8'), markdown);
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
