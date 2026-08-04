#!/usr/bin/env bash
# check-workspace-deps.sh — a crate that lives in this repo must be
# depended on by path.
#
# `mailrs-rfc5322 = "1"` names a crate that is right there in
# `crates/rfc5322`, and Cargo reads it as a different package: the one
# crates.io last published. Both then compile into the same binary.
# On 2026-08-03 all 23 such crates were duplicated that way, two of them
# at different versions — rfc5322 at 1.0.1 and 1.1.0, mta-sts at 1.0.0
# and 2.0.1 — so which behaviour a message got depended on which module
# handled it.
#
# It is also why editing one of those files changed nothing: the fix
# went into the path copy and the consumer compiled the published one.
#
# The house form is `{ path = "../x", version = "N" }`: builds from the
# tree here, still resolvable for anyone consuming the published crate.
set -euo pipefail
cd "$(dirname "$0")/.."

# name -> directory, for every crate in this workspace
declare_local() {
    for ct in crates/*/Cargo.toml; do
        name=$(sed -n 's/^name = "\(.*\)"$/\1/p' "$ct" | head -1)
        [ -n "$name" ] && echo "$name $(dirname "$ct")"
    done
}
locals=$(declare_local)

bad=0
for ct in crates/*/Cargo.toml; do
    consumer=$(dirname "$ct")
    # `mailrs-foo = "1"` / `mailrs-foo = { version = "1" }` with no path
    while IFS= read -r line; do
        dep=${line%% *}
        home=$(echo "$locals" | awk -v d="$dep" '$1==d {print $2}')
        [ -z "$home" ] && continue          # genuinely external
        echo "  $consumer/Cargo.toml: $dep is $home but is taken from crates.io"
        bad=$((bad + 1))
    # Anything whose value does not mention `path` at all — the key
    # order inside the table is free, so this asks what is missing
    # rather than what comes first.
    done < <(grep -E '^mailrs-[a-z0-9-]+ *=' "$ct" | grep -v 'path *=' | sed 's/ *=.*//;s/$/ /')
done

if [ "$bad" -gt 0 ]; then
    echo
    echo "$bad dependency(s) name a crate in this repo and resolve to the"
    echo "published copy instead. Both end up in the binary, and edits to"
    echo "the local one do nothing. Use the path form:"
    echo
    echo "    mailrs-foo = { path = \"../foo\", version = \"1\" }"
    exit 1
fi
echo "workspace deps OK — every local crate is depended on by path"
