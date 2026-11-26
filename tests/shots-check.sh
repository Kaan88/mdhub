#!/usr/bin/env bash
# The smoke check of CI: after tests/shots.sh <dir>, every shot the script takes (the product of
# DEMOS, VIEWS, THEMES and SIZES in shots.sh) must exist, and every PNG in the folder must be a
# non-empty PNG of the size its name says. No pixel comparison: fonts differ between machines.
#   tests/shots-check.sh target/shots
set -euo pipefail
dir="${1:?usage: tests/shots-check.sh <output-dir>}"
root="$(cd "$(dirname "$0")/.." && pwd)"
word() { sed -n "s/^$1=\"\(.*\)\"\$/\1/p" "$root/tests/shots.sh" | head -1; }
demos="$(word DEMOS)" views="$(word VIEWS)" themes="$(word THEMES)" sizes="$(word SIZES)"
[ -n "$demos" ] && [ -n "$views" ] && [ -n "$themes" ] && [ -n "$sizes" ] || { echo "shots-check: cannot read the lists of shots.sh" >&2; exit 1; }
png() { [ "$(head -c 8 "$1" | od -An -tx1 | tr -d ' \n')" = "89504e470d0a1a0a" ]; }
# Width and height of a PNG: the IHDR, bytes 16 to 23, big-endian.
dims() { od -An -tu1 -j16 -N8 "$1" | awk '{ print $1*16777216+$2*65536+$3*256+$4 "x" $5*16777216+$6*65536+$7*256+$8 }'; }
bad=0 n=0
for demo in $demos; do vs="$views"; case "$demo" in notes*) vs="list" ;; row-menu) vs="list large" ;; esac; for view in $vs; do for theme in $themes; do for size in $sizes; do
    n=$((n + 1))
    f="$dir/$demo-$view-$theme-$size.png"
    if [ ! -s "$f" ]; then echo "missing or empty: $f" >&2; bad=$((bad + 1)); continue; fi
    png "$f" || { echo "not a PNG: $f" >&2; bad=$((bad + 1)); continue; }
    [ "$(dims "$f")" = "$size" ] || { echo "wrong size $(dims "$f"), wanted $size: $f" >&2; bad=$((bad + 1)); }
done; done; done; done
for f in "$dir"/*.png; do
    [ -e "$f" ] || continue
    [ -s "$f" ] && png "$f" || { echo "empty or not a PNG: $f" >&2; bad=$((bad + 1)); }
done
echo "shots-check: $n shots wanted, $bad bad, $(find "$dir" -maxdepth 1 -name '*.png' | wc -l) PNGs in $dir"
[ "$bad" = 0 ]
