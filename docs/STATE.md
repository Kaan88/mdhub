# State

Rewritten, never appended to. What is true now; `git log` is the story.

## Released

v0.1.0 (2026-11-22), v0.1.1, v0.1.2 and v0.1.3 (2026-11-23), `Kaan88/mdhub`: `Hub-Setup-<v>.exe`,
`hub.exe`, `hub-update.txt`, `hub.txt`; `ci` and `release` passed for each. The owner ran the
installer and the self-update 0.1.0 to 0.1.1 (2026-11-23): both worked. 0.1.3 has six apps. 0.1.2's
fix for updating an open Inbound or Sprawl (D-029) is not yet confirmed on the owner's PC.

## On `main`, committed

- `crates/hub-core`: the catalog, six apps compiled in (D-030 Purge, D-031 Barbican), model, `hub.txt`
  parser and writer plus the legacy stopgap, Hub's own files (`state.rs`), detection, checking,
  transport, installing, and the tray work: `autostart` (Run key), `selfupdate` (Hub's own update),
  `notify` (balloon text and memory).
- Purge: per-machine Inno, `wait` `purge-engine.exe` and `purge-broker.exe`, `setup_closes_app`;
  its v0.1.0 carries `hub.txt`. Barbican: per-user Inno, `setup_closes_app`; **no release** yet, so
  its row shows "no release information" and Hub offers no install for it.
- `crates/hub-core/tests/samples.rs`: each released app's `tools/samples/<id>/hub.txt` parses and
  round-trips; `NO_SAMPLE` names Barbican, and the test fails if it gains a sample.
- `bins/hub`: the window (`app`, winit + softbuffer, created on demand), its engine (`glue`),
  workers (`work`), `cli`, drawing (`vm view grid panes ui`), `--shot` and `demo`, and the tray
  work: `tray` (icon, system menu, balloon, on a thread of its own), `icon` (the icon and badge
  drawn at run time), `settings` (the card of the sliders button).
- Tray: closing the window keeps Hub in the tray, "Quit Hub" / `hub --quit` ends it; `--tray`
  starts with no window and the first check 2 minutes later; badge with the update count; one
  balloon per check that found something (D-021); the icon is re-added on `TaskbarCreated`.
- Start with Windows (Run key, off by default; only for an installed exe, D-020).
- Self-update (D-022): checked first, staged, swapped last, restart; works only for an installed
  copy, where it ran once (0.1.0 to 0.1.1, by the owner).
- The installer (`packaging/windows/hub.iss`, Inno, per user) and the workflows `ci` and `release`:
  all three have run on GitHub and passed; `release` builds the installer on a tag, and the owner
  ran it once (0.1.0).
- The icon (D-024): a white hub and four nodes in the apps' colours on a dark tile, drawn only
  in `icon.rs`; `assets/hub.ico` is packed from it by `tools/make-icon.ps1`. The four nodes were
  not redrawn when the catalog grew to six (D-031).
- The apps' own icons (D-027): one 128 px PNG each in `assets/apps/<id>.png`, decoded on the first
  draw, scaled once per size; the catalog `tile`/`glyph` draw only when an icon does not decode.
- Closing an app (D-025, D-026): the offer bar's yes covers one install; a roll back of an open
  app is not started; a question with no window opens the window.
- Settings card: start with Windows, tray, notifications, backdrop, check interval, update Hub.
- Window backdrop (D-032; on branch `claude/window-backdrop-57484c`, not yet on `main`): Off,
  Mica, Acrylic, `[settings] backdrop`, Off by default, the shared `Backdrop` and `Frost`. Frosted:
  the title row, the command bar, the status bar, only while the window has focus; the list and the
  pane stay opaque. Off draws byte for byte as before. Demos `backdrop-mica`, `backdrop-acrylic`
  show it over a made-up gradient, not what Windows draws.
- The release notes view (D-028): a page for the selected app's latest version with
  `mdedit 1.2.0 -> 1.2.1` and its date under the text; from the row menu, the pane's "Release
  notes" link and the key `n`; Esc, Backspace or the arrow go back. `notes` shows at once;
  `notes.file` is one GET (64 KiB, 10 s) only when the view is opened, kept on disk by app and
  version; loading, loaded, failed (reason, Try again) and no file are drawn.
- Verified 2026-11-24 on this PC, with the backdrop: fmt clean; clippy `-D warnings` clean with and
  without `--features test-hooks`; `cargo test --workspace --release` 166 passed (hub, 1 ignored:
  the icon generator) + 202 (hub-core, 1 ignored) + 2 (samples); `tests/shots.sh` 200 PNGs twice,
  byte-identical, `shots-check` 0 bad; against the 176 shots before it, 140 are byte-identical and
  the 36 `settings-*` differ only by the new row. Each backdrop test fails with its fix reverted.
  Before it: the six-app catalog 2026-11-23 (161 + 201 + 2, 176 shots); the whole repository
  2026-11-22 (no `HKCU\Software\HubTest` left behind).
- Reviewed 2026-11-22, the whole repository: of 36 safety rules removed one at a time, 33 failed a
  test, 2 got one, 1 is covered by a second check; 200,000 mutated inputs: no panic.
- Budgets measured (release, `--behind`, fresh `HUB_DATA`, 11 runs unless noted):
  | | now | budget | before the tray work |
  |---|---|---|---|
  | first frame, window path (`--demo main`) | median 62.3 ms, 53.3 to 87.3 | < 100 ms | 67.7 ms |
  | private memory, window open | 9.2 MiB, 9.1 to 9.3 | < 30 MiB | 9.2 MiB |
  | private memory, tray only (9 runs) | 2.9 MiB, 2.9 to 3.0 | < 8 MiB | none |
  | tray only after 20 open/close cycles | 5.1 to 5.9 MiB, flat at the end | < 8 MiB | none |
  | window open during the cycles | 9.4 to 10.2 MiB | < 30 MiB | none |
  | idle CPU over 10 s, tray only / window open | 0 ms / 0 ms | none | 0 |
  | exe size | 3,441,664 bytes (after the icon and the fixes) | none | 3.18 MB |
  The notes view: a 64 KiB notes file lays out in 31 ms once, a scroll frame in 0.16 ms.
  With the backdrop (2026-11-24, 11 runs each, interleaved with the build before it): `--demo main`
  first frame median 60.9 ms (57.3 to 74.2) against 62.0 (58.6 to 73.4) before; a `test-hooks` run
  without `--demo` 64.1 ms with Off stored, 63.1 with Mica; private memory 9.4 to 9.6 MiB in all;
  exe 3,662,336 bytes against 3,651,584.
  The frosted path itself was not timed: a `--behind` window has no focus and stays flat.
  Unmeasured: a real window-first start, memory with a long notes file open, a real network fetch.

## What does not exist

- **Seen live: nothing new.** The Purge and Barbican rows were looked at in a shot, not in a run
  against real data. The tray icon with its badge, the balloon, the tray menu, an Explorer restart
  and a click in the settings card were not seen or used by a person.
- An install, update or roll back of Purge or Barbican run by Hub on this PC: neither has been
  installed by it (CLAUDE.md rule 4 keeps that off this machine); Barbican has no release.
- The notes view in a real window: the page's wiring in `app.rs` (wheel, keys, scroll kept per
  page) is compiled and read, not run; the scrollbar thumb is drawn but cannot be dragged; the links
  are not clickable.
- The window made because the offer bar asks (D-025): its wiring in `app.rs` has no test.
- The backdrop seen live: Mica or Acrylic behind a focused Hub window has not been looked at, and
  the wiring in `app.rs` (sync on focus and on the setting, the key-out at present, the reset for
  a new window) is compiled and read, not covered by a test.
- A use for the manifest's `.min`, `.uninstall` and `.updates`: they are parsed and ignored.
- "Swap at next start" for a downloaded Hub: it is swapped at "Restart now" or at quit, else
  deleted by `clean_up` at the next start and fetched again.
- Streaming downloads with resume, free-space check (D-005): whole bodies in memory. Zip installs
  (refused), uninstalling from Hub, roll back of setup installs, skipping a version, the
  pre-release channel.
- The `close` request for mdedit and mdview, the `managed-by` stand-down (D-007), and every app
  change in `docs/APP-CHANGES.md`: proposals, applied to nothing.
- Linux and macOS (the window is Windows-only; `--shot` works everywhere).

## Known wrong, or at risk

- Closing the window leaves the tray process at 5 to 6 MiB, not the 2.9 MiB it started with. Under
  budget, but not back to start.
- The settings card's category list marks a group but shows all three at once (D-023).
- The ci workflow's Windows job and the pinned toolchain `1.99.1` pass, but the shots it uploads are
  never looked at by anyone; there is no tray menu shot (the menu is the system's).
- Quitting Hub during an install waits 8 seconds for it and then ends the process: an install caught
  while placing files is cut off there.
- A setup runs from Hub's cache folder, which this user can write to, also when it is elevated.
- `HUB_UPDATE_MANIFEST` lifts the pin on Hub's own downloads, in a release build too (D-022).
- Only the app's main exe is looked at before asking "close it?": a helper of its `wait` list
  running alone is sent `WM_CLOSE` by the install without a question. A setup that installs into
  another folder than the one on record is reported as "still the same version" (read from the
  code, not seen).
- Barbican's own updater does not stand down for Hub and neither does Purge's (D-007 not built):
  until it does, an app's updater and Hub could both offer the same update.
- Barbican shows "no release information" until its first release carries `hub.txt`; its catalog
  entry was written from its installer branch and is unproven against a real release.
- Inbound's `latest` is fragile (`android-v0.0.9` sorts newer than `v0.9.0`); Sprawl and Inbound
  show "no release information" until their next release carries `hub.txt`; mdedit and mdview use
  the stopgap (D-003); Purge 0.1.0 has `hub.txt` (D-030).
- The catalog's `wait` list for Inbound lacks `DownloadManager.Server.exe` and `inbound-torrent.exe`.
- A download is a memory spike of the file's size during an install (68.7 MiB for Inbound): the
  30 MiB budget holds at rest only.
- Samples have CRLF in a Windows checkout, no `.gitattributes`.

## Reviewed, and by whom

- Fable (a second model) reviewed the whole repository once, up to `8b2543a` (the icon): three
  passes, six defects fixed with tests (`48a9005` to `1f1a161`), 36 safety rules mutation-checked,
  200,000 fuzzed inputs, the FFI read for leaks.
- Not reviewed by Fable, written and checked by Sonnet agents only: the apps' own icons, the release
  notes page and its fetch and cache, the 0.1.1 and 0.1.2 bumps, the Purge entry (PR #1) and the
  Barbican entry (PR #2), the 0.1.3 bump, and the window backdrop (written by Opus). No second
  reviewer read either catalog entry or the backdrop.
- No human has read the code.

## Next

1. Look at the tray, the balloon, the card and Mica / Acrylic live on Windows, once, briefly.
2. The by-hand pass (plan section 12): a real install and update of Purge (elevated) through Hub, an
   app updated while open, Explorer restarted under the tray.
3. Barbican's first release, which brings its `hub.txt`, its sample and its updater standing down.
4. The owner reads `docs/APP-CHANGES.md` and decides.
