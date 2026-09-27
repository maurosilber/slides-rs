# Slides Notebook

Opens the markdown of a [slides-rs](../) deck as a notebook in VS Code. Each
`~~~` code cell the deck runs is a code cell, the markdown between them is a
markdown cell, and each code cell shows the outputs the deck saved for it in
`_outputs`.

The markdown is read and written, and the outputs found and saved, by the
deck's own Rust code compiled to WASI and run with
[vscode-wasm](https://github.com/microsoft/vscode-wasm). A cell has the
address the deck gives it, and the file is written back byte for byte, except
for the cells that changed.

## Use

Run **Slides: Open as Slides Notebook** on a markdown file (from its editor
title, the explorer, or the command palette), or use *Reopen Editor With… ›
Slides Notebook*.

The outputs are read from the closest `_outputs` directory above the file, as
a deck that imports it saves them there, or else from the one next to it.
Showing them is an edit, so a notebook that was saved is saved again when it
opens. That writes the file exactly as it was.

On save, the outputs of a code cell are written under its address unless they
are the ones already saved there. Outputs read from `_outputs` are never
written again, since after an edit they belong to the code before the edit.
The extension has no kernel, so outputs come from another extension's
controller. Outputs saved this way have no list of the files the cell read,
so the next `slides-rs` render runs the cell again.

## Build

The Rust half builds for `wasm32-wasip1-threads`, which vscode-wasm runs and
conda-forge packages. The `vscode` pixi environment has both halves' tools:

```sh
pixi run vscode-build    # npm install, then build into dist
pixi run vscode-dev      # open the example deck in a window running the extension
pixi run vscode-install  # package a .vsix and install it in VS Code
```

The tasks call the `code` CLI in `/Applications/Visual Studio Code.app`. The
extension depends on
[WASM WASI Core](https://marketplace.visualstudio.com/items?itemName=ms-vscode.wasm-wasi-core),
which VS Code installs along with it.

`npm test` runs the tests in a VS Code of its own. Set `VSCODE_EXECUTABLE` to
use an installed one, and unset `ELECTRON_RUN_AS_NODE` when running from
VS Code's terminal.
