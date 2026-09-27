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

~~~python
step = Step(0, 2)
for i in range(3):
    plt.plot(x, np.cos(i * x), gid=step)
    step = step.next()
~~~

~~~python
t = np.linspace(0, 2 * np.pi, 500, endpoint=False)

plt.figure(figsize=(3, 3))
point = plt.scatter(1, 0)
(line,) = plt.plot(np.cos(t), np.sin(t))
Motion(
    point,
    line,
    step="step=1",
    duration=3,
    repeat="indefinite",
)
~~~

~~~python
t = np.linspace(0, 2 * np.pi, 500, endpoint=False)

plt.figure(figsize=(3, 3))
step = Step(1)
for a in np.linspace(1, 5, 10):
    point = plt.scatter(a, 0)
    (line,) = plt.plot(a * np.cos(t), a * np.sin(t), linestyle="--")
    step = step.next()
    Motion(
        point,
        line,
        duration=a,
        repeat="indefinite",
    )
~~~

~~~python
t = np.linspace(-10, 10, 500)

plt.figure(figsize=(6, 3))
plt.xlim(-3, 3)
point, = plt.plot(t, np.exp(-(t**2)))
line = plt.axhline(-1, xmin=0, xmax=0.3, color="C1")
Motion(
    point,
    line,
    duration=a,
    easing="ease-in-out"
)
~~~
