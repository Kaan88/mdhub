# State

Rewritten, never appended to. What is true now; `git log` is the story.

## Released

v0.1.0 (2026-11-22), v0.1.1, v0.1.2 and v0.1.3 (2026-11-23), `Kaan88/mdhub`: `Hub-Setup-<v>.exe`,
`hub.exe`, `hub-update.txt`, `hub.txt`; `ci` and the release workflow passed on GitHub for each. The
owner ran the installer and the self-update from 0.1.0 to 0.1.1 on their PC (2026-11-23): both
worked. 0.1.3 is the first release whose catalog has six apps (Purge D-030, Barbican D-031). Not yet
run by hand: an elevated setup (Sprawl, Purge), an app updated while open, Explorer restarting under
the tray, the balloon and the tray menu seen live. 0.1.1 could not update Inbound or Sprawl while
they ran (no window to close); 0.1.2 has the fix (D-029), not yet confirmed on the owner's PC.

## On `main`, committed

- `crates/hub-core`: the catalog, six apps compiled in (D-030 Purge, D-031 Barbican), model, `hub.txt`
  parser and writer plus the legacy stopgap, Hub's own files (`state.rs`), detection, checking,
  transport, installing, and the tray work: `autostart` (Run key), `selfupdate` (Hub's own update),
  `notify` (balloon text and memory).
- Purge: per-machine Inno (`Program Files\Purge`), `Purge.exe`, `wait` `purge-engine.exe` and
  `purge-broker.exe`, `setup_closes_app`; its v0.1.0 release already carries `hub.txt`, so its row
  shows a real version. Barbican: per-user Inno, `barbican.exe`, `setup_closes_app`; **no release**
  yet (its installer is in its own repository, unreleased), so its row shows "no release
  information" and Hub offers no install for it.
- `crates/hub-core/tests/samples.rs`: every catalog app with a release parses its
  `tools/samples/<id>/hub.txt` and round-trips it; `NO_SAMPLE` names the one app without a release
  (Barbican) and the test fails if that app gains a sample, or if a listed id leaves the catalog.
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
- The apps' own icons (D-027): list, large and small views draw each app's own icon, one 128 px
  PNG in `assets/apps/<id>.png` (mdedit, mdview from their .ico's 128 frame; Sprawl, Inbound from
  their 256 px PNG; Purge from its .ico's 256 frame; Barbican's own 128 px; downscaled where
  larger), decoded on the first draw, scaled once per size. The catalog `tile`/`glyph` draw only
  when an icon does not decode.
- Closing an app (D-025, D-026): the offer bar's yes covers one install; a roll back of an open
  app is not started; a question with no window opens the window.
- Settings card: start with Windows, tray, notifications, check interval, update Hub.
- The release notes view (D-028): a page for the selected app's latest version with
  `mdedit 1.2.0 -> 1.2.1` and its date under the text; from the row menu, the pane's "Release
  notes" link and the key `n`; Esc, Backspace or the arrow go back. `notes` shows at once;
  `notes.file` is one GET (64 KiB, 10 s) only when the view is opened, kept on disk by app and
  version; loading, loaded, failed (reason, Try again) and no file are drawn. The scrollbar's thumb
  drags, a track click pages toward it, the bar widens under the pointer (`notes-bar-active`).
- Verified 2026-11-24 (notes scrollbar drag): fmt, clippy (with and without test-hooks) clean;
  tests 165 (hub) + 201 (hub-core) + 2; 180 shots twice, byte-identical, 0 bad. The whole repository
  2026-11-22: 129 + 183, both clippies, no `HKCU\Software\HubTest` left behind.
- Reviewed 2026-11-22, the whole repository: 36 safety rules were removed one at a time (hash and
  size checks, URL pins, the Run value's quoting, asking before closing, one install at a time);
  33 failed a test, 2 had none and got one, 1 is covered by a second check. 200,000 mutated
  manifests, ini files and roll back notes: no panic. The defects found are fixed (`git log`).
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
  With the notes view (7 runs, `--demo main`): first frame 57.9 to 66.3 ms (median 59.5), private
  memory 9.5 to 9.6 MiB, exe 3,638,272 bytes; a 64 KiB notes file lays out in 31 ms once, a scroll
  frame draws in 0.16 ms from the kept layout. Scrollbar drag, 2 runs before / after: first frame
  57.8, 56.9 / 69.3, 61.8 ms, private 9.5 / 9.5 MiB, no idle CPU, exe +1.5 KB.
  **The times were NOT measured again after the icon, the review fixes, the notes view in a
  window, Purge or Barbican**: two new rows add text and two decoded icons, nothing on the launch
  path, but that is reasoning. Unmeasured: a real window-first start, memory with a long notes file
  open, a real network fetch.

## What does not exist

- **Seen live: nothing new.** The Purge and Barbican rows were looked at in a shot, not in a run
  against real data. The tray icon with its badge, the balloon, the tray menu, an Explorer restart
  and a click in the settings card were not seen or used by a person.
- An install, update or roll back of Purge or Barbican run by Hub on this PC: neither has been
  installed by it (CLAUDE.md rule 4 keeps that off this machine); Barbican has no release.
- The notes view in a real window: the page's wiring in `app.rs` (wheel, keys, scroll kept per
  page, the scrollbar's drag, track click and hover) is compiled and read, not run; the arithmetic
  under it (`changelog::bar_press`, `bar_drag`) is tested. The links are not clickable.
- The window made because the offer bar asks (D-025): its wiring in `app.rs` has no test.
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
  notes page (fetch, cache), the 0.1.1 and 0.1.2 bumps, the Purge entry (PR #1), the Barbican entry
  (PR #2), the 0.1.3 bump; the notes scrollbar drag by an Opus agent alone. No second reviewer.
- No human has read the code.

## Next

1. Look at the tray, the balloon and the card live on Windows, once, briefly.
2. The by-hand pass (plan section 12): a real install and update of Purge (elevated) through Hub, an
   app updated while open, Explorer restarted under the tray.
3. Barbican's first release, which brings its `hub.txt`, its sample and its updater standing down.
4. The owner reads `docs/APP-CHANGES.md` and decides.
