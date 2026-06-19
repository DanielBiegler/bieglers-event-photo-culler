#!/usr/bin/env bash
# copy-keeper-raws.sh — copy the RAWs named in a keepers list.
#
# Uses the `keepers.txt` produced by the export (one stem per line, e.g.
# `IMG_1234`) to copy the matching RAW files out of a source tree (e.g. the
# mounted SD card) into a destination directory.
#
# Usage: ./copy-keeper-raws.sh <keepers.txt> <raw-src-dir> <dest-dir> [ext]
#   ext defaults to CR2 (matched case-insensitively).
#
# Example: ./copy-keeper-raws.sh ./export/keepers.txt /media/sd/DCIM ./raws
#
# Tip: run it a second time with `ext=xmp` against the export folder to copy the
# rating sidecars next to the RAWs so darktable/Lightroom picks up the stars.

set -euo pipefail

list="${1:?need keepers.txt}"
src="${2:?need RAW source dir}"
dest="${3:?need destination dir}"
ext="${4:-CR2}"

mkdir -p "$dest"

# Total non-blank stems, for the (N/M) progress prefix.
total="$(grep -cve '^[[:space:]]*$' "$list" || true)"

n=0 found=0 missing=0
while IFS= read -r stem || [[ -n "$stem" ]]; do
    [[ -z "$stem" ]] && continue                 # skip blank lines
    n=$((n + 1))
    prog="($n/$total)"

    # Find the RAW for this stem, case-insensitive on the extension
    # (CR2/cr2), recursing into the source tree.
    match="$(find "$src" -type f -iname "${stem}.${ext}" -print -quit)"

    if [[ -n "$match" ]]; then
        base="$(basename "$match")"
        # Skip if already copied (portable "no-clobber"; avoids `cp -n`, whose
        # behavior GNU coreutils 9+ warns is changing).
        if [[ -e "$dest/$base" ]]; then
            printf '%s skip (exists): %s\n' "$prog" "$base"
        else
            cp -- "$match" "$dest/"
            printf '%s copied: %s\n' "$prog" "$base"
        fi
        found=$((found + 1))
    else
        printf '%s MISSING: %s.%s\n' "$prog" "$stem" "$ext" >&2
        missing=$((missing + 1))
    fi
done < "$list"

printf 'Copied %d, missing %d.\n' "$found" "$missing"
