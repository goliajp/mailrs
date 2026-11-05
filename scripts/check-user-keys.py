#!/usr/bin/env python3
"""Every key family an account owns is in the purge registry.

Deleting an account purges, after its retention period, what the registry
(`crates/core-sidestate/src/families/account_purge/registry.rs`) lists. A
family left out of it outlives the account and is handed to whoever takes
the address next. Two rules:

1. every shared family `check-shared-keys.sh` lists (given on stdin, one
   ERE per line) names a registry entry: owned and purged, or kept with
   its reason;
2. every key built in `crates/` as `format!("<prefix>:{user}...")`, with
   the first placeholder an address (`user`, `address`, `owner`, ...),
   matches a registry entry.

`--self-test` runs both rules on fixtures that must fail and pass.
"""

import glob
import re
import sys

REGISTRY = "crates/core-sidestate/src/families/account_purge/registry.rs"
USERISH = r"(?:user|address|addr|owner|account|email|member|recipient|username)"
# key-shaped format strings that are not keys of their own
IGNORE = {
    "a:": "a scope inside an auth-guard `ag:` key, covered by `ag:*{user}*`",
    "sa:": "a scope inside an auth-guard `ag:` key, covered by `ag:*{user}*`",
    "mailto:": "a URL",
}
FORMAT = re.compile(r'format!\(\s*"([^"]*)"')
USER_KEY = re.compile(
    r"([A-Za-z_][A-Za-z0-9_.-]*:[A-Za-z0-9_:|.-]*)\{" + USERISH + r"\b[^}]*\}(.*)"
)


def registry_names(text):
    owned = re.findall(r'\b(?:net|mail)\("([^"]+)"', text)
    kept = re.findall(r'Kept \{ name: "([^"]+)"', text)
    return owned + kept


def name_regex(name):
    """`{user}` stands for the address, `*` and `{gid}` for anything; a
    note in parentheses after a space is not part of the key."""
    name = name.split(" (")[0]
    out = ""
    for part in re.split(r"(\{user\}|\{gid\}|\*)", name):
        if part == "{user}":
            out += r"\{user\}"
        elif part in ("*", "{gid}"):
            out += ".*"
        else:
            out += re.escape(part)
    return re.compile(out)


def unclassified_families(families, names):
    return [f for f in families if not any(re.match(f, n) for n in names)]


def user_keys(path, text):
    for m in FORMAT.finditer(text):
        k = USER_KEY.match(m.group(1))
        if not k:
            continue
        prefix, rest = k.group(1), k.group(2)
        key = prefix + "{user}" + re.sub(r"\{[^}]*\}", "x", rest)
        line = text.count("\n", 0, m.start()) + 1
        yield f"{path}:{line}", prefix, key


def unregistered_keys(sources, names):
    regexes = [name_regex(n) for n in names]
    out = []
    for path, text in sources.items():
        for where, prefix, key in user_keys(path, text):
            if prefix in IGNORE:
                continue
            if not any(r.fullmatch(key) for r in regexes):
                out.append(f"{where}: {key}")
    return out


def self_test():
    registry = 'net("drafts:{user}", x),\nmail("L:m:{user}|*", y),\nKept { name: "gl:*|{user}" }'
    names = registry_names(registry)
    fails = []
    if names != ["drafts:{user}", "L:m:{user}|*", "gl:*|{user}"]:
        fails.append(f"registry names read as {names}")
    src = {
        "a.rs": 'format!("drafts:{user}")\nformat!("L:m:{user}|{mid}")\n'
        'format!("mailto:{email}")\nformat!("newfam:{address}:{id}")\n'
    }
    got = unregistered_keys(src, names)
    if got != ["a.rs:4: newfam:{user}:x"]:
        fails.append(f"a new per-user family: wanted one finding, got {got}")
    if unclassified_families(["drafts:", "gl:", "newglobal:"], names) != ["newglobal:"]:
        fails.append("an unclassified shared family was not reported")
    return fails


def main():
    fails = self_test()
    if fails:
        print("!! check-user-keys self-test failed:")
        for f in fails:
            print("   " + f)
        return 1
    if sys.argv[1:] == ["--self-test"]:
        print("check-user-keys self-test OK")
        return 0
    names = registry_names(open(REGISTRY).read())
    families = [l.strip() for l in sys.stdin if l.strip()]
    sources = {
        p: open(p).read()
        for p in glob.glob("crates/**/*.rs", recursive=True)
        if p != REGISTRY
    }
    status = 0
    for f in unclassified_families(families, names):
        print(f'!! the shared family "{f}" is in no entry of {REGISTRY}:')
        print("   add it as owned (purged with the account) or kept, with why")
        status = 1
    missing = unregistered_keys(sources, names)
    if missing:
        print(f"!! keys built from an address that no entry of {REGISTRY} names;")
        print("   a deleted account's purge would leave them:")
        for m in missing:
            print("    " + m)
        status = 1
    if status == 0:
        print(f"user keys: {len(names)} registry entries cover every per-user key family")
    return status


if __name__ == "__main__":
    sys.exit(main())
