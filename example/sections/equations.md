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
