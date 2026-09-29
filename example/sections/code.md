# Code

### Shown

A backtick fence is shown, as code:

```python
print("Hello from the deck")
```

### Run

A tilde fence is run, and shows what it outputs:

~~~python
print("Hello from the deck")
~~~

Its outputs are saved, and it runs again only once its code, or a file it
reads, changes.

---

# Rich outputs

~~~python
from IPython.display import HTML, Markdown

Markdown("Markdown, from a *cell*, with its items stepping:\n\n- one\n- two")
~~~

~~~python
HTML("<p>Or <b>HTML</b>, as a table of data shows itself.</p>")
~~~
