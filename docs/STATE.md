# State

Rewritten, never appended to. What is true now; `git log` is the story.

## Released

Nothing. No release, no tag, no pushed repository, no GitHub repository, `ci` never run
(`docs/RELEASE.md`). The owner has not asked for a release.

## On `main`, committed

- `crates/hub-core`: catalog (four apps), model, `hub.txt` parser and writer plus the legacy
  stopgap (`manifest.rs`), Hub's own files (`state.rs`), detection (`detect.rs`), checking
  (`check.rs`), transport (`transport.rs`: `Net`, `FileRoot`), installing (`install.rs`: files
  and setup kinds, running apps asked to close, roll back of files installs, launch).
  Verified here, 2026-11-22: `cargo test --release -p hub-core`, 143 passed, 1 ignored.
- `bins/hub` drawing: `vm view grid panes ui demo shot`; `hub --shot` renders ten demo states in
  list, large and small views, both themes. `tests/shots.sh` renders all of them at 1280x800 and
  860x600 (120 PNGs, about 15 s, byte-identical on two runs); `tests/shots-check.sh` checks them.
  Looked at: `main-list-dark-1280x800` only; the other 119 were not looked at one by one.
- `tools/write-hub-txt.ps1`, `tools/make-samples.ps1`, `tools/samples/<app>/hub.txt` for the four
  apps (GitHub's real sizes and hashes of each project's latest release). The four samples parse
  and round-trip with `manifest::parse` (checked from a scratch crate outside the repository;
  there is no test in the repository for it yet).
- Docs: this file, `DECISIONS.md`, `DESIGN.md`, `CODE.md`, `APP-CHANGES.md`, `RELEASE.md`,
  `INTERFACES.md`; `CLAUDE.md`, `README.md`; `.github/workflows/ci.yml` (never run).

## In the checkout, not committed, not mine

`bins/hub/src/{app,glue,work,cli,trace,e2e_tests,glue_tests}.rs` and an edit of `main.rs`: the
window, its engine and their tests, being written by another agent. `docs/DESIGN.md` describes
them only from their headers. `cargo build --release -p hub` and `cargo clippy ... -D warnings`
passed with them at the time of writing; `cargo fmt --all --check` reports a difference in
`app.rs` (theirs). Not run by me: their tests (they may open windows), the window itself.

## What does not exist

- **A working window on Windows has not been seen by me.** Nothing here has run as a program in
  a window; every claim about the tray, Run key, single instance, notifications, start at login
  is untested. The plan's phase 0 tray, phase 6 (Hub's installer, self-update, release workflow,
  first run, Settings) are not built.
- Streaming downloads with resume (`fetch_to_file`), a free-space check (D-005). Downloads are
  whole bodies in memory.
- Zip installs (refused), uninstalling from Hub, roll back of setup installs, notes on demand
  (`notes.file`), skipping a version in the window (the store has it), the pre-release channel.
- The `close` request for mdedit and mdview, the `managed-by` stand-down (D-007), and every app
  change in `docs/APP-CHANGES.md`: proposals, applied to nothing.
- Linux and macOS (the window is Windows-only; `--shot` works everywhere).
- Budgets measured: none. First frame, private memory, idle CPU of the window: not measured by
  me (`--trace` exists, `docs/CODE.md`).
- A test in the repository that parses `tools/samples`; a test that `write-hub-txt.ps1` and the
  parser agree on refusals (the script mirrors the parser by hand, D-004).
- Anything run by hand on Windows with a real installer (plan section 12).

## Known wrong, or at risk

- The ci workflow has never run: its Windows-only job, `bash tests/shots.sh` under Git Bash on
  `windows-latest`, and the pinned toolchain `1.99.1` (copied from mdedit) are unproven there.
- **Inbound's `latest` is fragile**: `android-v0.0.9` is a non-pre-release newer than `v0.9.0`
  and `latest` still resolves to `v0.9.0`, so it was made with "latest: false"; the next one made
  without it takes `releases/latest/download/hub.txt` away (and Inbound's own updater's
  `manifest.json`). `docs/APP-CHANGES.md`.
- Until each app's next release carries `hub.txt`, Sprawl and Inbound show "no release
  information"; mdedit and mdview are read through the stopgap (D-003). Uploading the samples to
  the current releases would close the gap: the owner's call.
- The catalog's `wait` list for Inbound lacks `DownloadManager.Server.exe` and
  `inbound-torrent.exe`, which its install tree also runs (Setup's restart manager closes them).
- A download is a memory spike of the file's size during an install (Inbound's Setup 68.7 MiB):
  the 30 MiB private-memory budget holds at rest only.
- The write-hub-txt samples have CRLF line endings in a Windows checkout (git autocrlf); the
  parser trims lines and does not mind. No `.gitattributes` is set.
- mdedit's `uninstall_key` is `None`, so its own updater never writes `DisplayVersion`; the value
  proposed in `APP-CHANGES.md` carries its `HKCU\` prefix, which the shared `record_version`
  needs (a value without it is silently ignored).

## Next

1. The owner reads `docs/APP-CHANGES.md` and decides: the marker for standing down (D-007), the
   `close` request, uploading the samples to the current releases, Inbound's Android releases.
2. Look at the window on Windows (`--behind`, scratch `HUB_DATA`) and measure the budgets.
3. A test in the repository for the samples; Hub's installer and release workflow (phase 6).
