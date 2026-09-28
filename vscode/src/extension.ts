// A slides-rs deck's markdown as a notebook: its markdown and code cells, and
// the outputs the deck saved for them in the outputs directory.

import * as vscode from 'vscode';
import { cellOutput, Saved, SAVED_AS, toBase64 } from './output';
import { registerFrontmatter } from './frontmatter';
import { canRunCells, Kernels } from './kernel';
import { Cell, CodeCell, Module } from './wasm';

const NOTEBOOK = 'slides-notebook';

export function activate(context: vscode.ExtensionContext) {
	const log = vscode.window.createOutputChannel('Slides Notebook', { log: true });
	const module = new Module(context.extensionUri, context.globalStorageUri, log);
	const outputs = new Outputs(module, log);
	context.subscriptions.push(
		log,
		registerFrontmatter(),
		vscode.workspace.registerNotebookSerializer(NOTEBOOK, new Serializer(module)),
		vscode.workspace.onDidOpenNotebookDocument((notebook) => outputs.load(notebook)),
		vscode.workspace.onDidSaveNotebookDocument((notebook) => outputs.save(notebook)),
		vscode.workspace.onDidCloseNotebookDocument((notebook) => outputs.forget(notebook)),
		vscode.commands.registerCommand('slides.openNotebook', (uri?: vscode.Uri) => {
			uri ??= vscode.window.activeTextEditor?.document.uri;
			if (uri) {
				return vscode.commands.executeCommand('vscode.openWith', uri, NOTEBOOK);
			}
		}),
	);
	if (canRunCells()) {
		const kernels = new Kernels(NOTEBOOK, context.extensionUri, log);
		context.subscriptions.push(
			kernels,
			vscode.commands.registerCommand('slides.restartKernel', () => {
				const notebook = vscode.window.activeNotebookEditor?.notebook;
				if (notebook?.notebookType === NOTEBOOK) {
					kernels.restart(notebook);
				}
			}),
		);
	}
	for (const notebook of vscode.workspace.notebookDocuments) {
		void outputs.load(notebook);
	}
}

/** Reads and writes the markdown, which holds no outputs: the deck saves those apart. */
class Serializer implements vscode.NotebookSerializer {
	constructor(private readonly module: Module) {}

	async deserializeNotebook(content: Uint8Array): Promise<vscode.NotebookData> {
		const cells = await this.module.cells(new TextDecoder().decode(content));
		return new vscode.NotebookData(cells.map(cellData));
	}

	async serializeNotebook(data: vscode.NotebookData): Promise<Uint8Array> {
		const cells = data.cells.map((cell): Cell => {
			const code = cell.kind === vscode.NotebookCellKind.Code;
			return { kind: code ? 'code' : 'markdown', source: cell.value, language: code ? cell.languageId : '', written: cell.metadata?.written ?? null };
		});
		return new TextEncoder().encode(await this.module.markdown(cells));
	}
}

function cellData(cell: Cell): vscode.NotebookCellData {
	const data =
		cell.kind === 'code'
			? new vscode.NotebookCellData(vscode.NotebookCellKind.Code, cell.source, cell.language)
			: new vscode.NotebookCellData(vscode.NotebookCellKind.Markup, cell.source, 'markdown');
	data.metadata = { written: cell.written };
	return data;
}

/**
 * The outputs of the notebooks' code cells, which the serializer cannot see: it is not
 * told which file it reads, and the outputs are saved under the address of each cell,
 * which depends on where the file is.
 */
class Outputs {
	/** The notebooks whose outputs were read, which are the only ones whose outputs are saved,
	 * so that saving before they are read does not take them for cleared. */
	private readonly loaded = new Set<string>();

	constructor(
		private readonly module: Module,
		private readonly log: vscode.LogOutputChannel,
	) {}

	/** Shows the saved outputs of a notebook just opened. */
	async load(notebook: vscode.NotebookDocument) {
		if (notebook.notebookType !== NOTEBOOK) {
			return;
		}
		try {
			const version = notebook.version;
			const cells = codeCells(notebook);
			const saved = await this.module.load(
				notebook.uri,
				cells.map((cell) => cell.document.getText()),
			);
			if (notebook.isClosed || notebook.version !== version) {
				this.log.info(`${notebook.uri}: changed while its outputs were read, so they are not shown`);
				return;
			}
			const edits = cells.flatMap((cell, index) =>
				saved[index].length === 0 ? [] : [vscode.NotebookEdit.replaceCells(new vscode.NotebookRange(cell.index, cell.index + 1), [withOutputs(cell, saved[index])])],
			);
			this.loaded.add(notebook.uri.toString());
			if (edits.length === 0) {
				return;
			}
			// Showing an output is an edit, so a notebook that was saved is saved again, which
			// writes it as it was.
			const wasDirty = notebook.isDirty;
			const edit = new vscode.WorkspaceEdit();
			edit.set(notebook.uri, edits);
			await vscode.workspace.applyEdit(edit);
			if (!wasDirty && !notebook.isUntitled) {
				await notebook.save();
			}
			this.log.info(`${notebook.uri}: read the outputs of ${edits.length} cell(s)`);
		} catch (error) {
			this.log.error(`${notebook.uri}: could not read the outputs: ${error}`);
		}
	}

	/** Saves the outputs that changed, under the address of each cell as it is now. */
	async save(notebook: vscode.NotebookDocument) {
		if (notebook.notebookType !== NOTEBOOK || !this.loaded.has(notebook.uri.toString())) {
			return;
		}
		try {
			const cells = codeCells(notebook).map(
				(cell): CodeCell => ({
					source: cell.document.getText(),
					outputs: cell.outputs.map((output) => ({
						name: output.metadata?.[SAVED_AS] ?? null,
						items: output.items.map((item) => ({ mime: item.mime, data: toBase64(item.data) })),
					})),
				}),
			);
			const saved = await this.module.save(notebook.uri, cells);
			if (saved > 0) {
				this.log.info(`${notebook.uri}: saved the outputs of ${saved} cell(s)`);
			}
		} catch (error) {
			this.log.error(`${notebook.uri}: could not save the outputs: ${error}`);
			void vscode.window.showErrorMessage(`Could not save the outputs of ${vscode.workspace.asRelativePath(notebook.uri)}: ${error}`);
		}
	}

	forget(notebook: vscode.NotebookDocument) {
		this.loaded.delete(notebook.uri.toString());
	}
}

function codeCells(notebook: vscode.NotebookDocument): vscode.NotebookCell[] {
	return notebook.getCells().filter((cell) => cell.kind === vscode.NotebookCellKind.Code);
}

function withOutputs(cell: vscode.NotebookCell, saved: Saved[]): vscode.NotebookCellData {
	const data = new vscode.NotebookCellData(cell.kind, cell.document.getText(), cell.document.languageId);
	data.metadata = cell.metadata;
	data.executionSummary = cell.executionSummary;
	data.outputs = saved.map(cellOutput);
	return data;
}
