// Runs the tests in a VS Code of their own, with the slides-rs that cargo builds.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { downloadAndUnzipVSCode, runTests } from '@vscode/test-electron';

const root = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
let vscodeExecutablePath = process.env.VSCODE_EXECUTABLE ?? (await downloadAndUnzipVSCode());
// On macOS, VS Code's binary is `Code` since 1.139, which @vscode/test-electron still calls `Electron`.
if (!fs.existsSync(vscodeExecutablePath)) {
	vscodeExecutablePath = path.join(path.dirname(vscodeExecutablePath), 'Code');
}
const extensions = path.join(root, '.vscode-test', 'extensions');

// The extension starts the slides-rs the setting names, rather than the workspace's pixi
// environment's, which does not have one. The repository opens as a workspace of its own,
// whose setting holds over its folder's.
const slidesRs = path.join(path.dirname(root), 'target', 'debug', 'slides-rs');
const workspace = path.join(root, '.vscode-test', 'tests.code-workspace');
// SLIDES_PYTHON_SERVER names the Python language server the code cells have, as
// `SLIDES_PYTHON_SERVER=ty pixi run vscode-test`, or else it is the first one found.
const settings = { 'slides.path': slidesRs };
if (process.env.SLIDES_PYTHON_SERVER) settings['slides.pythonServer'] = process.env.SLIDES_PYTHON_SERVER;
fs.writeFileSync(workspace, JSON.stringify({ folders: [{ path: path.dirname(root) }], settings }, null, '\t'));

// Run from VS Code's own terminal, it would start as Node rather than as VS Code.
delete process.env.ELECTRON_RUN_AS_NODE;

// A deck outside the repository, whose outputs the tests save.
const deck = fs.mkdtempSync(path.join(os.tmpdir(), 'slides-notebook-'));
await runTests({
	vscodeExecutablePath,
	extensionDevelopmentPath: root,
	extensionTestsPath: path.join(root, 'test', 'suite.cjs'),
	launchArgs: ['--extensions-dir', extensions, '--user-data-dir', path.join(root, '.vscode-test', 'user-data'), '--disable-workspace-trust', workspace],
	extensionTestsEnv: { SLIDES_DECK: deck, SLIDES_RS: slidesRs, SLIDES_PYTHON_SERVER: process.env.SLIDES_PYTHON_SERVER ?? '' },
});
console.log(`deck: ${deck}`);
