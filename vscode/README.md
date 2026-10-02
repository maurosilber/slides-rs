# Slides Notebook

Opens the markdown of a [slides-rs](../) deck as a notebook in VS Code. Each
`~~~` code cell the deck runs is a code cell, the markdown between them is a
markdown cell, and each code cell shows the outputs the deck saved for it in
`_outputs`. The cells run as in any Python notebook, on the kernel the deck
runs them on, their outputs are saved where the deck reads them as they run,
and a figure steps through its steps as on its slide.

The markdown is read and written, and the outputs found and saved, by the
deck's own Rust code compiled to WASI and run with
[vscode-wasm](https://github.com/microsoft/vscode-wasm). A cell has the
address the deck gives it, and the file is written back byte for byte, except
for the cells that changed. The cells run on `slides-rs kernel`, which the
extension comes with, as the WebAssembly cannot start a process.

## Use

Run **Slides: Open as Slides Notebook** on a deck's markdown (from its editor
title, the explorer, or the command palette), or use *Reopen Editor With… ›
Slides Notebook*.

The commands show on the markdown files that are decks, or parts of one: a file
with a key of the deck's frontmatter, an `<import-slide>` or a `~~~` code
cell, one another file imports, and any file named `*.slides.md`. That name
opens in the *Slides Markdown* language, which is highlighted as markdown but
leaves out what VS Code does for markdown, as its preview, outline and link
checks, and what other extensions add to it. Its name still ends in `.md`, so
GitHub and slides-rs read it as markdown.

In the YAML frontmatter of any markdown file, as a notebook's first cell or in
the text editor, the keys the deck reads are completed, with *Trigger Suggest*
(<kbd>Ctrl</kbd>+<kbd>Space</kbd>) as markdown suggests nothing as it is typed,
and their values after the space that follows a key. Hovering over a key
describes it. Both come from the deck's
[`frontmatter.schema.json`](../src/frontmatter.schema.json).

In the text editor, the Python cells have the diagnostics, hovers, signature
help, definitions, references and completions of a Python language server that
the extension starts in the environment the cells run in, so that it resolves
their imports as the kernel does, in the environment the deck's lock file
pins, which `slides-rs environment` activates. It is the one the editor has
for Python: that of the ty, basedpyright or Pyright extension that is enabled,
or Jedi if `python.languageServer` is `Jedi`, as installed in that environment
or on the `PATH`, or else as its extension bundles it, which needs nothing
installed. Its settings, as `ty.*` or `basedpyright.analysis.*`, hold for it.
Without one, it is the first of `basedpyright-langserver`, `ty` and
`pyright-langserver` installed. The `slides.pythonServer` setting names
another, or is `off`. The server is told
of the file's Python cells, in order, as the cells before each one run, each
line where it is in the deck and the rest blank, as a file that is never
written. An expression left unused at the end of a cell is not reported, as
the kernel shows it. Its log is the *Slides Python Language Server* output.

Without such a server, as with Pylance, which only its own extension can
start, a cell's completions, after a `.` or with *Trigger Suggest*, hovers,
signature help, definitions and references are those of the Python language
server VS Code has, but not its diagnostics: it is asked of a hidden file next
to the deck, `.<deck>.md.<id>.py`, which holds the file's Python cells and is
deleted once it answers. In a notebook, its cells
are Python documents the server completes itself.

In the text editor, the line where each slide begins, its `---` or an
`<import-slide>`, is highlighted, with the slide it begins written after it,
as the deck breaks them: not at a `---` under a line of text, which makes it
a heading, nor in a code block. Its color is `slides.slideBreak`, which a
`workbench.colorCustomizations` setting can change, as can
`slides.alternateSlide`, the background of every other slide. Under each
break, or under the frontmatter for the first slide, buttons move the slide,
with all its markdown, up, down, or before any other, and add a slide before
or after it; the rules and blank lines between slides stay where they are.
**Slides: New Slide Above**, **New Slide Below**, **Move Slide Up**, **Move
Slide Down** and **Move Slide To…** do the same for the slide the cursor is
in. The `src` of an `<import-slide>` is a link: Ctrl/Cmd+click it to open the
imported file.

The headings are highlighted by what they are on the slide: its title, a `#`,
in `slides.title`, a subtitle, a `##`, in `slides.subtitle`, and a column's
`###` in `slides.columnHeading`. Each column, from its `###` up to the next
heading of its level or above, has a line down it, before its text, in
`slides.column` and `slides.alternateColumn` by turns, so that two side by
side tell apart. Before each line that steps, in a column of their own, the
steps it shows in are written as the deck numbers them, to tell why an
animation steps as it does: `3` from the third step on, `2..4` from the second
up to the fourth, and a `*` if it takes no space while hidden; above each
slide, how many steps it has. Clicking that, or hovering over the column,
collapses it to a mark on each line that steps, and expands it again. A code
cell counts as one step, whatever its outputs, and the frontmatter's `steps`
is the file's own, as it is unless another file imports it.

Each of these is a setting, to turn off for a quieter editor:
`slides.editor.alternateSlides`, `slides.editor.headings`,
`slides.editor.columns` and `slides.editor.steps`.

The outputs are read from the closest `_outputs` directory above the file, as
a deck that imports it saves them there, or else from the one next to it.
Showing them is an edit, so a notebook that was saved is saved again when it
opens. That writes the file exactly as it was.

The outputs of another extension's kernel are saved with the notebook: those
of a code cell are written under its address unless they are the ones already
saved there. Outputs read from `_outputs` are never written again, since after
an edit they belong to the code before the edit. Saved this way, they have no
list of the files the cell read, so the next `slides-rs` render runs the cell
again.

## Run

The **slides-rs** kernel runs the cells, one kernel per notebook, as the deck
does: next to the file, in the environment its `pixi.lock` or `uv.lock` pins,
on `xpython` or else `python3`, with figures as the SVG a slide shows.

A cell's outputs are saved in `_outputs` as soon as it runs, under its address.
If the kernel ran the cells before it, in order, and nothing else, since it
started, they are saved with the files the cell read, as the deck saves them,
and the next render shows them rather than running the cell again. Otherwise,
as after running a cell again or interrupting one, they are saved without, and
the render runs the cell again. An interrupted cell's outputs are not saved.

A figure whose parts matplotlib's `gid` marks as steps opens at its first step.
Clicking it, the arrow keys once it is focused, or the buttons above it step
through it, as the deck's own `steps.js` numbers the steps. Clearing
**Animate** shows every figure whole, and is remembered. In a dark theme, figures are inverted,
as the deck's dark theme inverts them.

**Slides: Restart Kernel**, in the notebook's toolbar, starts a new kernel for
the next cell. xeus-python's `xpython` cannot stop a cell, so interrupting one
stops its kernel too. The `slides.path` setting names another `slides-rs` to
run, and running cells needs VS Code on the desktop.

## Show

**Slides: Show Slides**, in the editor's title, the notebook's toolbar, the
explorer, or the command palette, renders the deck with `slides-rs --watch`
and opens its page in VS Code's integrated browser, or in the default browser
where VS Code has none. It renders again whenever the deck or a file it
imports is saved, and the integrated browser reloads the page, as its
`workbench.browser.autoReloadOnFileChange` setting has it by default.
It opens the page at the slide and the step the cursor is at, if the file is
in a text editor, counting the slides its imports bring, in the tab that
shows the page already rather than a new one, and so does every save of the
file shown, once it is rendered.
A file another one in the workspace imports shows its own slides as they are
in that deck, with `--deck`: in its theme and shape, and stepping as it does.
**Slides: Stop Rendering Slides** stops every deck being rendered. Like
running cells, this needs VS Code on the desktop.

## Build

The Rust half builds for `wasm32-wasip1-threads`, which vscode-wasm runs and
conda-forge packages, and `slides-rs` for this machine, so the `.vsix` runs on
this platform alone. The `vscode` pixi environment has the tools of both:

```sh
pixi run vscode-build    # npm install, then build into dist
pixi run vscode-dev      # open the example deck in a window running the extension
pixi run vscode-test     # run the tests in a VS Code of their own
pixi run vscode-install  # package a .vsix and install it in VS Code
```

The tasks call the `code` CLI in `/Applications/Visual Studio Code.app`. The
extension depends on
[WASM WASI Core](https://marketplace.visualstudio.com/items?itemName=ms-vscode.wasm-wasi-core),
which VS Code installs along with it.

The tests download a VS Code of their own. Set `VSCODE_EXECUTABLE` to use an
installed one, and unset `ELECTRON_RUN_AS_NODE` when running from VS Code's
terminal.
