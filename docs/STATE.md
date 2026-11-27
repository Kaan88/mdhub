# State

Current checkout only; `git log` is the history.

## Release and scope

- Latest Hub release: **v0.1.7**, `Kaan88/mdhub`. Its Windows installer, `hub.exe`,
  `hub-update.txt` and `hub.txt` are published from the tag; everything below is in it.
  The owner previously verified an installed copy and the v0.1.0 → v0.1.1 self-update.
- Eight compiled-in apps: mdedit, mdview, Sprawl, Inbound, Purge, Barbican, Claude Switch and
  Sözlük (D-035, D-042). Checked-in manifests under `tools/samples/` parse and round-trip. Barbican and
  Claude Switch have no checked-in release sample (Sözlük's is its v0.3.0 `hub.txt`); without a manifest Hub offers no install.
- Hub reads each app's latest release on GitHub; mdedit and mdview have the legacy fallback
  (D-003). Hub verifies the download before placing or running it. No other servers or accounts.
- `hub-core` owns manifest, detection, transport, installation, rollback and persistent state.
  `bins/hub` owns the engine, workers, winit/softbuffer UI, native tray, and offscreen shots.

## In this checkout

- The window exists only while open. Close normally returns to the tray; Quit ends Hub.
  The native tray has an update badge, a menu and one balloon per check with new updates.
  The initial check is scheduled two minutes after launch, not run on the launch path.
  The Run-key setting is off by default. Hub's self-update stages first and swaps on
  Restart now or quit; it does not swap at the next start.
- **Motion (D-036, D-043):** all four shared crates are pinned at `15f0138`. Hovers, statuses and
  progress animate; list/grid and notes content crossfade; row menus, the settings card and
  the interval dropdown enter and exit. The loop wakes only for a pending deadline, reduced
  motion snaps, and shots use settled endpoints. Tested motion goes idle after settling.
- **Window placement (D-037):** default 860×712 logical px, minimum 520×360. Close-to-tray and
  quit save the normal position, logical client size and maximized state. Restore uses the
  current work areas; an inaccessible saved position falls back to a centered default.
  `--size` and demos bypass saved placement. The caption shows Restore when maximized.
- **Activation (D-038):** ordinary controls arm on press and activate on release over the
  same still-available target. Focus loss cancels, and lets go of a press on the settings
  card (its scrollbar thumb) off every control. App actions use catalog ID and action,
  and an offer is scoped to owner, version and issuance; a replacement cannot take its
  consent. Drag, resize and notes scrollbar gestures still start on press.
- **Notes and filtering (D-039):** notes has its own keys and scroll; Tab/Shift+Tab reaches
  Back, Try again when available, and View on GitHub. Opening notes clears filter focus but
  keeps the query and selection. Search opens from the compact toolbar or Ctrl+F, with
  Clear, a no-match message, and Esc clearing before blurring. Update all counts the whole
  eligible catalog; grid counts follow visible results.
- **Chrome and offer (D-040):** app-close offers are warning-tinted and state the consequence
  before asking. Hub restart offers retain their usual appearance. The list drops both
  version columns to preserve 110 px for names; the footer drops privacy text before
  shortening the notice and does not let it run under the globe or version.
- **Grid and pointer (D-041):** compact tiles have a left-aligned overflow menu with app
  actions and no duplicate Open. Release checks current enablement; Update still asks
  before closing an app. Small-tile status stays readable with a dimmed icon; failure and
  done badge glyphs use theme-specific ink. Large tiles elide both versions inside a padded
  line. Keyboard selection, view switches and edits to the filter reveal rows or tiles;
  unrelated commands do not snap back after manual scrolling. One pointer resolver handles
  overlays, resize edges and
  painted hits for move, redraw, press and release. Leaving clears hover. The filter uses
  a text cursor, the notes link a hand, and the notes thumb sits inside the resize band.
- **Copy and notes links:** settings/tray wording agrees where they share actions (including
  Start with Windows and Notify about updates). Settings is a single card with a “Manually
  only” check option. Markdown link labels are plain text; View on GitHub opens the
  release. Notes fetches at most one asset per attempt (64 KiB, 10 s) and caches by app/version.
- The D-033 backdrop is Off by default. Mica and Acrylic affect focused Windows 11 chrome
  only; the content stays opaque. Backdrop demos use a stand-in, not the Windows compositor.
- Purge's per-machine Inno release has `hub.txt` and two helpers in the wait list. Barbican
  is per-user. Claude Switch's setup does not close its terminal sessions. App-close consent
  covers one install; rollback of an open app is refused (D-025/D-026).

## Windows verification

- `cargo fmt --all --check`, workspace clippy and `test-hooks` clippy passed with warnings
  denied. Release workspace tests: 218 Hub + 206 core + 2 sample tests passed, 4 existing
  ignores. Both merge regressions failed against targeted scratch reverts.
- Two renders of 440 PNGs were byte-identical; `shots-check` reported 0 bad. Reviewed the
  small Install menu, long versions, filter states, close offers and notes focus in both
  themes. Shots are not owner approval.
- Live `--behind` scratch runs with posted Windows input: caption activates on release,
  not on press, drag-away or focus loss. At 520×360 the small overflow opens on release;
  Install starts only on the same-target release and copies a verified inert payload to
  the fake programs folder. Notes retries from keyboard focus and reopens from cache.
  Caption/menu and small-menu/row motion both settled at 0 ms CPU over two seconds.

Seven interleaved Windows `--behind --no-session --demo main --exit-after-ms 5000 --trace`
runs per build, release with `test-hooks` and fresh scratch roots. Baseline is `v0.1.6`;
after is v0.1.7 (Sözlük, the 712 px default height and shared at `15f0138`).

| Metric | v0.1.6 | v0.1.7 | Budget |
|---|---:|---:|---:|
| First frame, median / max | 53.9 / 56.0 ms | 53.7 / 58.1 ms | <100 ms |
| Private memory at exit, range | 10.0–10.1 MiB | 10.4–10.5 MiB | <30 MiB |
| CPU from window open to exit (5 s) | 0–15.6 ms | 0–15.6 ms | 0 ms idle |
| Test-hooks exe | 4,094,464 bytes | 4,442,624 bytes | — |

Every run on both builds met the 100 ms budget; the earlier 139 ms medians were taken on
the same PC under different load, so the change is not Hub's. The CPU figure is one 15.6 ms
timer tick in some runs of both builds, spent while the open settles, not a separate idle
sample. The +0.4 MiB and +340 KiB come from the shared bump. Tray-only memory, focused
backdrop and live network were not measured.

## Not verified or still at risk

- No owner approval of the final UI in a real window. Real install/update/rollback, focused
  Mica/Acrylic, native tray/balloon usability and Explorer restarts were not exercised by
  this merge. Physical monitor changes and mixed DPI have not been tested on hardware.
- The offer's automatic window-open when only the tray is showing still lacks a wiring test.
- No real confirmation of D-029 on this PC for an open Inbound or Sprawl. A helper running
  without its main app can still receive `WM_CLOSE` without an offer. Inbound's `latest` is
  fragile and its wait list lacks `DownloadManager.Server.exe` and `inbound-torrent.exe`.
- Quit waits up to eight seconds for an install before ending Hub. Setup can run from the
  user-writable cache even when elevated. `HUB_UPDATE_MANIFEST` can override the pin on Hub's
  own downloads. Some setup destinations can be reported as unchanged if redetection misses.
- Whole-body downloads can exceed the idle memory budget while installing. No resume,
  space check, zip installs, in-Hub uninstall, setup rollback or prerelease channel.
- `.min`, `.uninstall` and `.updates` are parsed but not used. `managed-by` stand-down and
  the mdedit/mdview close request are still proposals (`APP-CHANGES.md`). The window is
  Windows-only; shots can render without a window.

## Next

1. Keep first-frame, open/idle memory and CPU checks; watch the exe size shared adds.
2. Get owner review of the polished window and a real tray/backdrop pass outside agent tests.
3. Verify published release manifests and add samples as Barbican and Claude Switch ship them. Sözlük's repository is public
   (D-044), so Hub reads its releases.
