---
theme: dark
---

# Leg by leg

~~~python
import numpy as np
import matplotlib.pyplot as plt
from slides_rs import Motion, Step

stops = np.array([[0, 0], [1, 1], [2, 0], [3, 1]])
plt.figure(figsize=(6, 2.5))
plt.axis("off")
plt.plot(*stops.T, "s", color="0.6")
(dot,) = plt.plot(*stops[0], "o", color="C1")
for i, (a, b) in enumerate(zip(stops, stops[1:]), start=1):
    (leg,) = plt.plot(*np.array([a, b]).T, color="0.8")
    _ = Motion(dot, along=leg).starts(Step(i)).timing(duration=3, easing="ease-in-out")
~~~
