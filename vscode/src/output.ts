// Outputs as VS Code holds them, from those the Rust half passes, their bytes
// in base64.

import * as vscode from 'vscode';

/** The output metadata naming the file an output is saved in. */
export const SAVED_AS = 'slides.savedAs';

/** An output saved in the outputs directory, and the file it is saved in. */
export interface Saved {
	mime: string;
	data: string;
	name: string;
}

export function cellOutput(saved: Saved): vscode.NotebookCellOutput {
	return new vscode.NotebookCellOutput([new vscode.NotebookCellOutputItem(fromBase64(saved.data), saved.mime)], { [SAVED_AS]: saved.name });
}

export function toBase64(bytes: Uint8Array): string {
	let binary = '';
	// In chunks, as spreading a large array overflows the stack.
	for (let start = 0; start < bytes.length; start += 0x8000) {
		binary += String.fromCharCode(...bytes.subarray(start, start + 0x8000));
	}
	return btoa(binary);
}

export function fromBase64(base64: string): Uint8Array {
	const binary = atob(base64);
	const bytes = new Uint8Array(binary.length);
	for (let index = 0; index < binary.length; index++) {
		bytes[index] = binary.charCodeAt(index);
	}
	return bytes;
}
