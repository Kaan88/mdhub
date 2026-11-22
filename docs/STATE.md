# State

Rewritten, never appended to. What is true now; `git log` is the story.

## Released

Nothing. No release, no tag, no pushed repository, no GitHub repository, `ci` never run
(`docs/RELEASE.md`). The owner has not asked for a release.

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
  copy; nothing installs Hub yet, so it has run only on a scratch copy of the exe.
- Settings card: start with Windows, tray, notifications, check interval, update Hub.
- Verified 2026-11-22 on this PC: `cargo fmt --all --check` clean; `cargo clippy --workspace
  --all-targets -D warnings` clean with and without `--features hub/test-hooks`; `cargo test
  --workspace` 114 (hub) + 180 (hub-core, 1 ignored), same with test-hooks; no `HKCU\Software\HubTest`
  left behind. Shots: 156 PNGs, check clean.
- Budgets measured (release, `--behind`, fresh `HUB_DATA`, 11 runs unless noted):
  | | now | budget | before the tray work |
  |---|---|---|---|
  | first frame, window path (`--demo main`) | median 62.3 ms, 53.3 to 87.3 | < 100 ms | 67.7 ms |
  | private memory, window open | 9.2 MiB, 9.1 to 9.3 | < 30 MiB | 9.2 MiB |
  | private memory, tray only (9 runs) | 2.9 MiB, 2.9 to 3.0 | < 8 MiB | none |
  | tray only after 20 open/close cycles | 5.1 to 5.9 MiB, flat at the end | < 8 MiB | none |
  | window open during the cycles | 9.4 to 10.2 MiB | < 30 MiB | none |
  | idle CPU over 10 s, tray only / window open | 0 ms / 0 ms | none | 0 |
  | exe size | 3,400,704 bytes | none | 3.18 MB |
  The first frame was measured with `--demo main` (no engine work on the path). A real
  window-first start without `--demo` was not timed here (it needs a test-hooks build and a fake
  github root); the integrator's 212 to 244 ms for it is not reproduced and not explained here.

## What does not exist

- **Seen live: nothing.** The balloon, the icon with its badge, the tray menu, an Explorer
  restart, a click in the settings card were not seen or used by a person. The tray starts and
  stops, the window opens and closes 20 times by message here, and the card is checked by tests
  and shots only. Routing of mouse, keyboard and wheel into the card (`app.rs`) is compiled and
  read, not exercised.
- Hub's installer and release workflow; a repository for Hub (`mdhub` is a working name).
- "Swap at next start" for a downloaded Hub: it is swapped at "Restart now" or at quit, else
  deleted by `clean_up` at the next start and fetched again.
- Streaming downloads with resume, free-space check (D-005): downloads are whole bodies in memory.
- Zip installs (refused), uninstalling from Hub, roll back of setup installs, notes on demand,
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
- Apps' manifest URLs (`manifest.rs`) were not checked for the dot-segment weakness that Hub's own
  pin had; only Hub's self-update pin was fixed.
- Inbound's `latest` is fragile (`android-v0.0.9` newer than `v0.9.0`); see `docs/APP-CHANGES.md`.
- Until each app's next release carries `hub.txt`, Sprawl and Inbound show "no release
  information"; mdedit and mdview are read through the stopgap (D-003).
- The catalog's `wait` list for Inbound lacks `DownloadManager.Server.exe` and
  `inbound-torrent.exe`.
- A download is a memory spike of the file's size during an install (68.7 MiB for Inbound): the
  30 MiB budget holds at rest only.
- Samples have CRLF in a Windows checkout; no `.gitattributes`.

## Next

1. The owner decides the repository name for Hub and whether to release (`docs/RELEASE.md`).
2. Look at the tray, the balloon and the card live on Windows, once, briefly.
3. The installer and release workflow; then the by-hand pass (plan section 12).
4. The owner reads `docs/APP-CHANGES.md` and decides.
