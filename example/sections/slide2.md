~~~python
import numpy as np
import matplotlib.pyplot as plt

plt.rcParams["figure.facecolor"] = "none"
plt.rcParams["axes.facecolor"] = "none"
plt.rcParams["svg.fonttype"] = "none"

x = np.linspace(-10, 10, 300)
~~~

# Figures with steps

~~~python
plt.plot(x, np.sin(x), gid="step=1")
plt.plot(x, np.cos(x), gid="step=2")
for i, phi in enumerate(np.linspace(0, np.pi/2, 10), start=3):
    plt.plot(
        x,
        np.sin(x + phi),
        gid=f"step={i}..{i + 1}",
        color="C2",
    )
~~~

---

# Figures are centered

## Sinusoidals

### Sine

~~~python
plt.plot(x, np.sin(x))
None
~~~

### Cosine

~~~python
plt.plot(x, np.cos(x))
None
~~~
