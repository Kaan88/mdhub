# State

Current checkout only; `git log` is the history.

## Release and scope

- Latest Hub release: **v0.1.5**, `Kaan88/mdhub`. Its Windows installer, `hub.exe`,
  `hub-update.txt` and `hub.txt` are published. The changes below are not released.
  The owner previously verified an installed copy and the v0.1.0 → v0.1.1 self-update.
- Seven compiled-in apps: mdedit, mdview, Sprawl, Inbound, Purge, Barbican and Claude Switch
  (D-035). Checked-in manifests under `tools/samples/` parse and round-trip. Barbican and
  Claude Switch have no checked-in release sample; without a manifest Hub offers no install.
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
- **Motion (D-036):** all four shared crates are pinned at `5665039`. Hovers, statuses and
  progress animate; list/grid and notes content crossfade; row menus, the settings card and
  the interval dropdown enter and exit. The loop wakes only for a pending deadline, reduced
  motion snaps, and shots use settled endpoints. Tested motion goes idle after settling.
- **Window placement (D-037):** default 860×660 logical px, minimum 520×360. Close-to-tray and
  quit save the normal position, logical client size and maximized state. Restore uses the
  current work areas; an inaccessible saved position falls back to a centered default.
  `--size` and demos bypass saved placement. The caption shows Restore when maximized.
- **Activation (D-038):** ordinary controls arm on press and activate on release over the
  same still-available target. Focus loss cancels. App actions use catalog ID and action,
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
  denied. Release workspace tests: 218 Hub + 205 core + 2 sample tests passed, 4 existing
  ignores. Both merge regressions failed against targeted scratch reverts.
- Two renders of 440 PNGs were byte-identical; `shots-check` reported 0 bad. Reviewed the
  small Install menu, long versions, filter states, close offers and notes focus in both
  themes. Shots are not owner approval.
- Live `--behind` scratch runs with posted Windows input: caption activates on release,
  not on press, drag-away or focus loss. At 520×360 the small overflow opens on release;
  Install starts only on the same-target release and copies a verified inert payload to
  the fake programs folder. Notes retries from keyboard focus and reopens from cache.
  Caption/menu and small-menu/row motion both settled at 0 ms CPU over two seconds.

Seven interleaved Windows `--behind --demo main --trace` runs per build, release with
`test-hooks` and fresh scratch roots. Baseline is `81ec2a3` (input polish before the merge).

| Metric | Before | Merged | Budget |
|---|---:|---:|---:|
| First frame, median / max | 139.5 / 338.5 ms | 139.7 / 174.1 ms | <100 ms |
| Presented, median / max | 173.9 / 407.6 ms | 171.9 / 209.1 ms | — |
| Private memory, range | 9.9 MiB | 9.8–9.9 MiB | <30 MiB |
| Idle CPU, every ~2 s sample | 0 ms | 0 ms | 0 ms |
| Test-hooks exe | 4,092,928 bytes | 4,094,464 bytes | — |

Every run on both builds missed the 100 ms budget. The first-frame medians are 0.2 ms
apart; the baseline maximum is one slow run whose cause is unknown. These are sample
maxima, not bounds. Tray-only memory, focused backdrop and live network were not measured.

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

1. Profile the missed first-frame budget and keep open/idle memory and CPU checks.
2. Get owner review of the polished window and a real tray/backdrop pass outside agent tests.
3. Verify published release manifests and add samples as Barbican and Claude Switch ship them.
