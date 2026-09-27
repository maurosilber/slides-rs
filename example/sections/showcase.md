# slides-rs

Markdown in, a deck out: text, equations and figures that step in as you
present them.

Press → to step, ← to step back, ↓ and ↑ to finish or restart a slide, and
<kbd>A</kbd> to show every step at once.

---

# Markdown

A slide is plain markdown, with **bold**, *italic*, `code` and
[links](https://commonmark.org), split from the next one by a `---` rule.

> A quote, as the theme dresses it.

A `#` heading is the slide's title, a `##` a subtitle, and each `###` a column.

---

# Lists step one item at a time

- Each item is a step of its own
- shown after the one before
  - nested items too
- and numbered ones:

1. first
2. second

---

# Columns

Before the first heading, everything shows at once.

### Left

- Each `###` is a column,
- a step of its own,
- then its items, one by one.

### Right

- Columns sit side by side,
- as many as there are headings.

---

# Subtitles

## A `##` is a step, the width of the slide

### Under it

- Its columns come after it,

### Beside it

- side by side as ever.

---

# Turning steps off

### Stepped

- one item
- at a time

### All at once { steps=false }

- every item
- shows with its heading,
- as `{ steps=false }` says.

---

<style>
    .accent { color: var(--accent); }
</style>

# Raw HTML { #raw-html .accent }

A heading takes an id and classes, as `{ #raw-html .accent }` does here,
which a `<style>` on the slide can dress.

Any element with a `step` attribute steps: <span step>like this</span>,
<span also>and this along with it</span>.

<p step="..3" collapse>With <code>collapse</code>, this sentence…</p>
<p step="3" collapse>…is replaced by this one, in its place.</p>

---

# Equations step too

$$
\begin{aligned}
x^2 + 2x - 3 &= 0 \\
\step{(x + 1)^2 - 4} &\also{= 0} \\
\step{x + 1} &\also{= \pm 2} \\
\step{x} &\also{\in \{1, -3\}}
\end{aligned}
$$

`\step{...}` shows one step after the latest, and `\also{...}` along with it.

---

# Rewriting an equation

### `\step`: hidden, it keeps its space

$$
\step[..2]{x^2 + 2x - 3}
\step[2..3]{(x + 1)^2 - 4}
\step[3..]{(x + 3)(x - 1)}
= 0
$$

### `\step*`: hidden, it takes none

$$
\step*[..2]{x^2 + 2x - 3}
\step*[2..3]{(x + 1)^2 - 4}
\step*[3..]{(x + 3)(x - 1)}
= 0
$$

---

# In a sentence

The roots of $\step*[..2]{x^2 + 2x - 3}\step*[1..2]{\;\text{(factor it)}}\step*[2..]{(x + 3)(x - 1)}$
are $\step[3]{-3}$ and $\also{1}$: the text after the polynomial moves along
as it is rewritten, and the roots, hidden, keep their place.

A range is written as in Rust: `[2..4]` shows from step 2 up to 4, `[..3]`
until 3, and `[3..]`, or `[3]`, from 3 on.

$$
x^2 + 2x - 3 = \underbrace{x^2 + 2x + 1}_{\step{(x + 1)^2}} - 4
$$

---

# Code

### Shown

A backtick fence is shown, as code:

```python
print("Hello from the deck")
```

### Run

A tilde fence is run, and shows what it outputs:

~~~python
print("Hello from the deck")
~~~

Its outputs are saved, and it runs again only once its code, or a file it
reads, changes.

---

# Rich outputs

~~~python
from IPython.display import HTML, Markdown

Markdown("Markdown, from a *cell*, with its items stepping:\n\n- one\n- two")
~~~

~~~python
HTML("<p>Or <b>HTML</b>, as a table of data shows itself.</p>")
~~~

---

# Figures

~~~python
import matplotlib.pyplot as plt
import numpy as np
from slides_rs import Motion, Step

# Drawn for the theme: see-through, with its fonts.
plt.rcParams["figure.facecolor"] = "none"
plt.rcParams["axes.facecolor"] = "none"
plt.rcParams["svg.fonttype"] = "none"

x = np.linspace(-np.pi, np.pi, 300)
~~~

A figure's artists step as their `gid` says: `step`, `also`, or a range.

~~~python
plt.figure(figsize=(6, 3))
plt.plot(x, np.sin(x), gid="step")
plt.plot(x, np.cos(x), gid="step")
plt.fill_between(x, np.sin(x), np.cos(x), alpha=0.2, gid="also")
None
~~~

---

# One curve at a time

`Step(1, 2)` is `step=1..2`, which `next()` moves along.

~~~python
plt.figure(figsize=(6, 3))
step = Step(1, 2)
for i, phase in enumerate(np.linspace(0, np.pi, 6)):
    plt.plot(x, np.sin(x + phase), color="C0", gid=step.next(i))
~~~

---

# Figures in columns

### Sine

~~~python
plt.figure(figsize=(4, 3))
plt.plot(x, np.sin(x))
None
~~~

### Cosine

~~~python
plt.figure(figsize=(4, 3))
plt.plot(x, np.cos(x), color="C1")
None
~~~

---

# Motion

### Along a line

~~~python
t = np.linspace(0, 2 * np.pi, 200)
plt.figure(figsize=(3, 3))
(circle,) = plt.plot(np.cos(t), np.sin(t))
(dot,) = plt.plot(1, 0, "o")
_ = Motion(dot, along=circle).timing(duration=3).repeat()
~~~

`Motion(dot, along=line)`, which moves as its column shows.

### Turning with it

~~~python
plt.figure(figsize=(3, 3))
(circle,) = plt.plot(np.cos(t), np.sin(t), linestyle="--")
(arrow,) = plt.plot(1, 0, marker=">", markersize=12, color="C3")
_ = Motion(arrow, along=circle).timing(duration=3).rotate("auto").repeat()
~~~

`.rotate("auto")` turns it along the path.

---

# Easing and waiting

~~~python
plt.figure(figsize=(6, 2.5))
plt.axis("off")
for y, label in [(2, "linear"), (1, "ease-in-out"), (0, "waits halfway")]:
    (line,) = plt.plot([0, 1], [y, y], color="0.8")
    (dot,) = plt.plot(0, y, "o")
    plt.text(-0.05, y, label, ha="right", va="center")
    motion = Motion(dot, along=line).repeat()
    if label == "linear":
        motion.timing(duration=2)
    elif label == "ease-in-out":
        motion.timing(duration=2, easing="ease-in-out")
    else:
        # Waits half a second, goes halfway, waits again, and goes on.
        motion.timing([0.5, 1, 1.5, 2], fraction=[0, 0.5, 0.5, 1], easing="ease-out")
~~~

`.timing(duration=...)`, with an `easing=`, or with the times `t` it is each
`fraction=` of the path: before `t[0]`, it waits.

---

# Timed as plotted

~~~python
# A point of a rolling wheel, fastest at the top.
t = np.linspace(0, 4 * np.pi, 400)
plt.figure(figsize=(6, 2))
plt.gca().set_aspect("equal")
(line,) = plt.plot(t - np.sin(t), 1 - np.cos(t))
(dot,) = plt.plot(0, 0, "o")
_ = Motion(dot, along=line).starts(Step(1)).timing(t / 2).repeat()
~~~

With a time at each vertex, `.timing(t)` follows the line as it was plotted,
and `.starts(Step(1))` waits for the next step.

---

# Lap after lap

~~~python
x = np.linspace(0, 2 * np.pi, 100)
plt.figure(figsize=(6, 2.5))
plt.xlim(0, 6 * np.pi)
plt.plot(3 * x, np.sin(3 * x), color="0.8")
(line,) = plt.plot(x, np.sin(x))
(dot,) = plt.plot(0, 0, "o")
_ = Motion(dot, along=line).timing(duration=1.5).repeat(3, accumulate=True)
(line,) = plt.plot(x, np.sin(x) - 2.5)
(dot,) = plt.plot(0, -2.5, "o")
_ = Motion(dot, along=line).timing(duration=1.5).repeat(2).hold(False)
~~~

`.repeat(3, accumulate=True)` goes on from where each lap ended, and
`.hold(False)` goes back to the start once done.

---

# Leg by leg

~~~python
stops = np.array([[0, 0], [1, 1], [2, 0], [3, 1]])
plt.figure(figsize=(6, 2.5))
plt.axis("off")
plt.plot(*stops.T, "s", color="0.6")
for i, (a, b) in enumerate(zip(stops, stops[1:]), start=1):
    (leg,) = plt.plot(*np.array([a, b]).T, color="0.8")
    (dot,) = plt.plot(*a, "o", color="C1", gid=Step(i))
    _ = Motion(dot, along=leg).timing(duration=3, easing="ease-in-out")
~~~

Each leg moves in a step of its own: stepping on before one ends takes it to
its end, as the next begins.

---

# The deck

### Frontmatter

```yaml
---
theme: dark           # light, paper, projector
aspect-ratio: 16:9    # 4:3, 1.6, none to fill
steps: false          # this file all at once
invert-figures: false # figures as drawn
fade-figures: false   # figures step at once
---
```

Other files join with `<import-slide src="..." />`.

### Command line { steps=false }

- `slides-rs talk.md` writes `talk.html`
- `--watch` renders again on every change
- `--self-contained` puts every file inside the page
- `--kernel` picks `xpython` or `python3`
- `--clean` removes outputs no cell uses anymore
- The VS Code extension runs the cells as a notebook, stepping through its figures.
