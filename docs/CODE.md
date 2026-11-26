# Code: where it is, how to build, test, take shots, measure

## Layout

```
Cargo.toml              workspace; the four shared crates pinned at 5665039d08d1e97964685ac3ed6e050b69bac467
crates/hub-core/src     catalog model manifest state detect check transport install   (no window)
bins/hub/src            vm view grid panes ui   draw a Vm        demo shot   --shot and its states
                         glue work app cli trace the window and the engine behind it; motion popup
tools/                  write-hub-txt.ps1, make-samples.ps1, samples/<app>/hub.txt
tests/                  shots.sh, shots-check.sh
.github/workflows/ci.yml
```

The shared crates are git dependencies on `Kaan88/markdown-shared-rs`. Move all four `rev`s in
`Cargo.toml` together. A change to shared itself calls for mdedit's and mdview's shot checks
there too; a Hub-only pin move does not edit the shared checkout.

## Build, on this PC

Keep the build away from the other checkouts and keep temp files inside it:

```bash
export CARGO_TARGET_DIR=C:/Users/kaank/Desktop/hub-wt/target-shared
mkdir -p "$CARGO_TARGET_DIR/tmp"
export TEMP=$CARGO_TARGET_DIR/tmp TMP=$CARGO_TARGET_DIR/tmp
cargo build --release -p hub
cargo test --release --workspace
cargo fmt --all --check
cargo clippy --release --workspace --all-targets -- -D warnings
cargo clippy --release -p hub --features test-hooks --all-targets -- -D warnings
```

## Never on the owner's PC state (CLAUDE.md rules 4 and 5)

Every run of the `hub` exe, by hand or from a test, sets all of these, and opens its window with
`--behind`:

| Variable | Means | Needs |
|---|---|---|
| `HUB_DATA` | Hub's own folder (`installed.ini`, `settings.ini`, `checks.ini`, `latest\`, `cache\`); a scratch folder | always |
| `HUB_PROGRAMS`, `HUB_PROGRAM_FILES` | stand-ins for `%LOCALAPPDATA%\Programs` and Program Files (`;` separated) | a fake programs root |
| `HUB_FETCH_ROOT` | a folder standing in for github.com: `https://github.com/X/Y` reads `<root>/X/Y` | a build with `--features test-hooks` (`cargo build --release -p hub --features test-hooks`) |
| `HUB_FETCH_LOG` | every URL asked for is appended to this file | with `HUB_FETCH_ROOT` |
| `HUB_FIRST_CHECK_MS` | the first check's delay, instead of 2 minutes | a test |

```bash
export HUB_DATA=C:/Users/kaank/Desktop/hub-wt/scratch/data
export HUB_PROGRAMS=C:/Users/kaank/Desktop/hub-wt/scratch/programs
export HUB_PROGRAM_FILES=C:/Users/kaank/Desktop/hub-wt/scratch/program-files
export HUB_FETCH_ROOT=C:/Users/kaank/Desktop/hub-wt/scratch/github
cargo build --release -p hub --features test-hooks
$CARGO_TARGET_DIR/release/hub.exe --behind --no-session --demo main --exit-after-ms 3000 --trace
```

`--demo NAME` fills the window with a made-up Vm; `--exit-after-ms` ends a test run. The release
exe has no `test-hooks`, so `HUB_FETCH_ROOT` means nothing in a release. A release exe started
without `HUB_FETCH_ROOT` and without `--demo` reaches the real github.com: do not start one so
while testing.

## Shots

```bash
bash tests/shots.sh "$CARGO_TARGET_DIR/shots/a"
bash tests/shots.sh "$CARGO_TARGET_DIR/shots/b"
diff -qr "$CARGO_TARGET_DIR/shots/a" "$CARGO_TARGET_DIR/shots/b" # byte-identical
bash tests/shots-check.sh "$CARGO_TARGET_DIR/shots/a"
```

File names are `<state>-<view>-<theme>-<W>x<H>.png`. `shots.sh` stops when `demo.rs` has a state
it does not list. A new visible state: add it to `demo.rs` (`NAMES` and `build`), to `DEMOS` in
`shots.sh`, and look at its shots in both themes. Shots use `Policy::Never`: they show the
settled endpoints, never timed intermediate frames. The motion tests drive a `ManualClock`.
No cross-machine pixel comparison in CI (fonts differ).

## The samples are tested

`tools/samples/<app>/hub.txt` are written by `tools/make-samples.ps1`; they must parse.
`crates/hub-core/tests/samples.rs` reads the sample of every catalog app from
`concat!(env!("CARGO_MANIFEST_DIR"), "/../../tools/samples/")`, parses it with that app's catalog
entry and round-trips it through the writer, so a new catalog app needs a sample too.

## Writing a hub.txt

```powershell
pwsh tools/write-hub-txt.ps1 -App sprawl -Repo sprawl -Version 1.0.5 -Notes "Faster scans." `
  -Kind setup -Exe sprawl.exe -Uninstall Sprawl_is1 -Out dist/hub.txt `
  -Files @{ Name='Sprawl-Setup-1.0.5.exe'; Path='dist/Sprawl-Setup-1.0.5.exe'; Role='setup'; Silent='/VERYSILENT /SUPPRESSMSGBOXES /NORESTART'; Scope='machine' }
```

`Get-Help tools/write-hub-txt.ps1` lists the fields. Exit 1 and nothing written on anything the
parser would refuse.

## Measuring (rule 1)

`hub --trace` prints `hub:` lines: stage times, private memory and CPU time. The method, for
every change that could touch them: build release before and after, run
`hub --behind --demo main --exit-after-ms 5000 --trace` with the variables above, compare the
first-frame time and the private memory, and check that the CPU time over an idle stretch is
zero. Compare like-for-like test-hooks builds; `--trace` names when motion begins and settles.
If you did not measure, say so and say why the change cannot move them. Budgets: first frame
under 100 ms, private memory under 30 MiB, no CPU at idle, nothing heavy on launch.
