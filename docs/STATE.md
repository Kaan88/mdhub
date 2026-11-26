# State

Rewritten, never appended to. What is true now; `git log` is the story.

## Released

v0.1.0 (2026-11-22), v0.1.1, v0.1.2, v0.1.3 (2026-11-23), v0.1.4, v0.1.5 (2026-11-24), `Kaan88/mdhub`:
`Hub-Setup-<v>.exe`, `hub.exe`, `hub-update.txt`, `hub.txt`; `ci` and `release` passed for each. The owner ran the
installer and the self-update 0.1.0 to 0.1.1 (2026-11-23): both worked. 0.1.3 has six apps. 0.1.2's
fix for updating an open Inbound or Sprawl (D-029) is not yet confirmed on the owner's PC.
0.1.4 brings hover and press (D-032), the notes scrollbar's drag and the window backdrop (D-033),
three branches merged on `release/0.1.4`; none of the three was tried in a real window. 0.1.5
makes the settings card one page with no category list (D-034).

## In this checkout

- `crates/hub-core`: seven apps compiled in (D-035 Claude Switch), model, `hub.txt`
  parser and writer plus the legacy stopgap, Hub's own files (`state.rs`), detection, checking,
  transport, installing, and the tray work: `autostart` (Run key), `selfupdate` (Hub's own update),
  `notify` (balloon text and memory).
- Purge: per-machine Inno, `wait` `purge-engine.exe` and `purge-broker.exe`, `setup_closes_app`;
  its v0.1.0 carries `hub.txt`. Barbican: per-user Inno, `setup_closes_app`; **no release** yet, so
  its row shows "no release information" and Hub offers no install for it. Claude Switch is also
  unreleased; its per-user setup never closes the running app, to protect live embedded sessions.
- `crates/hub-core/tests/samples.rs`: each released app's `tools/samples/<id>/hub.txt` parses and
  round-trips; `NO_SAMPLE` names Barbican and Claude Switch until they gain real release samples.
- `bins/hub`: the window (`app`, winit + softbuffer, created on demand), its engine (`glue`),
  workers (`work`), `cli`, drawing (`vm view grid panes ui`), `--shot` and `demo`, and the tray
  work: `tray` (icon, system menu, balloon, on a thread of its own), `icon` (the icon and badge
  drawn at run time), `settings` (the card of the sliders button).
- Tray: closing the window keeps Hub in the tray, "Quit Hub" / `hub --quit` ends it; `--tray`
  starts with no window, first check 2 minutes later; badge; one balloon per check that found
  something (D-021); re-added on `TaskbarCreated`. Start with Windows: Run key, off (D-020).
- Window (D-036): 860×660 logical by default, enough for seven list rows, the details pane and the
  footer. Close-to-tray and quit save position, logical client size and maximized state in
  `settings.ini`; restore uses the current monitor work areas. `--size` and shots are independent.
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
- Hover and press (D-032): every button lights under the pointer and darkens while held (window
  buttons, command bar, view switch, offer bar and its X, row and tile buttons, "..." menus, notes
  page buttons; the "Release notes" link underlines); redrawn only when the control under the
  pointer changes. The "close it and update" question is a warning (`warn: true`).
- Window backdrop (D-033): Off, Mica, Acrylic, `[settings] backdrop`, Off by default, the shared
  `Backdrop` and `Frost`. Frosted: the title row, the command bar, the status bar, only while the
  window has focus; the list and the pane stay opaque. Off draws byte for byte as before. Demos
  `backdrop-mica`, `backdrop-acrylic` show it over a made-up gradient, not what Windows draws.
- The release notes view (D-028): a page for the selected app's latest version with
  `mdedit 1.2.0 -> 1.2.1` and its date under the text; from the row menu, the pane's "Release
  notes" link and the key `n`; Esc, Backspace or the arrow go back. `notes` shows at once;
  `notes.file` is one GET (64 KiB, 10 s) only when the view is opened, kept on disk by app and
  version; loading, loaded, failed (reason, Try again) and no file are drawn. The scrollbar's thumb
  drags, a track click pages toward it, the bar widens under the pointer (`notes-bar-active`).
- Windows release checks for D-036: fmt, both clippy runs, 389 tests passed with four existing
  ignores; all eight new tests caught deliberate regressions. The two sets of 336 shots were
  byte-identical and passed `shots-check`; list, grids and settings were reviewed in both themes.
  Scratch Windows runs verified tray reopen, restart, maximize, off-screen fallback and CLI
  override. Physical monitor changes and mixed DPI were not tested on hardware.
- D-036 `--trace`, seven paired runs per path: first-frame median 185.3 → 190.9 ms (demo) and
  215.5 → 223.3 ms (normal startup with a saved rectangle), both already over the 100 ms budget.
  Private memory 9.4–9.6 → 9.8–10.1 MiB; after: 0 ms CPU in every 2 s idle sample.
- Historical tray measurements (release, `--behind`, fresh `HUB_DATA`, 11 runs unless noted):
  | | after the tray work | budget | before the tray work |
  |---|---|---|---|
  | first frame, window path (`--demo main`) | median 62.3 ms, 53.3 to 87.3 | < 100 ms | 67.7 ms |
  | private memory, window open | 9.2 MiB, 9.1 to 9.3 | < 30 MiB | 9.2 MiB |
  | private memory, tray only (9 runs) | 2.9 MiB, 2.9 to 3.0 | < 8 MiB | none |
  | tray only after 20 open/close cycles | 5.1 to 5.9 MiB, flat at the end | < 8 MiB | none |
  | window open during the cycles | 9.4 to 10.2 MiB | < 30 MiB | none |
  | idle CPU over 10 s, tray only / window open | 0 ms / 0 ms | none | 0 |
  | exe size | 3,441,664 bytes (after the icon and the fixes) | none | 3.18 MB |
  A 64 KiB notes file lays out in 31 ms once, a scroll frame in 0.16 ms. Still unmeasured: the
  focused frosted path, an installed-copy start, and a real network fetch.

## What does not exist

- The Purge and Barbican rows were looked at in a shot, not in a run
  against real data. The tray icon with its badge, the balloon, the tray menu, an Explorer restart
  and a click in the settings card were not seen or used by a person. Hover and press were seen in
  shots only: the pointer wiring in `app.rs` is compiled, not driven by a mouse, and has no test.
- A real install, update or roll back of Purge, Barbican or Claude Switch through Hub on this PC.
- The notes view in a real window: the page's wiring in `app.rs` (wheel, keys, scroll kept per
  page, the scrollbar's drag, track click and hover) is compiled and read, not run; the arithmetic
  under it (`changelog::bar_press`, `bar_drag`) is tested. The links are not clickable.
- The window made because the offer bar asks (D-025): no test of its wiring in `app.rs`.
- The backdrop seen live: Mica or Acrylic behind a focused Hub window has not been looked at, and
  the wiring in `app.rs` (sync on focus and on the setting, the key-out at present, the reset for
  a new window) is compiled and read, not covered by a test.
- A use for the manifest's `.min`, `.uninstall` and `.updates`: they are parsed and ignored.
- "Swap at next start" for a downloaded Hub: it is swapped at "Restart now" or at quit, else
  deleted by `clean_up` at the next start and fetched again.
- Streaming downloads with resume, free-space check (D-005): whole bodies in memory. Zip installs
  (refused), uninstalling from Hub, roll back of setup installs, skipping a version, the
  pre-release channel.
- The `close` request for mdedit and mdview, the `managed-by` stand-down (D-007), and the other
  proposals in `docs/APP-CHANGES.md`.
- Linux and macOS (the window is Windows-only; `--shot` works everywhere).

## Known wrong, or at risk

- Closing the window leaves the tray at 5 to 6 MiB, not the 2.9 MiB it started with (in budget).
- The close question's warning is weak: the shared offer bar draws its sign in muted grey on the
  ordinary bar colour, and it still appears at once. A warning-tinted bar (`pal.warn_bg`) needs an
  opt-in variant in `markdown-shared-rs`; not built, the owner decides.
- The title bar's max button is always drawn as "not maximized" (`app.rs` ignores the state).
- Nobody looks at the shots `ci` uploads; there is no tray menu shot (the menu is the system's).
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
- Barbican and Claude Switch show "no release information" until their first releases carry
  `hub.txt`; neither entry has been verified against a published installer.
- Inbound's `latest` is fragile (`android-v0.0.9` sorts newer than `v0.9.0`); Sprawl and Inbound
  show "no release information" until their next release carries `hub.txt`; mdedit and mdview use
  the stopgap (D-003); Purge 0.1.0 has `hub.txt` (D-030).
- The catalog's `wait` list for Inbound lacks `DownloadManager.Server.exe` and `inbound-torrent.exe`.
- Downloads spike memory by the file's size (68.7 MiB for Inbound); the 30 MiB budget is for idle.

## Reviewed, and by whom

- Fable (a second model) reviewed the whole repository once, up to `8b2543a` (the icon): three
  passes, six defects fixed with tests (`48a9005` to `1f1a161`), 36 safety rules mutation-checked,
  200,000 fuzzed inputs, the FFI read for leaks.
- Not reviewed by a second model: the apps' icons, the notes page, the bumps, the Purge and
  Barbican entries, the notes scrollbar drag, hover and press, the window backdrop. No human has
  read the code.

## Next

1. Look at the tray, the balloon, the card and Mica / Acrylic live on Windows, once, briefly.
2. The by-hand pass (plan section 12): a real install and update of Purge (elevated) through Hub, an
   app updated while open, Explorer restarted under the tray.
3. First releases of Barbican and Claude Switch: verify their `hub.txt` and add real samples.
