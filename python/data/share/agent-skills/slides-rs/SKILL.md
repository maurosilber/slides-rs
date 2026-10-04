---
name: slides-rs
description: "Write and render slides-rs decks: markdown files that become HTML slide decks with stepped reveals, KaTeX equations, and Python cells whose outputs and matplotlib figures (with stepping artists and animations via the slides_rs package) are embedded. Use when creating or editing a slides-rs presentation, running the slides-rs command, or drawing figures with slides_rs.Step / slides_rs.Motion / slides_rs.Slider."
---

# slides-rs

`slides-rs` turns a markdown file into an HTML slide deck. Slides reveal
step by step. Python cells run on a Jupyter kernel, and their outputs,
including matplotlib figures, are embedded in the slides.

## Command line

```sh
slides-rs talk.md                  # writes _outputs/talk.html, which git ignores
slides-rs talk.md out/deck.html    # or to a given path
slides-rs talk.md --watch          # re-render on every change to talk.md or a file it imports
slides-rs talk.md --open           # open the page in the default browser once rendered
slides-rs talk.md --self-contained # inline CSS, JS and images: one shareable .html (KaTeX still from CDN)
slides-rs talk.md --kernel python3 # xpython (xeus-python) or python3 (ipykernel); default: first installed
slides-rs talk.md --clean          # after rendering, delete saved outputs no cell uses anymore
slides-rs part.md --deck talk.md   # only part.md's slides, as they are in talk.md, which imports it
```

`--clean` wipes everything in the `_outputs/` directory that the current deck
doesn't use, including outputs of other decks rendered in the same directory,
but keeps their pages.

Links in the markdown (images, a `theme:` stylesheet) are written relative to
the deck's `.md`; the page rewrites them to work from wherever it is written.

In the browser: → / ← step forward and back, ↓ finishes the slide (then goes
to the next), ↑ restarts it (then goes to the previous), and <kbd>A</kbd>
toggles showing every step at once. The URL hash tracks the slide.

## Deck structure

```markdown
---
theme: dark           # light (default look), dark, paper, projector, or a path to your own .css
aspect-ratio: 16:9    # 4:3, 16/10, 1.6, or none to fill the window
steps: false          # show this file's slides all at once (imported files inherit unless they say)
figures:
  invert: false       # show figures as drawn instead of filtered by the theme (dark inverts)
  fade: false         # figure steps appear at once instead of fading
  rush: 0.5           # seconds an animation still playing takes to finish when the next step begins (default 0.2)
---

# Slide title

Plain CommonMark: **bold**, *italic*, `code`, [links](https://example.com), > quotes.

---

# Next slide
```

- `---` separates slides. The frontmatter keys are exactly those above;
  other keys, such as a title or author, are ignored.
- `#` is the slide title. `##` is a full-width subtitle, which counts as a
  step. Each `###` starts a column, and consecutive `###` columns sit side by
  side.
- Headings take attributes: `# Title { #some-id .some-class }`, and
  `{ steps=false }` turns stepping off for that heading's content.
- `<import-slide src="sections/intro.md" />` inlines another markdown file's
  slides. That file may have its own frontmatter.
- Raw HTML and `<style>` blocks are allowed. Theme colors are CSS
  variables, such as `var(--accent)`.

## Steps

By default, reveals happen in document order:

- The slide opens with its first element, usually the `#` title.
- Each element after it (heading, paragraph, code block, figure, quote) is
  one step, in order.
- A list is not a step itself: each of its items (nested ones too) is one.
- `step` is the only attribute that controls steps. Any HTML element with a
  `step` attribute steps: `<span step>` is one step after the latest. An
  element with a `step` of its own joins the step before it, instead of
  being a step itself.
- Explicit ranges use Rust syntax: `step="2..4"` shows from step 2 up to
  (not including) 4, `step="..3"` from the element's own step until 3, and
  `step="3.."` or `step="3"` from 3 on. A range hidden again before it shows
  is warned of. They are the slide's steps, wherever the element is: `1` is
  the step the slide opens with.
- Signed numbers are relative: `step="+1"`, `+0` or `-1` count from the
  step of the previous sibling that steps, or, if there is none, from the
  element it is inside: `<li step="+0">` shows with the item before it,
  `<span step="+1">` one step after its paragraph. They work in ranges too,
  as `step="+0..+2"`, and inside a column they count in that column's
  steps. To count from an element elsewhere, name it and count from its
  name, as `a+1` (see below).
- `{ steps=parallel }` on a heading makes the `###` columns under it step
  together: the first step of each column at once, then the second of each.
  `{ steps=interleave }` makes them take turns: the first step of each
  column, one after another, then the second of each. Inside such a column,
  range numbers are that column's steps (its heading is 0), so
  `step="3"` or `\step[3..]{...}` shows with the other columns' third step.
- Name a step to sync with it from anywhere on the slide: `step="a"` (on a
  heading, `### Method { step=a }`, in math `\step[a]{...}`, in a figure
  `gid="step=a"`) is a step of its own, as an unmarked element would be,
  named `a`. Refer to it with the name and a signed offset: `a+0` is the same
  step, `a+1` one after, `a-1` one before, as `<p step="a+0">`,
  `\step[a+0..b+0]{...}`, or `Step("a+0", "a+2")` in Python. Use `step="a"`
  only once per slide: a repeated name stays with the first step, and the
  renderer and the VS Code extension warn, suggesting `a+0`.
- `collapse` makes a hidden element take no space. Use it to swap content in
  place:

  ```html
  <p step="..3" collapse>First version…</p>
  <p step="3" collapse>…replaced by this.</p>
  ```

## Math (KaTeX)

Use `$inline$` and `$$display$$`. Parts of an equation can step:

- `\step{...}` appears one step after the latest.
- `\step[2..4]{...}` takes an explicit range, as above, and `\step[+1]{...}`
  counts from the step of the `\step` before it, or the one it is inside:
  `\step{x + 1} &\step[+0]{= \pm 2}` shows both sides together.
- A hidden `\step{}` keeps its space, while `\step*{}` takes none. Use
  `\step*` to rewrite an expression in place:

  ```latex
  $$
  \step*[..2]{x^2 + 2x - 3}
  \step*[2..3]{(x + 1)^2 - 4}
  \step*[3..]{(x + 3)(x - 1)}
  = 0
  $$
  ```

## Code: shown vs. run

- A backtick fence (```` ```python ````) is only displayed, as highlighted
  code.
- A tilde fence (`~~~python`) is executed, and the slide shows its outputs
  instead of the code: stdout, stderr, rich reprs (`IPython.display.Markdown`
  and `HTML`, whose markdown lists step too), images, and tracebacks.

Execution model:

- Each markdown file is one notebook: its cells share one kernel and run in
  order. Imported files run as their own notebooks.
- Cells run in the Python environment pinned by the nearest `pixi.lock` or
  `uv.lock` in the deck's directory or above it, which slides-rs activates,
  installing it first if needed. They do not run in the environment
  slides-rs was launched from. That environment needs a kernel (`xeus-python`
  or `ipykernel`), plus `matplotlib` and `slides-rs` (the Python package) for
  figures.
- Outputs are cached in `_outputs/` next to the deck. A cell reruns only when
  its code, an earlier cell in the same file, the lock file, or a file it
  read changes. slides-rs audits which files the cells open.
- The last expression's repr is shown, as in Jupyter. End a figure cell with
  `None`, or assign to `_ = ...`, so that no `[<Line2D ...>]` text appears
  under the figure.
- Figures are rendered as inline SVG (slides-rs sets
  `InlineBackend.figure_formats = ['svg']`), and one figure is drawn per
  `plt.figure()`.

Recommended setup cell for figures that blend with the theme:

```python
plt.rcParams["figure.facecolor"] = "none"
plt.rcParams["axes.facecolor"] = "none"
plt.rcParams["svg.fonttype"] = "none"   # text uses the deck's fonts
```

## Stepping figure artists: `gid`

matplotlib writes an artist's `gid` into the SVG, and slides-rs reads it as
that artist's step:

```python
plt.plot(x, np.sin(x), gid="step")                               # next step
plt.plot(x, np.cos(x), gid="step=cos")                           # the one after, named
plt.fill_between(x, np.sin(x), np.cos(x), alpha=.2, gid="step=cos+0")  # with the cosine
```

Relative steps (`step=+0`) follow the SVG's order, which is matplotlib's
drawing order (by `zorder`: patches and fills before lines), not the order of
the code. To show an artist with another one, name it as above.

`slides_rs.Step` builds range gids. It is a `str` subclass, so it can be
passed directly as `gid=`:

```python
from slides_rs import Step

Step(3, 5)                 # "step=3..5"  shown on steps 3 and 4
Step(3)                    # "step=3.."   from 3 on
Step(stop=3)               # "step=..3"   until 3
Step()                     # "step=.."    always
Step(1, 3, collapse=True)  # "step*=1..3" collapse variant
Step("+0", "+2")           # "step=+0..+2" from the previous stepping artist's step

step = Step(1, 2)
for i, phase in enumerate(phases):
    plt.plot(x, np.sin(x + phase), gid=step.next(i))   # one curve per step
# .next(n) / .previous(n) shift both bounds by n; a number below 0 is an error
```

## Animating figure artists: `Motion`

`slides_rs.Motion` moves an artist along another artist's path, using SVG
`<animateMotion>`. Draw the moving artist at the start of the path. It plays
when its step shows and restarts when the step is hidden again. Every method
returns the motion, so calls chain.

```python
from slides_rs import Motion, Step

(path,) = plt.plot(np.cos(t), np.sin(t))
(dot,) = plt.plot(1, 0, "o")
_ = (
    Motion(dot, along=path)
    .starts(Step(2))                  # start on step 2 instead of when the dot appears
    .timing(duration=3, easing="ease-in-out")
    .rotate("auto")                   # turn along the path ("auto-reverse", degrees, or None)
    .repeat()                         # forever; .repeat(3), .repeat(seconds=...)
)
```

- `.timing(duration=s)` crosses the whole path in `s` seconds.
- `.timing(t)` sets a time for each vertex of `along`, so a line plotted
  from `x(t), y(t)` is followed as plotted. It waits at the start until
  `t[0]`.
- `.timing(t, fraction=f)` puts the artist at fraction `f[i]` (0–1) of the
  path at time `t[i]`, so it can pause by repeating a fraction.
- `easing=` takes `"linear"`, `"ease"`, `"ease-in"`, `"ease-out"`,
  `"ease-in-out"`, a cubic-Bézier 4-tuple, one easing per interval, or
  `"discrete"` to jump from vertex to vertex.
- `.repeat(n, accumulate=True)` continues each lap from where the previous
  one ended. `accumulate` can't be combined with `rotate`.
- `.hold(False)` returns to the start when done. By default the artist stays
  at the end.
- `.remove()` undoes the motion.
- Animations run only in the deck's SVG. A PNG shows the artist where it
  was drawn.
- Stepping forward while an animation plays rushes it to its end (see
  `figures.rush`). Stepping back plays it backward.

## Scrubbing motions with a slider: `Slider`

`slides_rs.Slider` shows a figure with a slider that sets the time its
motions are at, instead of the deck playing them. Make it the cell's last
expression: it is shown as HTML, and the figure is closed so that it is not
shown twice. Every method returns the slider, so calls chain.

```python
from slides_rs import Motion, Slider

(line,) = plt.plot(t, np.sin(t))
(dot,) = plt.plot(0, 0, "o")
(
    Slider(plt.gcf(), Motion(dot, along=line).timing(t))
    .range(0, 10, step=0.1)           # default: 0 to the end of the longest motion, 100 ticks
    .value(2)                         # where it starts; default: the start
    .label("t")
    .play(speed=2)                    # ▶ plays 2 slider-seconds per second; .play(False) hides it
    .style(width="80%", accent="tomato")
)
```

- The slider's value is the motions' time in seconds, so `.timing(t)` with
  the times a line was plotted at puts the artist where the line was at `t`.
- `.style(part, **properties)` adds inline CSS to a part: `"slider"` (the
  default), `"figure"`, `"controls"`, `"button"`, `"label"`, `"input"` or
  `"output"`. Underscores become dashes (`font_size`), `None` removes a
  property, and `css={...}` takes any other name, such as a custom property.
- On the slider, `accent`, `track`, `thumb`, `text`, `track_height` and
  `thumb_size` set its `--slider-*` custom properties. They default to the
  theme's colors.
- A deck stylesheet can restyle every slider through `.slides-rs-slider`,
  `.slides-rs-slider-controls` and `.slides-rs-slider > svg`.
- Motions driven by a slider ignore `.starts(Step(...))`.

## Tips for writing decks

- Keep one idea per slide, and use lists or `###` columns for progressive
  reveals.
- To see a whole slide at once, press <kbd>A</kbd> in the browser, or set
  `steps: false` or `{ steps=false }`.
- Use `--watch` while editing. Cached outputs keep re-renders fast.
- A VS Code extension can run a deck's cells as a notebook and step through
  its figures.
