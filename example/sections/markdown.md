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

After the title, each element is a step: this paragraph, each heading, each item.

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

# Columns in parallel { steps=parallel }

### Plain

- With `{ steps=parallel }`,
- the columns under a heading
- step together.

### Ranged

<p step="3">A range in a column is a step of the column: this is its third.</p>

---

# Columns taking turns { steps=interleave }

### Question

- What steps?
- In which order?

### Answer

- Every element, in turn,
- one column after the other.

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
