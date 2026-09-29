# The deck

### Frontmatter

```yaml
---
theme: dark           # light, paper, projector
aspect-ratio: 16:9    # 4:3, 1.6, none to fill
steps: false          # this file all at once
figures:
  invert: false       # as drawn
  fade: false         # steps at once
  rush: 0.5           # seconds to finish an animation cut short
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
