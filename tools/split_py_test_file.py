#!/usr/bin/env python3
"""Split an oversized Python `unittest` test module by TestCase class.

The Python counterpart of `tools/split_test_file.py`, written for slice D's
last three ceiling offenders. It is simpler than the Rust one for a good
reason: Python has a real parser, so item spans come from `ast` rather than
from brace counting and doc-comment backtracking, and three of that tool's
traps cannot occur here.

The one property both share is the important one: **it refuses to write if the
set of test identities would change.** A `unittest` test is identified as
`ClassName.method_name`, so a class silently dropped from every output file --
the Python equivalent of a stripped `#[test]` -- is caught before anything is
written rather than noticed later as a smaller number.

Module-level helpers and constants move to a shared module whose name starts
with `_`, so pytest does not collect it as a test file.
"""

import ast
import pathlib


def test_identities(source: str) -> set[str]:
    """`ClassName.method` for every unittest test, plus bare `test_*` functions."""
    tree = ast.parse(source)
    found = set()
    for node in tree.body:
        if isinstance(node, ast.ClassDef):
            for sub in node.body:
                if isinstance(sub, ast.FunctionDef) and sub.name.startswith("test"):
                    found.add(f"{node.name}.{sub.name}")
        elif isinstance(node, ast.FunctionDef) and node.name.startswith("test"):
            found.add(node.name)
    return found


def _span(source_lines: list[str], node: ast.AST) -> tuple[int, int]:
    """1-based inclusive line span, including any decorators and the comment
    block directly above the definition."""
    start = min([node.lineno] + [d.lineno for d in getattr(node, "decorator_list", [])])
    i = start - 1
    while i > 0:
        prev = source_lines[i - 1].strip()
        if prev.startswith("#") or prev == "":
            # keep a comment block, but stop at the section rule above it
            if prev.startswith("# ---"):
                break
            i -= 1
            continue
        break
    return i + 1, node.end_lineno


def split(path: str, groups: dict[str, tuple[str, list[str], str]], shared_path: str,
          shared_doc: str) -> tuple[set[str], dict[str, int]]:
    """`groups` maps key -> (out_filename, [class names], module docstring)."""
    p = pathlib.Path(path)
    source = p.read_text()
    lines = source.split("\n")
    baseline = test_identities(source)
    tree = ast.parse(source)

    classes = {n.name: n for n in tree.body if isinstance(n, ast.ClassDef)}
    assigned = {c for _, names, _ in groups.values() for c in names}
    missing = set(classes) - assigned
    if missing:
        raise SystemExit(f"REFUSING: classes assigned to no output file: {sorted(missing)}")

    # everything that is not a class is shared fixture material
    header_end = min(
        (n.lineno for n in tree.body if isinstance(n, ast.ClassDef)), default=len(lines)
    )
    imports_and_helpers = []
    for idx, node in enumerate(tree.body):
        if isinstance(node, ast.ClassDef):
            continue
        # skip the original module docstring: the shared module gets its own,
        # and a second string literal in front of `from __future__ import ...`
        # is a SyntaxError, not a stylistic wrinkle
        if idx == 0 and isinstance(node, ast.Expr) and isinstance(node.value, ast.Constant) \
                and isinstance(node.value.value, str):
            continue
        a, b = _span(lines, node)
        imports_and_helpers.append((a, b))
    imports_and_helpers.sort()
    shared_body = "\n".join(
        "\n".join(lines[a - 1 : b]) for a, b in imports_and_helpers
    )

    # names the shared module exports, for the `from ... import` each file needs
    exported = []
    for node in tree.body:
        if isinstance(node, ast.FunctionDef):
            exported.append(node.name)
        elif isinstance(node, ast.Assign):
            for t in node.targets:
                if isinstance(t, ast.Name):
                    exported.append(t.id)

    # the original import statements, verbatim: a class body may reach for
    # `json`, `Path` or the module under test just as freely as for a helper
    original_imports = "\n".join(
        "\n".join(lines[n.lineno - 1 : n.end_lineno])
        for n in tree.body
        if isinstance(n, (ast.Import, ast.ImportFrom))
    )

    shared_mod = pathlib.Path(shared_path).stem
    pkg = "tools.memory_battery.tests"
    written = {shared_path: shared_doc + "\n\n" + shared_body.strip() + "\n"}

    for key, (fname, names, doc) in groups.items():
        parts = []
        for n in names:
            first, last = _span(lines, classes[n])
            parts.append("\n".join(lines[first - 1 : last]))
        body = "\n\n".join(parts)
        imp = (
            f"from {pkg}.{shared_mod} import (  # noqa: F401\n"
            + "".join(f"    {n},\n" for n in sorted(set(exported)))
            + ")\n"
        )
        written[str(p.parent / fname)] = (
            doc + "\n\n" + original_imports + "\n\n" + imp + "\n\n" + body.strip() + "\n\n\n"
            'if __name__ == "__main__":\n    unittest.main()\n'
        )

    after = set()
    for f, text in written.items():
        if f != shared_path:
            after |= test_identities(text)
    if after != baseline:
        raise SystemExit(
            "REFUSING TO WRITE: test identities changed.\n"
            f"  lost:  {sorted(baseline - after)}\n"
            f"  added: {sorted(after - baseline)}"
        )

    for f, text in written.items():
        pathlib.Path(f).write_text(text)
    return baseline, {f: len(t.split("\n")) for f, t in written.items()}
