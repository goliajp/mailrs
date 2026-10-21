#!/usr/bin/env python3
"""Print `<lines> <path> <name> <line>` for every prod Rust fn longer than a limit.

Usage: fn-size.py LIMIT FILE...

A function is measured from the line holding its `fn` keyword to the line
holding its closing brace. Comments, string, raw-string and char literals
are blanked out before any brace is counted, so a `"{"` or a `'}'` cannot
shift a body's end. Test code is not prod code: items under `#[cfg(test)]`
(inline `mod tests { }`, or `mod x;` naming a file) and fns marked with a
`#[test]`-style attribute are skipped. A file whose first lines carry a
`CARVE-OUT:` or generated-code marker is skipped whole.
"""

import os
import re
import sys

MARKER = re.compile(r"carve-out:|codegen:|auto-generated|@generated|do not edit", re.I)
IDENT = re.compile(r"[A-Za-z0-9_]")


def blank(src):
    """Return src with comments and literals replaced by spaces, newlines kept."""
    out = []
    i, n = 0, len(src)

    def keep_newlines(s):
        return "".join(c if c == "\n" else " " for c in s)

    while i < n:
        c = src[i]
        if c == "/" and src.startswith("//", i):
            j = src.find("\n", i)
            j = n if j < 0 else j
            out.append(" " * (j - i))
            i = j
            continue
        if c == "/" and src.startswith("/*", i):
            depth, j = 1, i + 2
            while j < n and depth:
                if src.startswith("/*", j):
                    depth += 1
                    j += 2
                elif src.startswith("*/", j):
                    depth -= 1
                    j += 2
                else:
                    j += 1
            out.append(keep_newlines(src[i:j]))
            i = j
            continue
        prev_ident = i > 0 and IDENT.match(src[i - 1])
        if c in "rbc" and not prev_ident:
            m = re.compile(r"(?:br|cr|r)(#*)\"").match(src, i)
            if m:
                close = '"' + m.group(1)
                j = src.find(close, m.end())
                j = n if j < 0 else j + len(close)
                out.append(keep_newlines(src[i:j]))
                i = j
                continue
        if c == '"':
            j = i + 1
            while j < n and src[j] != '"':
                j += 2 if src[j] == "\\" else 1
            j += 1
            out.append(keep_newlines(src[i:j]))
            i = j
            continue
        if c == "'":
            # a char literal, or a lifetime / loop label
            if i + 1 < n and src[i + 1] == "\\":
                j = src.find("'", i + 2)
                if src.startswith("'", i + 2):
                    j = i + 3
                j = n if j < 0 else j + 1
                out.append(keep_newlines(src[i:j]))
                i = j
                continue
            if i + 2 < n and src[i + 2] == "'":
                out.append("   ")
                i += 3
                continue
        out.append(c)
        i += 1
    return "".join(out)


def match_brace(text, i):
    """Index just past the brace block opening at text[i] == '{'."""
    depth = 0
    for j in range(i, len(text)):
        if text[j] == "{":
            depth += 1
        elif text[j] == "}":
            depth -= 1
            if depth == 0:
                return j + 1
    return len(text)


def item_end(text, i):
    """End of the item starting at i: its first top-level `;` or brace block."""
    depth = 0
    for j in range(i, len(text)):
        c = text[j]
        if c in "([":
            depth += 1
        elif c in ")]":
            depth -= 1
        elif depth == 0 and c == ";":
            return j + 1
        elif depth == 0 and c == "{":
            return match_brace(text, j)
    return len(text)


ATTR = re.compile(r"#\[")
INNER_TEST_CFG = re.compile(r"#!\[\s*cfg\s*\(\s*test\s*\)\s*\]")
TEST_CFG = re.compile(r"#\[\s*cfg\s*\(\s*(?:test\s*\)|all\s*\(\s*test\b)")
TEST_ATTR = re.compile(r"#\[\s*(?:[A-Za-z_:]+::)?test\b")
MOD_DECL = re.compile(r"\s*(?:pub(?:\s*\([^)]*\))?\s+)?mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*;")
FN = re.compile(r"\bfn\s+([A-Za-z_][A-Za-z0-9_]*)")


def attr_end(text, i):
    depth = 0
    for j in range(i, len(text)):
        if text[j] == "[":
            depth += 1
        elif text[j] == "]":
            depth -= 1
            if depth == 0:
                return j + 1
    return len(text)


def test_spans(text):
    """Spans of items marked as test-only, and the `mod x;` names among them."""
    spans, mods = [], []
    for m in ATTR.finditer(text):
        a = m.start()
        if not (TEST_CFG.match(text, a) or TEST_ATTR.match(text, a)):
            continue
        j = attr_end(text, a)
        while True:
            k = len(text) - len(text[j:].lstrip())
            if text.startswith("#[", k):
                j = attr_end(text, k)
            else:
                j = k
                break
        d = MOD_DECL.match(text, j)
        if d and TEST_CFG.match(text, a):
            mods.append(d.group(1))
        spans.append((a, item_end(text, j)))
    return spans, mods


def functions(text):
    for m in FN.finditer(text):
        body = item_end(text, m.end())
        if text[body - 1] == "}":
            yield m.group(1), m.start(), body


def parse(path):
    """Blanked text, test spans and test-only module paths, or None to skip."""
    src = open(path, encoding="utf-8").read()
    if any(MARKER.search(line) for line in src.splitlines()[:5]):
        return None
    text = blank(src)
    if INNER_TEST_CFG.search(text):
        return None
    spans, mods = test_spans(text)
    stem = path[:-3]
    if os.path.basename(path) in ("lib.rs", "main.rs", "mod.rs"):
        stem = os.path.dirname(path)
    return text, spans, [os.path.join(stem, name) for name in mods]


def long_functions(path, text, spans, limit):
    for name, start, end in functions(text):
        if any(a <= start < b for a, b in spans):
            continue
        first = text.count("\n", 0, start) + 1
        last = text.count("\n", 0, end - 1) + 1
        if last - first + 1 > limit:
            yield last - first + 1, name, first


def main():
    limit = int(sys.argv[1])
    parsed = {path: parse(path) for path in sys.argv[2:]}
    # `#[cfg(test)] mod x;` makes x.rs, x/mod.rs and everything under x/ test code
    test_mods = {m for p in parsed.values() if p for m in p[2]}

    def is_test_module(path):
        stem = path[:-3]
        if stem.endswith("/mod"):
            stem = stem[:-4]
        return any(stem == m or stem.startswith(m + "/") for m in test_mods)

    for path, p in parsed.items():
        if p is None or is_test_module(path):
            continue
        for lines, name, first in long_functions(path, p[0], p[1], limit):
            print(f"{lines} {path} {name} {first}")


main()
