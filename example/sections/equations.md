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
