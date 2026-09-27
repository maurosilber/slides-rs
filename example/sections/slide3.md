# Sines

~~~python
import matplotlib.pyplot as plt
import numpy as np
from slides_rs import Motion, Step

plt.rcParams['figure.facecolor'] = 'none'
plt.rcParams['axes.facecolor'] = 'none'
plt.rcParams['svg.fonttype'] = 'none'

x = np.pi * np.linspace(-1, 1, 1000)
~~~

---

~~~python
step = Step(0, 2)
for i in range(3):
    plt.plot(x, np.cos(i * x), gid=step)
    step = step.next()
~~~

---

~~~python
t = np.linspace(0, 2 * np.pi, 500, endpoint=False)

plt.figure(figsize=(3, 3))
point = plt.scatter(1, 0)
(line,) = plt.plot(np.cos(t), np.sin(t))
Motion(point, along=line).starts("step=1").timing(duration=3).repeat()
~~~

---

~~~python
t = np.linspace(0, 2 * np.pi, 500, endpoint=False)

plt.figure(figsize=(3, 3))
step = Step(1)
for a in np.linspace(1, 5, 10):
    step = step.next()
    point = plt.scatter(a, 0, gid=step)
    (line,) = plt.plot(a * np.cos(t), a * np.sin(t), linestyle="--")
    Motion(point, along=line).timing(duration=a).repeat()
~~~

---

~~~python
t = np.linspace(-10, 10, 500)

plt.figure(figsize=(6, 3))
plt.xlim(-3, 3)
plt.grid()
point, = plt.plot(t, np.exp(-(t**2)))
Motion(point, along=line).timing(duration=1, easing="ease-in-out")
~~~

---

~~~python
# A wheel's point, where the plot has it at each time: fastest at the top.
t = np.linspace(0, 4 * np.pi, 400)

plt.figure(figsize=(6, 2))
plt.gca().set_aspect("equal")
(line,) = plt.plot(t - np.sin(t), 1 - np.cos(t), gid=Step(1))
(point,) = plt.plot(0, 0, "o", gid=Step(1))
Motion(point, along=line).starts(Step(2)).timing(t / 2).repeat()
~~~
