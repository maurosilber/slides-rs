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
		const file = path.join(repo, 'slides', 'sections', 'slide2.md');
		const before = fs.readFileSync(file);
		const notebook = await open(file);
		const kinds = notebook.getCells().map((cell) => (cell.kind === vscode.NotebookCellKind.Code ? 'code' : 'markdown'));
		assert.deepStrictEqual(kinds, ['markdown', 'code', 'code', 'markdown', 'code']);
		assert.strictEqual(notebook.cellAt(0).document.getText(), '# Cuadratic');
		assert.strictEqual(notebook.cellAt(1).document.languageId, 'python');
		const code = notebook.getCells().filter((cell) => cell.kind === vscode.NotebookCellKind.Code);
		await until(() => code[1].outputs.length > 0);
		const mimes = code.map((cell) => cell.outputs.flatMap((output) => output.items.map((item) => item.mime)));
		assert.deepStrictEqual(mimes, [[], ['image/svg+xml'], ['image/svg+xml']]);
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
