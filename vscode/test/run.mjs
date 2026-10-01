// Runs the tests in a VS Code of their own, with the WASI extension installed.
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { downloadAndUnzipVSCode, resolveCliArgsFromVSCodeExecutablePath, runTests } from '@vscode/test-electron';

const root = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
let vscodeExecutablePath = process.env.VSCODE_EXECUTABLE ?? (await downloadAndUnzipVSCode());
// On macOS, VS Code's binary is `Code` since 1.139, which @vscode/test-electron still calls `Electron`.
if (!fs.existsSync(vscodeExecutablePath)) {
	vscodeExecutablePath = path.join(path.dirname(vscodeExecutablePath), 'Code');
}
const extensions = path.join(root, '.vscode-test', 'extensions');
const [cli, ...args] = resolveCliArgsFromVSCodeExecutablePath(vscodeExecutablePath);
execFileSync(cli, [...args, '--extensions-dir', extensions, '--install-extension', 'ms-vscode.wasm-wasi-core'], { stdio: 'inherit' });

// Run from VS Code's own terminal, it would start as Node rather than as VS Code.
delete process.env.ELECTRON_RUN_AS_NODE;

// A deck outside the repository, whose outputs the tests save.
const deck = fs.mkdtempSync(path.join(os.tmpdir(), 'slides-notebook-'));
await runTests({
	vscodeExecutablePath,
	extensionDevelopmentPath: root,
	extensionTestsPath: path.join(root, 'test', 'suite.cjs'),
	launchArgs: ['--extensions-dir', extensions, '--user-data-dir', path.join(root, '.vscode-test', 'user-data'), '--disable-workspace-trust', path.dirname(root)],
	extensionTestsEnv: { SLIDES_DECK: deck },
});
console.log(`deck: ${deck}`);
