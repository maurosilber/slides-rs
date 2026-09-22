"""Run one notebook cell and print its Jupyter outputs as JSON.

The cell source arrives on stdin and the output list goes to stdout, so a
fresh process per cell is all the isolation a cell needs.
"""

import ast
import base64
import io
import json
import sys
import traceback
from contextlib import redirect_stderr, redirect_stdout

FILENAME = "<cell>"

# Kept before any user code runs, in case the cell reassigns sys.stdout.
report = sys.stdout

# (dunder repr method, mime type, whether the payload is bytes)
REPR_METHODS = [
    ("_repr_html_", "text/html", False),
    ("_repr_markdown_", "text/markdown", False),
    ("_repr_svg_", "image/svg+xml", False),
    ("_repr_latex_", "text/latex", False),
    ("_repr_json_", "application/json", False),
    ("_repr_png_", "image/png", True),
    ("_repr_jpeg_", "image/jpeg", True),
]


def mime_bundle(obj):
    """What Jupyter would display for `obj`, keyed by mime type."""
    bundle = {"text/plain": repr(obj)}
    for method, mime, is_binary in REPR_METHODS:
        # Looked up on the type, the way IPython's formatters do it.
        formatter = getattr(type(obj), method, None)
        if formatter is None:
            continue
        try:
            payload = formatter(obj)
        except Exception:
            continue
        if payload is None:
            continue
        if isinstance(payload, tuple):  # (data, metadata)
            payload = payload[0]
        bundle[mime] = b64(payload) if is_binary else payload
    return bundle


def b64(data):
    if isinstance(data, str):
        data = data.encode()
    return base64.b64encode(data).decode("ascii")


def figures():
    """Open matplotlib figures as PNGs, the way the inline backend does."""
    pyplot = sys.modules.get("matplotlib.pyplot")
    if pyplot is None:
        return []
    outputs = []
    for num in pyplot.get_fignums():
        figure = pyplot.figure(num)
        buffer = io.BytesIO()
        figure.savefig(buffer, format="png", bbox_inches="tight")
        outputs.append(
            {
                "output_type": "display_data",
                "data": {
                    "image/png": b64(buffer.getvalue()),
                    "text/plain": repr(figure),
                },
                "metadata": {},
            }
        )
    pyplot.close("all")
    return outputs


def split_last_expression(code):
    """Split into (statements, trailing expression), as `last_expr` display."""
    module = ast.parse(code, FILENAME, "exec")
    expression = None
    if module.body and isinstance(module.body[-1], ast.Expr):
        expression = ast.Expression(module.body.pop().value)
    return module, expression


def format_exception(error):
    # Drop this module's frame so the traceback starts inside the cell.
    tb = error.__traceback__.tb_next if error.__traceback__ else None
    lines = traceback.format_exception(type(error), error, tb)
    return {
        "output_type": "error",
        "ename": type(error).__name__,
        "evalue": str(error),
        "traceback": "".join(lines).rstrip("\n").split("\n"),
    }


def run(code):
    outputs = []
    stdout, stderr = io.StringIO(), io.StringIO()
    namespace = {"__name__": "__main__", "__builtins__": __builtins__}
    error = None
    result = None

    try:
        module, expression = split_last_expression(code)
    except SyntaxError as syntax_error:
        return [format_exception(syntax_error)]

    with redirect_stdout(stdout), redirect_stderr(stderr):
        try:
            exec(compile(module, FILENAME, "exec"), namespace)
            if expression is not None:
                result = eval(compile(expression, FILENAME, "eval"), namespace)
        except BaseException as caught:  # SystemExit is an output too
            error = caught

    for name, stream in [("stdout", stdout), ("stderr", stderr)]:
        if text := stream.getvalue():
            outputs.append({"output_type": "stream", "name": name, "text": text})
    if result is not None:
        outputs.append(mime_bundle(result))
    outputs.extend(figures())
    if error is not None:
        outputs.append(format_exception(error))
    return outputs


json.dump(run(sys.stdin.read()), report)
