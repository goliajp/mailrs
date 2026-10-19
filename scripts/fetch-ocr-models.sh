#!/bin/sh
# Download the OCR models listed in crates/ocr/models.tsv into a directory
# (default target/ocr-models), checking each against its sha256. A file
# already there with the right hash is kept.
#
#   scripts/fetch-ocr-models.sh [DIR] [MANIFEST]
set -eu
here=$(cd "$(dirname "$0")/.." && pwd)
dir=${1:-$here/target/ocr-models}
manifest=${2:-$here/crates/ocr/models.tsv}
mkdir -p "$dir"

sha() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d' ' -f1
    else
        shasum -a 256 "$1" | cut -d' ' -f1
    fi
}

grep -v '^#' "$manifest" | while IFS="$(printf '\t')" read -r name url want; do
    [ -n "$name" ] || continue
    if [ -f "$dir/$name" ] && [ "$(sha "$dir/$name")" = "$want" ]; then
        continue
    fi
    curl -fsSL --retry 3 -o "$dir/$name.part" "$url"
    got=$(sha "$dir/$name.part")
    if [ "$got" != "$want" ]; then
        rm -f "$dir/$name.part"
        echo "fetch-ocr-models: $name has sha256 $got, expected $want" >&2
        exit 1
    fi
    mv "$dir/$name.part" "$dir/$name"
    echo "fetch-ocr-models: $name"
done
