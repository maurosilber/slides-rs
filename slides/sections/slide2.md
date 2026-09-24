# Cuadratic

~~~python
import numpy as np
import matplotlib.pyplot as plt

x = np.linspace(-10, 10, 100)
~~~

~~~python
plt.plot(x, x, gid="fragment 1")
plt.plot(x, x**2, gid="fragment 2")
plt.plot(x, np.cos(x), gid="fragment 1")
None
~~~

---

# Coseno

~~~python
plt.plot(x, np.cos(x))
None
~~~