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

A figure's artists step as their `gid` says: `step`, a name, or a range, as
`step=cos+0`, which matplotlib draws first, but shows with the cosine.

~~~python
plt.figure(figsize=(6, 3))
plt.plot(x, np.sin(x), gid="step")
plt.plot(x, np.cos(x), gid="step=cos")
plt.fill_between(x, np.sin(x), np.cos(x), alpha=0.2, gid="step=cos+0")
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
