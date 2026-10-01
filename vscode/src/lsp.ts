// The Python of a deck's code cells, in the markdown text editor, as a Python language
// server has it: one the extension starts in the environment the cells run in, which
// `slides-rs environment` activates as for the kernel, so that the server resolves the
// imports as the cells do when they run. The server is told of a Python file of the
// cells, in order, each line where it is in the markdown and the rest blank, which is
// never written to disk, so that its answers hold for the markdown as they are. Without
// a server, python.ts completes the cells with the one VS Code has instead.

import * as vscode from 'vscode';
import * as fs from 'node:fs';
import * as path from 'node:path';
import { execFile } from 'node:child_process';
import {
	CompletionRequest,
	CompletionResolveRequest,
	DefinitionRequest,
	DiagnosticRefreshRequest,
	DidChangeTextDocumentNotification,
	DidChangeWorkspaceFoldersNotification,
	DidCloseTextDocumentNotification,
	DidOpenTextDocumentNotification,
	DocumentDiagnosticRequest,
	HoverRequest,
	LanguageClient,
	ReferencesRequest,
	SignatureHelpRequest,
	State,
} from 'vscode-languageclient/node';
import { Decks, SELECTOR } from './decks';
import { command } from './kernel';
import { keepInCells, pythonCells } from './python';

/** The servers looked for, in order, by the command that starts one. */
const SERVERS: { command: string; args: string[] }[] = [
	{ command: 'basedpyright-langserver', args: ['--stdio'] },
	{ command: 'ty', args: ['server'] },
	{ command: 'pyright-langserver', args: ['--stdio'] },
];

/** The environment the cells of a file run in, as `slides-rs environment` prints it. */
interface Environment {
	/** The lock file that pins it, or nothing, for the one VS Code runs in. */
	lock: string | null;
	prefix?: string;
	/** The variables that activate it. */
	vars?: Record<string, string>;
	/** The variables to remove, which would point Python at another environment. */
	remove?: string[];
}

/** A file of the cells of a markdown file, as the server knows it. */
interface Shadow {
	uri: vscode.Uri;
	version: number;
	text: string;
	/** The lines of its cells, which only its answers within are kept of. */
	cells: { start: number; end: number }[];
}

/** A server, for the files whose cells run in one environment. */
class Server implements vscode.Disposable {
	/** The cells of each markdown file it was told of, by the markdown's uri. */
	readonly shadows = new Map<string, Shadow>();
	/** The directories outside the workspace it was told of as folders of their own. */
	private readonly folders = new Set<string>();

	constructor(
		readonly client: LanguageClient,
		private readonly diagnostics: vscode.DiagnosticCollection,
		/** Shows the diagnostics of a file of cells, by its uri, on the markdown it is of. */
		private readonly publish: (uri: string, diagnostics: vscode.Diagnostic[]) => void,
	) {}

	/** Asks for the diagnostics of a file of cells, from a server that answers them when
	 * asked, as one does to a client that can ask, rather than publishing them, and may
	 * say so only once it has started. The client asks of the documents it syncs itself,
	 * which are none. */
	async pull(shadow: Shadow) {
		const version = shadow.version;
		try {
			const report = await this.client.sendRequest(DocumentDiagnosticRequest.type, { textDocument: { uri: shadow.uri.toString() } });
			if (report.kind === 'full' && shadow.version === version) {
				this.publish(shadow.uri.toString(), await this.client.protocol2CodeConverter.asDiagnostics(report.items));
			}
		} catch {
			// Cancelled by a newer change, whose own request answers, or not answered by a
			// server that publishes them instead.
		}
	}

	/** Tells the server of the cells of the document as they are now, and returns them. */
	sync(document: vscode.TextDocument): Shadow {
		const lines = document.getText().split(/\r?\n/);
		const cells = pythonCells(lines);
		const python = lines.map(() => '');
		for (const { start, end } of cells) {
			for (let line = start; line < end; line++) python[line] = lines[line];
		}
		const text = python.join('\n');
		const key = document.uri.toString();
		const shadow = this.shadows.get(key);
		if (!shadow) {
			this.addFolder(document.uri);
			const name = document.uri.path.split('/').pop();
			const uri = vscode.Uri.joinPath(document.uri, '..', `.${name}.py`);
			const opened = { uri, version: 1, text, cells };
			this.shadows.set(key, opened);
			void this.client.sendNotification(DidOpenTextDocumentNotification.type, {
				textDocument: { uri: uri.toString(), languageId: 'python', version: 1, text },
			});
			void this.pull(opened);
			return opened;
		}
		shadow.cells = cells;
		if (shadow.text !== text) {
			shadow.text = text;
			shadow.version++;
			void this.client.sendNotification(DidChangeTextDocumentNotification.type, {
				textDocument: { uri: shadow.uri.toString(), version: shadow.version },
				contentChanges: [{ text }],
			});
			void this.pull(shadow);
		}
		return shadow;
	}

	/** Tells the server of the directory of a file outside the workspace, as a folder of
	 * its own, as a server may check only the files in its folders, as ty does. */
	private addFolder(uri: vscode.Uri) {
		if (vscode.workspace.getWorkspaceFolder(uri)) {
			return;
		}
		const dir = vscode.Uri.joinPath(uri, '..');
		if (this.folders.has(dir.toString())) {
			return;
		}
		this.folders.add(dir.toString());
		void this.client.sendNotification(DidChangeWorkspaceFoldersNotification.type, {
			event: { added: [{ uri: dir.toString(), name: dir.path.split('/').pop() ?? '' }], removed: [] },
		});
	}

	close(document: vscode.Uri) {
		const shadow = this.shadows.get(document.toString());
		if (shadow) {
			this.shadows.delete(document.toString());
			this.diagnostics.delete(document);
			void this.client.sendNotification(DidCloseTextDocumentNotification.type, { textDocument: { uri: shadow.uri.toString() } });
		}
	}

	/** The markdown file whose cells the file at `uri` holds. */
	markdownOf(uri: string): { document: vscode.Uri; shadow: Shadow } | undefined {
		for (const [document, shadow] of this.shadows) {
			if (shadow.uri.toString() === uri) {
				return { document: vscode.Uri.parse(document), shadow };
			}
		}
		return undefined;
	}

	dispose() {
		for (const document of this.shadows.keys()) {
			this.diagnostics.delete(vscode.Uri.parse(document));
		}
		void this.client.stop().catch(() => {});
	}
}

export class PythonServers implements vscode.Disposable {
	/** The environment of the cells of the files in each directory, by its path. */
	private readonly environments = new Map<string, Promise<Environment | undefined>>();
	/** The server of each environment, by its lock file, or nothing where none was found. */
	private readonly servers = new Map<string, Promise<Server | undefined>>();
	/** The server each markdown file was told to, by its uri. */
	private readonly assigned = new Map<string, Server>();
	private readonly diagnostics = vscode.languages.createDiagnosticCollection('slides-python');
	private readonly output = vscode.window.createOutputChannel('Slides Python Language Server');
	private readonly timers = new Map<string, ReturnType<typeof setTimeout>>();
	private readonly disposables: vscode.Disposable[] = [];
	/** The completion items each server gave, to resolve them with. */
	private readonly given = new WeakMap<vscode.CompletionItem, Server>();

	constructor(
		private readonly extensionUri: vscode.Uri,
		private readonly decks: Decks,
		private readonly log: vscode.LogOutputChannel,
	) {
		const selector = SELECTOR.map((filter) => ({ ...filter, scheme: 'file' }));
		this.disposables.push(
			this.diagnostics,
			this.output,
			vscode.languages.registerHoverProvider(selector, { provideHover: (document, position, token) => this.hover(document, position, token) }),
			vscode.languages.registerSignatureHelpProvider(
				selector,
				{ provideSignatureHelp: (document, position, token, context) => this.signatureHelp(document, position, token, context) },
				'(',
				',',
			),
			vscode.languages.registerDefinitionProvider(selector, { provideDefinition: (document, position, token) => this.definition(document, position, token) }),
			vscode.languages.registerReferenceProvider(selector, { provideReferences: (document, position, context, token) => this.references(document, position, context, token) }),
			vscode.workspace.onDidOpenTextDocument((document) => void this.track(document)),
			vscode.workspace.onDidChangeTextDocument(({ document }) => this.later(document)),
			vscode.workspace.onDidCloseTextDocument((document) => this.untrack(document.uri)),
			this.decks.onDidChange(() => vscode.workspace.textDocuments.forEach((document) => void this.track(document))),
			vscode.workspace.onDidChangeConfiguration((event) => {
				if (event.affectsConfiguration('slides.pythonServer')) {
					this.restart();
				}
			}),
		);
		vscode.workspace.textDocuments.forEach((document) => void this.track(document));
	}

	/** The server of the document's cells, if one was found, told of them as they are now. */
	async serverFor(document: vscode.TextDocument): Promise<{ server: Server; shadow: Shadow } | undefined> {
		if (document.uri.scheme !== 'file' || !this.decks.isDeck(document)) {
			return undefined;
		}
		const server = await this.start(document.uri);
		if (!server || server.client.state !== State.Running) {
			return undefined;
		}
		this.assigned.set(document.uri.toString(), server);
		return { server, shadow: server.sync(document) };
	}

	/** Starts the server of a deck as it opens, for its diagnostics. */
	private async track(document: vscode.TextDocument) {
		if (vscode.languages.match(SELECTOR, document) > 0) {
			await this.serverFor(document);
		}
	}

	private untrack(uri: vscode.Uri) {
		this.assigned.get(uri.toString())?.close(uri);
		this.assigned.delete(uri.toString());
	}

	/** Tells the server of the document's cells, once it has not changed for a moment. */
	private later(document: vscode.TextDocument) {
		const key = document.uri.toString();
		const server = this.assigned.get(key);
		if (!server) {
			return;
		}
		clearTimeout(this.timers.get(key));
		this.timers.set(key, setTimeout(() => {
			this.timers.delete(key);
			server.sync(document);
		}, 300));
	}

	/** Stops every server, so that the next request starts them again, as the settings say. */
	private restart() {
		for (const server of this.servers.values()) {
			void server.then((server) => server?.dispose());
		}
		this.servers.clear();
		this.assigned.clear();
		vscode.workspace.textDocuments.forEach((document) => void this.track(document));
	}

	/** The environment the cells of the file at `uri` run in. */
	private environment(uri: vscode.Uri): Promise<Environment | undefined> {
		const dir = path.dirname(uri.fsPath);
		let environment = this.environments.get(dir);
		if (!environment) {
			environment = (async () => {
				const slidesRs = await command(this.extensionUri);
				return new Promise<Environment | undefined>((resolve) => {
					execFile(slidesRs, ['environment', uri.fsPath], { cwd: dir }, (error, stdout, stderr) => {
						if (error) {
							this.log.warn(`${vscode.workspace.asRelativePath(uri)}: could not activate the environment of its cells: ${stderr.trim() || error.message}`);
							this.environments.delete(dir);
							resolve(undefined);
						} else {
							resolve(JSON.parse(stdout) as Environment);
						}
					});
				});
			})();
			this.environments.set(dir, environment);
		}
		return environment;
	}

	/** The server of the environment the cells of the file at `uri` run in, started the
	 * first time it is asked for, or nothing if none is found in it. */
	private async start(uri: vscode.Uri): Promise<Server | undefined> {
		const setting = vscode.workspace.getConfiguration('slides').get<string>('pythonServer', '').trim();
		if (setting === 'off') {
			return undefined;
		}
		const environment = await this.environment(uri);
		if (!environment) {
			return undefined;
		}
		const key = environment.lock ?? '';
		let server = this.servers.get(key);
		if (!server) {
			server = this.launch(environment, setting);
			this.servers.set(key, server);
		}
		return server;
	}

	private async launch(environment: Environment, setting: string): Promise<Server | undefined> {
		const env: Record<string, string | undefined> = { ...process.env };
		for (const name of environment.remove ?? []) delete env[name];
		Object.assign(env, environment.vars);
		const where = environment.lock ? vscode.workspace.asRelativePath(environment.lock) : 'the environment VS Code runs in';
		const found = find(setting, env.PATH ?? '');
		if (!found) {
			this.log.info(
				setting
					? `the Python language server \`${setting}\` is not in ${where} nor on the PATH`
					: `no Python language server in ${where} nor on the PATH, so the cells are completed by the one VS Code has: install basedpyright or ty in it for diagnostics, hovers and definitions too`,
			);
			return undefined;
		}
		const python = pythonOf(environment.prefix);
		const client = new LanguageClient(
			'slides-python',
			'Slides Python Language Server',
			{ command: found.command, args: found.args, options: { cwd: environment.lock ? path.dirname(environment.lock) : undefined, env } },
			{
				// Told of the cells' files by hand, rather than of the documents VS Code opens.
				documentSelector: [],
				outputChannel: this.output,
				middleware: {
					handleDiagnostics: (uri, diagnostics) => this.publish(uri.toString(), diagnostics),
					workspace: {
						// The cells' interpreter, which pyright asks for as python.pythonPath.
						configuration: async (params, token, next) => {
							const result = await next(params, token);
							if (!python || !Array.isArray(result)) return result;
							return params.items.map((item, i) => (item.section === 'python' ? { ...(result[i] ?? {}), pythonPath: python } : result[i]));
						},
					},
				},
			},
		);
		const server = new Server(client, this.diagnostics, (uri, diagnostics) => this.publish(uri, diagnostics));
		try {
			await client.start();
		} catch (error) {
			this.log.error(`could not start the Python language server ${found.command} for ${where}: ${error}`);
			return undefined;
		}
		// The diagnostics may have changed, as when the server has read the environment.
		client.onRequest(DiagnosticRefreshRequest.type, () => {
			server.shadows.forEach((shadow) => void server.pull(shadow));
		});
		this.log.info(`started the Python language server ${found.command} for ${where}`);
		return server;
	}

	/** Shows the server's diagnostics of a file of cells on the markdown it is of, but
	 * for an expression left unused at the end of a cell, which the kernel shows. */
	private publish(uri: string, diagnostics: vscode.Diagnostic[]) {
		for (const server of new Set(this.assigned.values())) {
			const found = server.markdownOf(uri);
			if (found) {
				const { cells } = found.shadow;
				const lines = found.shadow.text.split('\n');
				const lastLine = (end: number) => {
					let line = end - 1;
					while (line > 0 && lines[line].trim() === '') line--;
					return line;
				};
				const kept = diagnostics.filter((diagnostic) => {
					const cell = cells.find(({ start, end }) => start <= diagnostic.range.start.line && diagnostic.range.end.line < end);
					if (!cell) return false;
					const code = typeof diagnostic.code === 'object' ? diagnostic.code.value : diagnostic.code;
					return !(/unused.?expression/i.test(String(code)) && diagnostic.range.start.line === lastLine(cell.end));
				});
				this.diagnostics.set(found.document, kept);
				return;
			}
		}
	}

	/** Whether the position is in one of the cells. */
	private inCell(shadow: Shadow, position: vscode.Position): boolean {
		return shadow.cells.some(({ start, end }) => start <= position.line && position.line < end);
	}

	/** The completions of a cell, or nothing outside the cells or without a server. */
	async completion(
		document: vscode.TextDocument,
		position: vscode.Position,
		token: vscode.CancellationToken,
		context: vscode.CompletionContext,
	): Promise<vscode.CompletionList | undefined> {
		const found = await this.serverFor(document);
		if (!found || !this.inCell(found.shadow, position)) return undefined;
		const { server, shadow } = found;
		const { client } = server;
		const result = await client.sendRequest(
			CompletionRequest.type,
			{
				textDocument: { uri: shadow.uri.toString() },
				position: client.code2ProtocolConverter.asPosition(position),
				// Counted from one, rather than from zero as VS Code counts them.
				context: { triggerKind: (context.triggerKind + 1) as 1 | 2 | 3, triggerCharacter: context.triggerCharacter },
			},
			token,
		);
		const converted = await client.protocol2CodeConverter.asCompletionResult(result, undefined, token);
		if (!converted) return undefined;
		const list = Array.isArray(converted) ? new vscode.CompletionList(converted) : converted;
		const first = new vscode.Position(shadow.cells[0].start, 0);
		for (const item of list.items) {
			this.given.set(item, server);
			item.additionalTextEdits = keepInCells(item.additionalTextEdits, shadow, first);
		}
		return list;
	}

	/** A completion item a server gave, with its documentation. */
	async resolve(item: vscode.CompletionItem, token: vscode.CancellationToken): Promise<vscode.CompletionItem> {
		const server = this.given.get(item);
		if (!server) return item;
		const { client } = server;
		const resolved = await client.sendRequest(CompletionResolveRequest.type, client.code2ProtocolConverter.asCompletionItem(item), token);
		const converted = client.protocol2CodeConverter.asCompletionItem(resolved);
		converted.additionalTextEdits = item.additionalTextEdits;
		return converted;
	}

	private async hover(document: vscode.TextDocument, position: vscode.Position, token: vscode.CancellationToken) {
		const found = await this.serverFor(document);
		if (!found || !this.inCell(found.shadow, position)) return undefined;
		const { client } = found.server;
		const result = await client.sendRequest(HoverRequest.type, this.at(found, position), token);
		return client.protocol2CodeConverter.asHover(result);
	}

	private async signatureHelp(document: vscode.TextDocument, position: vscode.Position, token: vscode.CancellationToken, context: vscode.SignatureHelpContext) {
		const found = await this.serverFor(document);
		if (!found || !this.inCell(found.shadow, position)) return undefined;
		const { client } = found.server;
		const result = await client.sendRequest(
			SignatureHelpRequest.type,
			{
				...this.at(found, position),
				// Left without the help shown, which the server works out again.
				context: { triggerKind: context.triggerKind, triggerCharacter: context.triggerCharacter, isRetrigger: context.isRetrigger },
			},
			token,
		);
		return client.protocol2CodeConverter.asSignatureHelp(result, token);
	}

	private async definition(document: vscode.TextDocument, position: vscode.Position, token: vscode.CancellationToken) {
		const found = await this.serverFor(document);
		if (!found || !this.inCell(found.shadow, position)) return undefined;
		const { client } = found.server;
		const result = await client.sendRequest(DefinitionRequest.type, this.at(found, position), token);
		const converted = await client.protocol2CodeConverter.asDefinitionResult(result, token);
		if (!converted) return undefined;
		const fix = (uri: vscode.Uri) => (uri.toString() === found.shadow.uri.toString() ? document.uri : uri);
		if (Array.isArray(converted)) {
			return (converted as (vscode.Location | vscode.LocationLink)[]).map((link) =>
				'targetUri' in link ? { ...link, targetUri: fix(link.targetUri) } : new vscode.Location(fix(link.uri), link.range),
			) as vscode.Location[] | vscode.LocationLink[];
		}
		return new vscode.Location(fix(converted.uri), converted.range);
	}

	private async references(document: vscode.TextDocument, position: vscode.Position, context: vscode.ReferenceContext, token: vscode.CancellationToken) {
		const found = await this.serverFor(document);
		if (!found || !this.inCell(found.shadow, position)) return undefined;
		const { client } = found.server;
		const result = await client.sendRequest(ReferencesRequest.type, { ...this.at(found, position), context }, token);
		const locations = await client.protocol2CodeConverter.asReferences(result, token);
		const shadow = found.shadow.uri.toString();
		return locations?.map((location) => (location.uri.toString() === shadow ? new vscode.Location(document.uri, location.range) : location));
	}

	/** Where a request is about, in the cells' file. */
	private at(found: { server: Server; shadow: Shadow }, position: vscode.Position) {
		return {
			textDocument: { uri: found.shadow.uri.toString() },
			position: found.server.client.code2ProtocolConverter.asPosition(position),
		};
	}

	dispose() {
		this.timers.forEach((timer) => clearTimeout(timer));
		for (const server of this.servers.values()) {
			void server.then((server) => server?.dispose());
		}
		this.disposables.forEach((disposable) => disposable.dispose());
	}
}

/** The command that starts the server the setting names, a command or a path, or else
 * the first of `SERVERS` on the `PATH`, with the arguments it takes. */
function find(setting: string, PATH: string): { command: string; args: string[] } | undefined {
	const executable = (name: string): string | undefined => {
		const names = process.platform === 'win32' ? [`${name}.exe`, `${name}.cmd`, name] : [name];
		if (path.isAbsolute(name)) return fs.existsSync(name) ? name : undefined;
		for (const dir of PATH.split(path.delimiter).filter(Boolean)) {
			for (const candidate of names) {
				const full = path.join(dir, candidate);
				if (fs.existsSync(full)) return full;
			}
		}
		return undefined;
	};
	if (setting) {
		const command = executable(setting);
		if (!command) return undefined;
		const known = SERVERS.find((server) => path.basename(setting).replace(/\.(exe|cmd)$/, '') === server.command);
		return { command, args: known?.args ?? ['--stdio'] };
	}
	for (const server of SERVERS) {
		const command = executable(server.command);
		if (command) return { command, args: server.args };
	}
	return undefined;
}

/** The Python of the environment installed at `prefix`, by conda or pixi, or by uv. */
function pythonOf(prefix: string | undefined): string | undefined {
	if (!prefix) return undefined;
	const candidates = process.platform === 'win32'
		? [path.join(prefix, 'python.exe'), path.join(prefix, 'Scripts', 'python.exe')]
		: [path.join(prefix, 'bin', 'python')];
	return candidates.find((candidate) => fs.existsSync(candidate));
}
