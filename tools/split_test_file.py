#!/usr/bin/env python3
"""Split an oversized Rust integration-test file along its `// ---` section rules.

Encodes the recipe recorded in bloomery's docs/CARRIED-DEBT.md (slice D),
including the four traps found the hard way:

  1. Cut on the SECTION RULE, never on the first item's signature line -- a
     test's #[test] attribute and doc comment sit above its `fn`, so cutting
     at the signature silently strips both and the test stops running while
     still looking present.
  2. Grab multi-line `use` blocks whole -- keeping only lines starting with
     `use ` truncates a wrapped braced import into an unclosed delimiter.
  3. Never emit a source line twice -- an item's computed span can swallow its
     neighbour when a preceding const carries a multi-line literal. The guard
     turns that into a loud missing symbol at build time instead of a
     duplicate definition.
  4. Close the shared set transitively -- a lifted helper's own callees must
     come with it, or the shared module will not compile.

Usage is programmatic: import and call `split(...)`. Verification is not
optional -- `split` refuses to write if the test-name set would change.
"""

import pathlib
import re
import subprocess

ITEM_RE = re.compile(r"(?:pub )?(?:fn|const|type|struct|enum) ([A-Za-z_][A-Za-z_0-9]*)")
# trap 6: an `impl` block is not named by ITEM_RE but must travel with its
# type. `impl Foo` and `impl Trait for Foo` both bind to Foo, so the block is
# classified under the same name as the struct and lands in the same file.
IMPL_RE = re.compile(r"impl(?:<[^>]*>)? (?:[A-Za-z_][A-Za-z_0-9:<>, ]*? for )?([A-Za-z_][A-Za-z_0-9]*)")


def test_names(lines):
    """The set of #[test] function names in `lines`."""
    out = set()
    for k, line in enumerate(lines):
        if line.strip() == "#[test]":
            for m in range(k + 1, min(k + 8, len(lines))):
                hit = re.match(r"\s*fn ([a-z_0-9]+)", lines[m])
                if hit:
                    out.add(hit.group(1))
                    break
    return out


def section_rules(lines):
    """1-based line numbers of the `// ---` rules that OPEN each section."""
    rules = [k + 1 for k, l in enumerate(lines) if l.startswith("// ---")]
    return [rules[i] for i in range(0, len(rules), 2)]


def item_span(lines, i):
    """(start, end) 0-based inclusive for the item whose signature is at `i`,
    walking back over its doc comments and attributes (trap 1)."""
    a = i
    while a > 0 and (
        lines[a - 1].startswith("///")
        or lines[a - 1].startswith("#[")
        or (lines[a - 1].lstrip().startswith("//") and not lines[a - 1].startswith("// ---"))
    ):
        a -= 1
    if "{" not in lines[i] and lines[i].rstrip().endswith(";"):
        return a, i
    depth, seen, j = 0, False, i
    while j < len(lines):
        depth += lines[j].count("{") - lines[j].count("}")
        if "{" in lines[j]:
            seen = True
        if seen and depth <= 0:
            break
        j += 1
    return a, min(j, len(lines) - 1)


def import_block(lines):
    """The file's whole import block, multi-line `use` items intact (trap 2)."""
    idx = [k for k, l in enumerate(lines) if l.startswith("use ")]
    if not idx:
        return ""
    first = idx[0]
    last = first
    k = first
    while k < len(lines):
        if lines[k].startswith("use ") or lines[k].startswith("    ") or lines[k].startswith("}"):
            if lines[k].rstrip().endswith(";"):
                last = k
            k += 1
            continue
        if lines[k].strip() == "":
            k += 1
            continue
        break
    return "\n".join(lines[first : last + 1])


def split(path, targets, docs, common_path, common_doc, common_extra_imports=""):
    """`targets` is [(key, start_line, end_line)] 1-based inclusive, cut on
    section rules. `docs` maps key -> (out_filename, module_doc)."""
    p = pathlib.Path(path)
    src = p.read_text().split("\n")
    baseline = test_names(src)
    imports = import_block(src)

    def tgt(ln):
        for key, a, b in targets:
            if a <= ln <= b:
                return key
        return None

    items = [
        (m.group(1), k, k > 0 and src[k - 1].strip() == "#[test]")
        for k, l in enumerate(src)
        if (m := ITEM_RE.match(l))
    ]
    helpers = [(n, i) for n, i, is_test in items if not is_test]
    impls = [(m.group(1), k) for k, l in enumerate(src) if (m := IMPL_RE.match(l))]

    def users(name, defline):
        seen = set()
        for k, l in enumerate(src):
            if k == defline or l.strip().startswith("//"):
                continue
            if re.search(rf"\b{name}\b", l):
                seen.add(tgt(k + 1))
        seen.discard(None)
        return seen

    shared_names = set()
    for name, i in helpers:
        u = users(name, i)
        if len(u) >= 2 or (not u and i + 1 < targets[0][1]):
            shared_names.add(name)

    # trap 4: close transitively over what the shared helpers themselves call
    changed = True
    while changed:
        changed = False
        body = "\n".join(
            "\n".join(src[a : b + 1])
            for n, i in helpers
            if n in shared_names
            for a, b in [item_span(src, i)]
        )
        for name, i in helpers:
            if name in shared_names:
                continue
            if re.search(rf"\b{name}\b", body):
                shared_names.add(name)
                changed = True

    # trap 5: an item used by NO target may still be called by one that IS
    # emitted -- a fixture that only other fixtures use. Dropping it silently
    # produces a missing symbol in whichever file inherited its caller, so
    # place it with that caller (or share it if callers disagree).
    placement = {}
    for name, i in helpers:
        u = users(name, i)
        placement[name] = "shared" if name in shared_names else (next(iter(u)) if len(u) == 1 else None)
    changed = True
    while changed:
        changed = False
        for name, i in helpers:
            if placement[name] is not None:
                continue
            # scan impl bodies too: a fixture called only from `impl Foo`
            # is otherwise invisible here and gets dropped (found on swap_test).
            scan = helpers + [(n2, i2) for n2, i2 in impls if placement.get(n2) is not None]
            callers = {
                placement.get(n2)
                for n2, i2 in scan
                if n2 != name
                and placement.get(n2) is not None
                and re.search(rf"\b{name}\b", "\n".join(src[slice(*item_span(src, i2))]))
            }
            callers.discard(None)
            if not callers:
                continue
            placement[name] = "shared" if len(callers) > 1 else next(iter(callers))
            changed = True

    # an impl block inherits its type's placement, so the two never separate
    for name, i in impls:
        if placement.get(name) is not None:
            helpers.append((name, i))

    consumed, shared_spans, home = set(), [], {}
    for name, i in helpers:
        a, b = item_span(src, i)
        if any(x in consumed for x in range(a, b + 1)):  # trap 3
            continue
        where = placement[name]
        if where == "shared":
            shared_spans.append((a, b, name))
        elif where is not None:
            home.setdefault(where, []).append((a, b, name))
        else:
            continue
        consumed.update(range(a, b + 1))

    shared_spans.sort()
    names = sorted({n for _, _, n in shared_spans})  # a struct and its impl share a name
    body = "\n\n".join("\n".join(src[a : b + 1]) for a, b, _ in shared_spans)
    for n in names:
        body = re.sub(rf"(?m)^(fn|const|type|struct|enum) {n}\b", rf"pub \1 {n}", body)
    # inherent methods on a shared type must be public too, or the callers that
    # moved to another file cannot reach them (trap 7)
    body = re.sub(r"(?m)^(    )fn ([a-z_][a-z_0-9]*)\(", r"\1pub fn \2(", body)

    written = {}
    written[common_path] = (
        common_doc + "\n\n" + imports + common_extra_imports + "\n\n" + body + "\n"
    )
    for key, a, b in targets:
        name, doc = docs[key]
        kept = [src[k] for k in range(a - 1, b) if k not in consumed]
        extra = "\n\n".join("\n".join(src[x : y + 1]) for x, y, _ in sorted(home.get(key, [])))
        parts = [doc, "", "mod common;", "", imports, ""]
        if names:
            mod = pathlib.Path(common_path).stem
            parts += ["use common::" + mod + "::{" + ", ".join(names) + "};", ""]
        if extra:
            parts += [extra, ""]
        parts.append("\n".join(kept))
        written[str(pathlib.Path(path).parent / name)] = "\n".join(parts).rstrip() + "\n"

    # verification is not optional
    after = set()
    for f, text in written.items():
        if f != common_path:
            after |= test_names(text.split("\n"))
    if after != baseline:
        raise SystemExit(
            f"REFUSING TO WRITE: test-name set changed.\n"
            f"  lost:  {sorted(baseline - after)}\n"
            f"  added: {sorted(after - baseline)}"
        )

    for f, text in written.items():
        pathlib.Path(f).write_text(text)
    return baseline, {f: len(t.split("\n")) for f, t in written.items()}
