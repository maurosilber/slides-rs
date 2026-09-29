# Motion

~~~python
import matplotlib.pyplot as plt
import numpy as np
from slides_rs import Motion, Slider, Step

plt.rcParams["figure.facecolor"] = "none"
plt.rcParams["axes.facecolor"] = "none"
plt.rcParams["svg.fonttype"] = "none"
~~~

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
(dot,) = plt.plot(*stops[0], "o", color="C1")
for i, (a, b) in enumerate(zip(stops, stops[1:]), start=1):
    (leg,) = plt.plot(*np.array([a, b]).T, color="0.8")
    _ = Motion(dot, along=leg).starts(Step(i)).timing(duration=3, easing="ease-in-out")
~~~


Each leg moves in a step of its own: stepping on before one ends takes it to
its end, as the next begins, and stepping back plays it backward.

---

# Scrubbed with a slider

~~~python
t = np.linspace(0, 4 * np.pi, 400)
figure = plt.figure(figsize=(6, 2))
plt.gca().set_aspect("equal")
(path,) = plt.plot(t - np.sin(t), 1 - np.cos(t), color="0.8")
(axle,) = plt.plot(t, np.ones_like(t), linewidth=0)
(wheel,) = plt.plot(np.sin(t), 1 + np.cos(t))
(dot,) = plt.plot(0, 0, "o")
Slider(
    figure,
    Motion(dot, along=path).timing(t),
    Motion(wheel, along=axle).timing(t),
).label("t").play(speed=2).style(accent="#2ca02c")
~~~

`Slider(figure, *motions)` moves them to the time it is set to, rather than
playing them: dragged, or played with ▶. `.range()`, `.value()`, `.label()`,
`.play()` and `.style()` set it up.
