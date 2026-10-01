// The Python of a deck's code cells, in the markdown text editor, as a Python language
// server has it: one the extension starts in the environment the cells run in, which
// `slides-rs environment` activates as for the kernel, so that the server resolves the
// imports as the cells do when they run. It is the one the `slides.pythonServer` setting
// names, or else the one the editor has for Python, as the extension that brings it or
// the Python extension's `python.languageServer` says, installed in the environment or
// as that extension bundles it, or else the first of `SERVERS` that is installed. The server is told of a Python file of the
// cells, in order, each line where it is in the markdown and the rest blank, which is
// never written to disk, so that its answers hold for the markdown as they are. Without
// a server, as with Pylance, which only its own extension can start, python.ts asks the
// one VS Code has instead.

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
	ServerOptions,
	State,
	TransportKind,
} from 'vscode-languageclient/node';
import { Decks } from './decks';
import { command } from './kernel';
import { keepInCells, pythonCells } from './python';

/** The servers looked for, in order, by the command that starts one. */
const SERVERS: { command: string; args: string[] }[] = [
	{ command: 'basedpyright-langserver', args: ['--stdio'] },
	{ command: 'ty', args: ['server'] },
	{ command: 'pyright-langserver', args: ['--stdio'] },
];

/** How to start a server: a command, or a script run on VS Code's own Node, and where it
 * comes from, for the log. */
type Launch = ({ command: string; args: string[] } | { module: string }) & { name: string; from: string };

/** A server an extension brings for Python: the command it is installed as, and how the
 * extension bundles it, if it does. */
interface EditorServer {
	command: string;
	args: string[];
	bundled(): Launch | undefined;
}

/** The extensions that bring a Python language server, which a user picks one with, by
 * their id, most preferred first, with how each bundles its server. */
const EXTENSIONS: { id: string; server: (extension: vscode.Extension<unknown>) => EditorServer | undefined }[] = [
	{
		id: 'astral-sh.ty',
		server: (extension) => {
			const ty = vscode.workspace.getConfiguration('ty');
			if (ty.get<boolean>('disableLanguageServices')) return undefined;
			return {
				command: 'ty',
				args: ['server'],
				bundled() {
					const command = [...ty.get<string[]>('path', []), path.join(extension.extensionPath, 'bundled', 'libs', 'bin', exe('ty'))]
						.find((candidate) => fs.existsSync(candidate));
					return command ? { command, args: ['server'], name: 'ty', from: 'the ty extension' } : undefined;
				},
			};
		},
	},
	{ id: 'detachhead.basedpyright', server: (extension) => pyright(extension, 'basedpyright') },
	{ id: 'ms-pyright.pyright', server: (extension) => pyright(extension, 'pyright') },
];

/** The server of pyright's extension, or basedpyright's, which bundles it as a script. */
function pyright(extension: vscode.Extension<unknown>, name: string): EditorServer {
	return {
		command: `${name}-langserver`,
		args: ['--stdio'],
		bundled() {
			const module = path.join(extension.extensionPath, 'dist', 'server.js');
			return fs.existsSync(module) ? { module, name, from: `the ${name} extension` } : undefined;
		},
	};
}

/** The server the editor has for Python: Jedi, if the Python extension is set to it,
 * which bundles it as a script its interpreter runs, or else that of the first extension
 * of `EXTENSIONS` that is enabled, which a disabled one is not as VS Code lists them. */
function editorServer(python: string | undefined, PATH: string): EditorServer | undefined {
	if (vscode.workspace.getConfiguration('python').get<string>('languageServer') === 'Jedi') {
		const extension = vscode.extensions.getExtension('ms-python.python');
		return {
			command: 'jedi-language-server',
			args: [],
			bundled() {
				const script = extension && path.join(extension.extensionPath, 'python_files', 'run-jedi-language-server.py');
				const interpreter = python ?? executable('python3', PATH) ?? executable('python', PATH);
				return script && interpreter && fs.existsSync(script)
					? { command: interpreter, args: [script], name: 'jedi-language-server', from: 'the Python extension' }
					: undefined;
			},
		};
	}
	for (const { id, server } of EXTENSIONS) {
		const extension = vscode.extensions.getExtension(id);
		const found = extension && server(extension);
		if (found) return found;
	}
	return undefined;
}

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
export interface Shadow {
	uri: vscode.Uri;
	version: number;
	text: string;
	/** The lines of its cells, which only its answers within are kept of. */
	cells: { start: number; end: number }[];
}

/** A server, and the file of a document's cells it was told of. */
export interface Found {
	server: Server;
	shadow: Shadow;
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
	private readonly output = vscode.window.createOutputChannel('Slides Python Language Server', { log: true });
	private readonly timers = new Map<string, ReturnType<typeof setTimeout>>();
	private readonly disposables: vscode.Disposable[] = [];
	/** The completion items each server gave, to resolve them with. */
	private readonly given = new WeakMap<vscode.CompletionItem, Server>();

	constructor(
		private readonly extensionUri: vscode.Uri,
		private readonly decks: Decks,
		private readonly log: vscode.LogOutputChannel,
	) {
		this.disposables.push(
			this.diagnostics,
			this.output,
			vscode.workspace.onDidOpenTextDocument((document) => void this.track(document)),
			vscode.workspace.onDidChangeTextDocument(({ document }) => this.later(document)),
			vscode.workspace.onDidCloseTextDocument((document) => this.untrack(document.uri)),
			this.decks.onDidChange(() => vscode.workspace.textDocuments.forEach((document) => void this.track(document))),
			vscode.workspace.onDidChangeConfiguration((event) => {
				const affects = ['slides.pythonServer', 'python.languageServer', 'ty.path', 'ty.disableLanguageServices'];
				if (affects.some((section) => event.affectsConfiguration(section))) {
					this.restart();
				}
			}),
			// An extension that brings a server, enabled or disabled.
			vscode.extensions.onDidChange(() => this.restart()),
		);
		vscode.workspace.textDocuments.forEach((document) => void this.track(document));
	}

	/** The server of the document's cells, if one was found, told of them as they are now. */
	async serverFor(document: vscode.TextDocument): Promise<Found | undefined> {
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
		await this.serverFor(document);
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
		const python = pythonOf(environment.prefix);
		const found = choose(setting, env.PATH ?? '', python);
		if (!found) {
			this.log.info(
				setting
					? `the Python language server \`${setting}\` is not in ${where} nor on the PATH`
					: `no Python language server for ${where}, so the cells are asked of the one VS Code has: install the ty or basedpyright extension, or either in the environment, for diagnostics too`,
			);
			return undefined;
		}
		const options = { cwd: environment.lock ? path.dirname(environment.lock) : undefined, env };
		const serverOptions: ServerOptions = 'module' in found
			? { module: found.module, transport: TransportKind.ipc, options }
			: { command: found.command, args: found.args, options };
		const client = new LanguageClient(
			'slides-python',
			'Slides Python Language Server',
			serverOptions,
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
			this.log.error(`could not start ${found.name}, from ${found.from}, for ${where}: ${error}`);
			return undefined;
		}
		// The diagnostics may have changed, as when the server has read the environment.
		client.onRequest(DiagnosticRefreshRequest.type, () => {
			server.shadows.forEach((shadow) => void server.pull(shadow));
		});
		this.log.info(`started ${found.name}, from ${found.from}, for ${where}`);
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

	// What the server has to say at a position in the cells of a document, which python.ts
	// asks it, rather than the one VS Code has, once it finds the document has a server.

	async completion(found: Found, position: vscode.Position, token: vscode.CancellationToken, context: vscode.CompletionContext): Promise<vscode.CompletionList | undefined> {
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

	async hover(found: Found, position: vscode.Position, token: vscode.CancellationToken) {
		const { client } = found.server;
		const result = await client.sendRequest(HoverRequest.type, this.at(found, position), token);
		return client.protocol2CodeConverter.asHover(result);
	}

	async signatureHelp(found: Found, position: vscode.Position, token: vscode.CancellationToken, context: vscode.SignatureHelpContext) {
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

	async definition(found: Found, document: vscode.TextDocument, position: vscode.Position, token: vscode.CancellationToken) {
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

	async references(found: Found, document: vscode.TextDocument, position: vscode.Position, context: vscode.ReferenceContext, token: vscode.CancellationToken) {
		const { client } = found.server;
		const result = await client.sendRequest(ReferencesRequest.type, { ...this.at(found, position), context }, token);
		const locations = await client.protocol2CodeConverter.asReferences(result, token);
		const shadow = found.shadow.uri.toString();
		return locations?.map((location) => (location.uri.toString() === shadow ? new vscode.Location(document.uri, location.range) : location));
	}

	/** Where a request is about, in the cells' file. */
	private at(found: Found, position: vscode.Position) {
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

/** The server to start: the one the setting names, a command or a path, or else the one
 * the editor has for Python, installed on the `PATH`, which the environment's comes first
 * on, or else as its extension bundles it, or else the first of `SERVERS` on the `PATH`. */
function choose(setting: string, PATH: string, python: string | undefined): Launch | undefined {
	if (setting) {
		const command = executable(setting, PATH);
		if (!command) return undefined;
		const name = path.basename(setting).replace(/\.(exe|cmd)$/, '');
		const known = SERVERS.find((server) => server.command === name);
		return { command, args: known?.args ?? ['--stdio'], name, from: 'the slides.pythonServer setting' };
	}
	const editor = editorServer(python, PATH);
	if (editor) {
		const command = executable(editor.command, PATH);
		const launch = command ? { command, args: editor.args, name: editor.command, from: path.dirname(command) } : editor.bundled();
		if (launch) return launch;
	}
	for (const server of SERVERS) {
		const command = executable(server.command, PATH);
		if (command) return { command, args: server.args, name: server.command, from: path.dirname(command) };
	}
	return undefined;
}

/** The path of the command `name`, on the `PATH`, unless it is a path itself. */
function executable(name: string, PATH: string): string | undefined {
	if (path.isAbsolute(name)) return fs.existsSync(name) ? name : undefined;
	const names = process.platform === 'win32' ? [`${name}.exe`, `${name}.cmd`, name] : [name];
	for (const dir of PATH.split(path.delimiter).filter(Boolean)) {
		for (const candidate of names) {
			const full = path.join(dir, candidate);
			if (fs.existsSync(full)) return full;
		}
	}
	return undefined;
}

/** The name of an executable file, as the platform has it. */
function exe(name: string): string {
	return process.platform === 'win32' ? `${name}.exe` : name;
}

/** The Python of the environment installed at `prefix`, by conda or pixi, or by uv. */
function pythonOf(prefix: string | undefined): string | undefined {
	if (!prefix) return undefined;
	const candidates = process.platform === 'win32'
		? [path.join(prefix, 'python.exe'), path.join(prefix, 'Scripts', 'python.exe')]
		: [path.join(prefix, 'bin', 'python')];
	return candidates.find((candidate) => fs.existsSync(candidate));
}
