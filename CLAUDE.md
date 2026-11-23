# Hub

A launcher and updater for the owner's apps: mdedit, mdview, Sprawl, Inbound, Purge, Barbican. A window that lists
them, shows which are installed and which have an update, installs and updates them, and opens
them. Rust, winit + softbuffer, every control drawn by hand, built on the shared crates
(`Kaan88/markdown-shared-rs`) that mdedit and mdview use. Windows first.

Its sisters are `Kaan88/markdown-editor-rs` (mdedit; on this PC
`C:\Users\kaank\Desktop\markdown-editor-ai`), `Kaan88/markdown-viewer-rs` (mdview; `..\mv`) and
`Kaan88/markdown-shared-rs` (`..\markdown-shared-rs`). The plan Hub is built from is
`docs/launcher-plan.md` in mdedit's repository (the branch `claude/launcher-build-1f1b4f`).

## Read in this order

1. This file. It is short on purpose and it is kept true.
2. `docs/STATE.md`: what exists, what does not, what is known to be wrong. Trust it over anything older.
3. `docs/DECISIONS.md`: what was decided, when, why, what would change it. Look there before you
   decide anything. A decision is never re-made silently: add an entry that names the one it replaces.
4. Then what the task needs:
   - `docs/INTERFACES.md`: the contract between the modules (as built).
   - `docs/DESIGN.md`: how each module works, one section per module.
   - `docs/CODE.md`: where the code is, how to build, test, take shots, measure.
   - `docs/COMPLIANCE.md`: what an app must do to work with Hub (the contract other repositories'
     agents read; keep it true when the parser, the catalog or the install rules change).
   - `docs/APP-CHANGES.md`: what each app must change for Hub (proposals, not applied).
   - `docs/RELEASE.md`: how a release will be cut. None has been.

## What it is, and what it refuses to be

- Light, before anything else. It is a program people leave in the tray, so what it costs at
  rest counts: no CPU at idle, nothing polling, the window exists only while it is open.
- It talks to `github.com` and to nobody else: `hub.txt` of each app's latest release, and the
  files that manifest names. No API, no token, no account, no telemetry. Every URL in a manifest
  must lie under the app's own repository's releases.
- It never ends a process (it asks an app to close and waits), is never elevated (a machine-wide
  setup is run through Windows' own prompt, at the owner's click), and never touches an app's
  settings, session or data.
- Not an app store, not a package manager: a fixed catalog of six apps compiled in; no adding a
  repository by URL; no zip unpacking, no delta updates, no signatures (SHA-256 over TLS).
- Scope: mdedit, mdview, Sprawl, Inbound, Purge (D-030), Barbican (D-031).

## The rules

1. **Budgets are measured, before and after.** First frame under 100 ms, private memory under
   30 MiB, no CPU at idle, nothing heavy on the launch path (the first check is scheduled, never
   run at start). `hub --trace` prints stage times, memory and CPU. If you did not measure, say so,
   and say why the change cannot move them.
2. **A test per behaviour, and it fails with its fix reverted.** Never weaken, skip or ignore a test.
3. **fmt and clippy are clean before a push**: `cargo fmt --all --check` and
   `cargo clippy --release --workspace --all-targets -- -D warnings`.
4. **No real install, ever, on this PC.** Hub's tests and runs never install, update, uninstall or
   alter a real app of the owner's (mdedit, mdview, Sprawl, Inbound, Purge, Barbican, anything). Every run of the
   `hub` exe, by hand or by test, uses `HUB_DATA` naming a scratch folder, a fake programs root
   (`HUB_PROGRAMS`, `HUB_PROGRAM_FILES`), and `HUB_FETCH_ROOT` (a `--features test-hooks` build)
   in place of the network. Never the real `%LOCALAPPDATA%\Hub`, never the owner's mdedit
   settings. Installers in tests are tiny programs the test builds.
5. **Every test window starts with `--behind`**, never in front of what the owner is doing.
6. **No new dependency without stopping and asking**, saying so plainly, and pricing it: what it
   adds to `Cargo.lock`, to the exe, to the launch path. None has been added (D-006).
7. **Shared first.** Code a second app could use is written in `markdown-shared-rs`; a change there
   must not change what an existing caller draws or does unless it opts in. Hub's own pieces
   (tray, grid) are written to be lifted there later (D-011 of the plan).
8. **The owner approves the interface by looking at it.** A shot shows it, it does not prove it
   right. Every new visible state goes into the demo list (`bins/hub/src/demo.rs`) and into
   `tests/shots.sh` (the script stops until it is there); look at each in both themes and say
   which shot shows what.
9. **Panic-free on anything from outside.** The release build aborts on a panic. A manifest, a
   registry value, a file name, an installer's exit code: parsed with no path that can panic.
   Nothing downloaded is placed or run before its size and SHA-256 match.
10. **Never publish without the owner.** No push, tag, release, GitHub repository, upload to
    another project's release. A release is cut by pushing a tag after `ci` is green, when the
    owner says so (`docs/RELEASE.md`). None exists.
11. **Other agents may be working in this checkout.** `git status` before you edit; stage only
    your own files by path (never `git add -A` or `commit -a`); changes you did not make are
    someone's work.
12. **Commit as phases complete.** Subjects read `hub: ...` or `docs: ...` and say what changed in
    plain words; the message ends with a `Co-Authored-By` line naming the model.
13. **Report plainly**: what was run, what was not, what was not verified. Nothing counts as
    working on Windows until it has run on Windows.

## Keeping these documents true

- `docs/STATE.md` is **rewritten**, never appended to; under 150 lines.
- `docs/DECISIONS.md` is **appended to**, never edited; a changed decision gets a new entry.
- When a piece of work is done: update STATE.md, add its decisions, correct its `DESIGN.md` section.
- No dated story of a session anywhere: `git log` is the story.
- This file changes only when a rule or a pointer changes.
