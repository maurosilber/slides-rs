# Sines

~~~python
import numpy as np
import matplotlib.pyplot as plt

plt.rcParams['figure.facecolor'] = 'none'
plt.rcParams['axes.facecolor'] = 'none'
plt.rcParams['svg.fonttype'] = 'none'

x = np.pi * np.linspace(-1, 1, 1000)
~~~

~~~python
for i in range(10):
    plt.plot(x, np.cos(i * x), gid=f"fragment {i}..{i+2}")
~~~
