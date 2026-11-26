#!/usr/bin/env bash
# Renders Hub's screenshot set with --shot into the directory given as the only argument: every
# demo state (bins/hub/src/demo.rs) x list / large / small x light / dark x 1280x800 and 860x600.
# Run it twice into two directories and compare: every PNG must be byte-identical.
#
#   tests/shots.sh target/shots/a
#
# Needs a release build with test hooks. A shot draws a made-up Vm, with scratch roots for
# data, programs and fetches. File names: <state>-<view>-<theme>-<W>x<H>.png.
#
# A state added to demo.rs must be added to DEMOS below (the script stops until it is), so every
# new visible state gets its shots (CLAUDE.md rule 8).

set -euo pipefail

out="${1:?usage: tests/shots.sh <output-dir>}"
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
t="${CARGO_TARGET_DIR:-target}"
exe="$t/release/hub.exe"
[ -x "$exe" ] || exe="$t/release/hub"
[ -x "$exe" ] || { echo "shots: no release build at $t/release (cargo build --release -p hub)" >&2; exit 1; }

DEMOS="main first-run updating failed done noinfo running banner all claude-switch empty settings-general settings-updates settings-about settings-interval-menu row-menu notes notes-offer notes-loading notes-failed notes-nofile notes-long notes-bar-active hover-close hover-min hover-bar hover-primary pressed-primary hover-open hover-menu close-question close-question-yes close-question-no backdrop-mica backdrop-acrylic"
VIEWS="list large small"
THEMES="light dark"
SIZES="1280x800 860x600"

mkdir -p "$out"
export HUB_DATA="$out/.hub-data"
export HUB_PROGRAMS="$out/.programs" HUB_PROGRAM_FILES="$out/.program-files" HUB_FETCH_ROOT="$out/.github"
mkdir -p "$HUB_DATA" "$HUB_PROGRAMS" "$HUB_PROGRAM_FILES" "$HUB_FETCH_ROOT"

# The names demo.rs knows, from the program's own refusal of an unknown one ("try: a, b, c").
known="$("$exe" --behind --shot "$out/.probe.png" --demo __unknown__ 2>&1 >/dev/null | sed -n 's/.*try: //p' | tr -d '\r' | tr ',' ' ' | xargs -n1 | sort | tr '\n' ' ' || true)"
mine="$(printf '%s\n' $DEMOS | sort | tr '\n' ' ')"
rm -f "$out/.probe.png"
if [ "$known" != "$mine" ]; then
    echo "shots: demo.rs has [$known] but DEMOS has [$mine]; add the new state to DEMOS" >&2
    exit 1
fi

n=0
for demo in $DEMOS; do
    # The notes view takes the whole content area: the list view makes no difference to it.
    views="$VIEWS"
    case "$demo" in notes*) views="list" ;; row-menu) views="list large" ;; esac
    for view in $views; do
        for theme in $THEMES; do
            for size in $SIZES; do
                # The scale is pinned, so the system's display scale cannot change the pictures.
                "$exe" --behind --shot "$out/$demo-$view-$theme-$size.png" --demo "$demo" --view "$view" \
                    --"$theme" --size "$size" --scale 1.0
                n=$((n + 1))
            done
        done
    done
done
echo "shots: $n PNGs in $out"
