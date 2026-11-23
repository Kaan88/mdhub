# State

Rewritten, never appended to. What is true now; `git log` is the story.

## Released

v0.1.0 (2026-11-22, `Kaan88/mdhub`: `Hub-Setup-0.1.0.exe`, `hub.exe`, `hub-update.txt`, `hub.txt`; the
release workflow and `ci` passed on GitHub). 0.1.1 is being cut: the apps' own icons, the release
notes page and the review fixes. The installer and a self-update from a real release have not been
run by hand.

## On `main`, committed

- `crates/hub-core`: catalog (four apps), model, `hub.txt` parser and writer plus the legacy
  stopgap, Hub's own files (`state.rs`), detection, checking, transport, installing, and the tray
  work: `autostart` (Run key), `selfupdate` (Hub's own update), `notify` (balloon text and memory).
- `bins/hub`: the window (`app`, winit + softbuffer, created on demand), its engine (`glue`),
  workers (`work`), `cli`, drawing (`vm view grid panes ui`), `--shot` and `demo`, and the tray
  work: `tray` (icon, system menu, balloon, on a thread of its own), `icon` (the icon and badge
  drawn at run time), `settings` (the card of the sliders button).
- Tray: closing the window keeps Hub in the tray, "Quit Hub" / `hub --quit` ends it; `--tray`
  starts with no window and the first check 2 minutes later; badge with the update count; one
  balloon per check that found something (D-021); the icon is re-added on `TaskbarCreated`.
- Start with Windows (Run key, off by default; only for an installed exe, D-020).
- Self-update (D-022): checked first, staged, swapped last, restart. Works only for an installed
  copy; the installer has not been run, so it has run only on a scratch copy of the exe.
- The installer (`packaging/windows/hub.iss`, Inno, per user) and the workflows `ci` and
  `release`: written, the installer never built or run here, `release` never run.
- The icon (D-024): a white hub and four nodes in the apps' colours on a dark tile, drawn only
  in `icon.rs`; `assets/hub.ico` is packed from it by `tools/make-icon.ps1`.
- The apps' own icons (D-027): list, large and small views draw each app's own icon, one 128 px
  PNG in `assets/apps/<id>.png` (mdedit, mdview from their .ico's 128 frame; Sprawl, Inbound
  from their 256 px PNG, downscaled), decoded on the first draw, scaled once per size. The
  catalog `tile`/`glyph` draw only when an icon does not decode.
- Closing an app (D-025, D-026): the offer bar's yes covers one install; a roll back of an open
  app is not started; a question with no window opens the window.
- Settings card: start with Windows, tray, notifications, check interval, update Hub.
- The release notes view (D-028), written after the review below: a page for the selected app's
  latest version with `mdedit 1.2.0 -> 1.2.1` and its date under the text; from the row menu,
  the pane's "Release notes" link and the key `n`; Esc, Backspace or the arrow go back. The
  one-line `notes` shows at once; `notes.file` is one GET (64 KiB, 10 s) only when the view is
  opened, kept on disk by app and version; loading, loaded, failed (reason, Try again) and no
  file are drawn. A Markdown subset reader of Hub's own over the shared painter, no new
  dependency. Tests: 17 for the reader and page, 13 end to end, 4 for the url, cleaning and the
  cache; 5 new shots (list view only), looked at in both themes at both sizes.
- Verified 2026-11-23 on this PC with the notes view: fmt, clippy (both feature sets) clean; tests
  159 (hub, 1 ignored) + 188 (hub-core, 1 ignored), same with test-hooks; shots 176 PNGs, check
  clean, two runs byte-identical. Before it, verified 2026-11-22: `cargo fmt --all --check` clean; `cargo clippy --workspace
  --all-targets -D warnings` clean with and without `--features hub/test-hooks`; `cargo test
  --workspace` 129 (hub, 1 ignored: the icon generator) + 183 (hub-core, 1 ignored), same with
  test-hooks; no `HKCU\Software\HubTest` left behind. Shots: 156 PNGs, check clean.
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
  The first frame was measured with `--demo main` (no engine work on the path). A real
  window-first start without `--demo` was not timed here (it needs a test-hooks build and a fake
  github root); the integrator's 212 to 244 ms for it is not reproduced and not explained here.
  After the notes view (2026-11-23, 7 runs, `--demo main`): first frame 57.9 to 66.3 ms (median
  59.5), private memory 9.5 to 9.6 MiB, CPU time flat from the first frame to exit (no CPU at
  idle), exe 3,638,272 bytes. A 64 KiB notes file lays out in 31 ms once, a scroll frame draws in
  0.16 ms from the kept layout (`a_64_kib_file_lays_out_once_and_scrolls_cheaply`). The view is
  not on the launch path: nothing of it runs until it is opened. Not measured: the view open in a
  real window (memory with the fonts' layouts of a long text), a real network fetch.
  The times and memory were NOT measured again after the icon and the review fixes: the icon is
  drawn once per change of the count, never per frame, and the fixes add nothing to the launch
  path, but that is reasoning, not a measurement.

## What does not exist

- **Seen live: nothing.** The new icon was looked at on a contact sheet (16 to 256 px, light and
  dark, with and without a badge), not in the tray. The balloon, the icon with its badge, the tray menu, an Explorer
  restart, a click in the settings card were not seen or used by a person. The tray starts and
  stops, the window opens and closes 20 times by message here, and the card is checked by tests
  and shots only. Routing of mouse, keyboard and wheel into the card (`app.rs`) is compiled and
  read, not exercised.
- The notes view in a real window: the page's wiring in `app.rs` (wheel, keys, the scroll kept per
  page) is compiled and read, not run; the scrollbar thumb is drawn but cannot be dragged; the
  links in the notes are not clickable; the mock picture of the owner was not available to look
  at (its file was not found), the page follows the description "1.2.1 above the installed 1.2.0".
- The window made because the offer bar asks (D-025): its wiring in `app.rs` has no test (it
  needs a window); the rule and the engine's side have.
- A use for the manifest's `.min`, `.uninstall` and `.updates`: they are parsed and ignored.
- "Swap at next start" for a downloaded Hub: it is swapped at "Restart now" or at quit, else
  deleted by `clean_up` at the next start and fetched again.
- Streaming downloads with resume, free-space check (D-005): downloads are whole bodies in memory.
- Zip installs (refused), uninstalling from Hub, roll back of setup installs,
  skipping a version in the window, the pre-release channel.
- The `close` request for mdedit and mdview, the `managed-by` stand-down (D-007), and every app
  change in `docs/APP-CHANGES.md`: proposals, applied to nothing.
- Linux and macOS (the window is Windows-only; `--shot` works everywhere).

## Known wrong, or at risk

- Closing the window leaves the tray process at 5 to 6 MiB, not the 2.9 MiB it started with (the
  winit loop and heap keep what the window used). Under budget, but not back to start.
- The settings card's category list marks a group but shows all three at once (D-023).
- Only 4 of the new shots were looked at one by one (settings general light 1280x800, settings
  updates dark 1280x800, settings about dark 860x600, banner list light 860x600); no tray menu
  shot exists (the menu is the system's, covered by tests only).
- The ci workflow has never run; its Windows job and the pinned toolchain `1.99.1` are unproven.
- Quitting Hub ("Quit Hub", `--quit`) during an install waits 8 seconds for it and then ends the
  process: an install caught while placing files is cut off there (the next start finds what is
  on disk; `.old` files may stay beside the app until it cleans up itself).
- A setup runs from Hub's cache folder, which this user can write to, also when it is elevated:
  the file is locked and hashed just before, but the folder is not protected.
- `HUB_UPDATE_MANIFEST` lifts the pin on Hub's own downloads, in a release build too (D-022).
- Only the app's main exe is looked at before asking "close it?": a helper of its `wait` list
  running alone is sent `WM_CLOSE` by the install without a question.
- A setup that installs into another folder than the one on record is reported as "still the
  same version" (the record's exe is found first). Not seen; read from the code.
- Inbound's `latest` is fragile (`android-v0.0.9` newer than `v0.9.0`); see `docs/APP-CHANGES.md`.
- Until each app's next release carries `hub.txt`, Sprawl and Inbound show "no release
  information"; mdedit and mdview are read through the stopgap (D-003).
- The catalog's `wait` list for Inbound lacks `DownloadManager.Server.exe` and
  `inbound-torrent.exe`.
- A download is a memory spike of the file's size during an install (68.7 MiB for Inbound): the
  30 MiB budget holds at rest only.
- Samples have CRLF in a Windows checkout; no `.gitattributes`.

## Reviewed, and by whom

- Fable (a second model) reviewed the whole repository once, up to `8b2543a` (the new icon): three
  passes, six defects fixed with tests (`48a9005` to `1f1a161`), 36 safety rules mutation-checked,
  200,000 fuzzed inputs, the FFI read for leaks. Its list of things needing the owner is under
  "Known wrong".
- Not reviewed by Fable, written and checked by Sonnet agents only: the apps' own icons (`cb25ff4`),
  the release notes page and its fetch and cache (`67d423c`), the 0.1.1 bump, and the first builds
  (two Sonnet reviewers per module). The four notes-file follow-up PRs (mdedit #11, mdview #8,
  Sprawl #2, Inbound #54) had no reviewer besides their author.
- No human has read the code.

## Next

1. The owner decides the repository name for Hub and whether to release (`docs/RELEASE.md`).
2. Look at the tray, the balloon and the card live on Windows, once, briefly.
3. The installer and release workflow; then the by-hand pass (plan section 12).
4. The owner reads `docs/APP-CHANGES.md` and decides.
