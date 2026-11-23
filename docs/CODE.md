# Code: where it is, how to build, test, take shots, measure

## Layout

```
Cargo.toml              workspace; the shared crates pinned by rev (b4b21824, as mdedit pins them)
crates/hub-core/src     catalog model manifest state detect check transport install   (no window)
bins/hub/src            vm view grid panes ui   draw a Vm        demo shot   --shot and its states
                        glue work app cli trace the window and the engine behind it
tools/                  write-hub-txt.ps1, make-samples.ps1, samples/<app>/hub.txt
tests/                  shots.sh, shots-check.sh
.github/workflows/ci.yml
```

The shared crates are git dependencies on `Kaan88/markdown-shared-rs`. To move one, change the one
`rev` in the workspace `Cargo.toml` (all four crates together) on purpose, after running both
mdedit's and mdview's `tests/shots.sh` before and after the change in the shared repository.

## Build, on this PC

Keep the build away from the other checkouts and keep temp files inside it:

```bash
export CARGO_TARGET_DIR=C:/Users/kaank/Desktop/hub-wt/target
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
export HUB_FETCH_ROOT=C:/Users/kaank/Desktop/hub-wt/scratch/github
$CARGO_TARGET_DIR/release/hub.exe --behind --no-session --demo main --exit-after-ms 3000 --trace
```

`--demo NAME` fills the window with a made-up Vm; `--exit-after-ms` ends a test run. The release
exe has no `test-hooks`, so `HUB_FETCH_ROOT` means nothing in a release. A release exe started
without `HUB_FETCH_ROOT` and without `--demo` reaches the real github.com: do not start one so
while testing.

## Shots

```bash
hub --shot out.png --demo main --view list --dark --size 1280x800 --scale 1.0
bash tests/shots.sh target/shots/a        # states x 3 views x 2 themes x 2 sizes = 120 PNGs, ~15 s
bash tests/shots.sh target/shots/b        # twice: every PNG must be byte-identical
bash tests/shots-check.sh target/shots/a  # all present, real PNGs, the size their name says
```

File names are `<state>-<view>-<theme>-<W>x<H>.png`. `shots.sh` stops when `demo.rs` has a state
it does not list. A new visible state: add it to `demo.rs` (`NAMES` and `build`), to `DEMOS` in
`shots.sh`, and look at its shots in both themes. No pixel comparison in CI (fonts differ).

## The check a Rust crate can run on the samples

`tools/samples/<app>/hub.txt` are written by `tools/make-samples.ps1`; they must parse. No test
exists yet (that is source, not mine to write here). From a scratch crate with a path dependency
on `hub-core` this was run on 2026-11-22 and all four parsed and round-tripped:

```rust
for app in hub_core::catalog::CATALOG.iter() {
    let bytes = std::fs::read(format!("tools/samples/{}/hub.txt", app.id)).unwrap();
    let m = hub_core::manifest::parse(&bytes, app).unwrap();
    assert_eq!(hub_core::manifest::parse(hub_core::manifest::write(&m).as_bytes(), app).unwrap(), m);
}
```

As an integration test, `crates/hub-core/tests/samples.rs` with the same loop and the folder
`concat!(env!("CARGO_MANIFEST_DIR"), "/../../tools/samples/")`.

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
zero. If you did not measure, say so and say why the change cannot move them. Budgets: first
frame under 100 ms, private memory under 30 MiB, no CPU at idle, nothing heavy on launch.
