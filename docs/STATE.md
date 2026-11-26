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
  The shared single-page form stays; its card, switches, segments and interval dropdown animate.
- D-036: all four shared crates are at `5665039`. Rows, tiles and buttons ease hover; row status
  and progress animate; list/grid and release-notes content crossfade; the row menu and card
  enter and exit. The one animation wake is armed only while moving. Reduced motion snaps;
  shots use settled endpoints. The tray menu and balloon are native, not in-window toasts.
- Window backdrop (D-033): Off, Mica, Acrylic, `[settings] backdrop`, Off by default, the shared
  `Backdrop` and `Frost`. Frosted: the title row, the command bar, the status bar, only while the
  window has focus; the list and the pane stay opaque. Off draws byte for byte as before. Demos
  `backdrop-mica`, `backdrop-acrylic` show it over a made-up gradient, not what Windows draws.
- The release notes view (D-028): a page for the selected app's latest version with
  `mdedit 1.2.0 -> 1.2.1` and its date under the text; from the row menu, the pane's "Release
  notes" link and the key `n`; Esc, Backspace or the arrow go back. The one-line `notes` are
  available without a fetch; `notes.file` is one GET (64 KiB, 10 s) on open, cached by app and
  version; loading, loaded, failed (reason, Try again) and no file are drawn. The scrollbar's thumb
  drags, a track click pages toward it, and the bar eases wider (`notes-bar-active`).
  A pending offer stays above the notes header (`notes-offer`).
- On Windows behind other windows: the row menu, grid switch, settings and its dropdown settled
  after synthetic input; the notes view opened from a local manifest, fetched one local asset
  and cached it, then closed. Both runs used 0 ms CPU over two seconds after settling.
- Windows release checks: fmt and both clippy commands clean; 186 Hub + 203 core + 2 sample
  tests passed, four existing ignores. The ten new tests cover the motion and its behavior;
  15 targeted reverts in scratch triggered their regression tests.
- Two local renders of 360 PNGs were byte-identical; `shots-check` reported zero bad. The row menu,
  interval dropdown and notes offer endpoints were inspected in both themes (not owner approval).
  The 336 pre-existing shots also match `ed94180` byte for byte.
- D-036 budgets: seven interleaved Windows runs per build (`--behind --demo main --trace`,
  release test hooks and fresh scratch roots). Compare against `ed94180`:
  | | before | now | budget |
  |---|---|---|---|
  | first frame (median, range) | 55.4 ms, 49.8–63.0 | 53.4 ms, 50.7–55.9 | < 100 ms |
  | private memory (median, range) | 9.5 MiB, 9.3–9.5 | 9.4 MiB, 9.4–9.5 | < 30 MiB |
  | idle CPU (each sample) | 0 ms / 2 s | 0 ms / 2 s | no idle CPU |
  | test-hooks exe | 3,691,008 bytes | 4,063,744 bytes | — |
  A separate initial baseline smoke took 178.1 ms; the paired table is not a worst-case bound.
  Tray-only memory, focused Mica/Acrylic timing and real network timing were not remeasured.

## What does not exist

- The Purge and Barbican rows were looked at in a shot, not in a run against real data. The tray
  icon with its badge, the balloon, the tray menu and an Explorer restart were not seen or used
  by a person; no human has approved the motion in a running window.
- A real install, update or roll back of Purge, Barbican or Claude Switch through Hub on this PC.
- Live scroll/drag/track-click through a real notes load have not been tested by a person. The
  scrollbar arithmetic and page paints/transitions are tested. Markdown links are not clickable.
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
