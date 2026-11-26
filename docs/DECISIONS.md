# Decisions

Appended to, never edited. A decision that changes gets a new entry that names the old one.
D-001 to D-003 and D-008 to D-016 are the plan's section 18 (`docs/launcher-plan.md` in mdedit's
repository) as they apply to this slice; D-004 to D-007 are taken while building. "Plan" below is
that document; dates are 2026-11-22 unless said otherwise.

## D-001 Hub is its own program and repository, not part of mdedit
Why: the launch path, running when the editor is not, updating the editor. Would change it: nothing.

## D-002 One manifest, `hub.txt`, in every app's release; no adapters for the apps' own formats
The family's `key: value` grammar with a header (`hub: 1`) no old parser reads, written by one
script (`tools/write-hub-txt.ps1`) from what the release workflow already has. Read from
`releases/latest/download/hub.txt`. Why: one parser to trust, one workflow step per repository.
Would change it: an app that cannot change its release workflow. (Plan 18 #2, #3.)

## D-003 Stopgap: mdedit and mdview are read through their old manifests until their release carries hub.txt
`manifest::from_legacy` reads `mdedit-update-2.txt` / `mdview-update.txt` (via
`mdshared_app::update::UpdateManifest`) as a `files` entry (size 0: the legacy format has none),
and `check_one` asks for it only when `hub.txt` is absent: one extra request. This is the
"adapter" D-002 refuses, accepted for two apps because the first useful slice (plan 14) must work
before phase A1 lands. Sprawl and Inbound have no stopgap: until their next release they show "no
release information". Would change it: drop `legacy_manifest` from the catalog entries and delete
`from_legacy` once both apps ship `hub.txt`; a release of Hub does that.

## D-004 The hub.txt parser lives in hub-core, not in the shared crate yet
Plan 3.4 puts it in `mdshared_app::hubmanifest` so each app's own test can parse the file its
workflow writes. Not done: a change in `markdown-shared-rs` must be run against both apps' shots
and pushed, then both apps' `rev` moved (CLAUDE.md of mdedit, rule 8), and nothing may be pushed
now. The parser is a self-contained module (`manifest.rs`, depends on `mdshared_app::update` for
`Version` only) written to be lifted. Meanwhile `write-hub-txt.ps1` repeats the parser's checks
(names, charsets, sizes, 9 files, 128 lines, 16 KiB) and the four samples are parsed by a test
here. Would change it: the first push to the shared repository; then move the file and add a
variant, not a change, for existing callers.

## D-005 The transport reads whole bodies into memory; streaming and resume are not built
`Transport::get(url, max_bytes, timeout) -> Vec<u8>`. The plan's `fetch_to_file` (stream to a
`.part`, `Range` resume, free-space check) is **not** built; it needs a change in the shared
fetcher (D-004's reason). Consequences: a download is a transient memory spike of the file's size
(Inbound's Setup is 68.7 MiB; the per-file cap is the manifest's declared size plus one byte, the
parser accepts up to 8 GiB, and a file with no size is capped at 512 MiB); an interrupted
download starts again; the private-memory budget (30 MiB) holds at rest, not during an install.
Caustica (770 MB) is out of scope for this slice and could not be served this way. Everything
else of plan decision 5 stands: a file is hashed and sized before anything is placed or run, and a
bad download is deleted. Would change it: streaming in the shared fetcher, or a third catalog app
bigger than memory allows.

## D-006 No new dependency
Hub builds on what mdedit and mdview already build: `mdshared-*` pinned by rev (b4b21824, as
mdedit pins them), `winit =0.30.13`, `softbuffer =0.4.8`, `windows-sys` (features added, nothing
new in `Cargo.lock`), `bytemuck` (a direct dependency of `bins/hub` for the pixel buffer, as in
mdedit; already in `Cargo.lock` through the shared crates, so no new entry). SHA-256, version
comparison, selfupdate swap and fetching are the shared crates'. The tray, Run key, notifications
and process list are `windows-sys` (plan decision 10). Would change it: a Windows API unworkable
from winit's thread; then ask first and price it (CLAUDE.md rule 6).

## D-007 `managed-by`: a marker in Hub's own folder, not in the app's (proposed, not built)
Plan section 15 has Hub write `managed-by: hub` into the app's install folder so the app's own
updater stands down. That cannot be done for Sprawl (Program Files; Hub is never elevated) and
leaves a stray file in every install folder an uninstall or a folder swap must know about.
Proposal: `%LOCALAPPDATA%\Hub\managed\<app id>` (content `hub`), written by Hub when it installs
or adopts an app, deleted when it forgets one; each app's updater tests for it before looking
(APP-CHANGES.md). Replaces the file location of plan section 15, not its intent. Needs the owner
(plan question 3: should the apps stand down at all?) and `state.rs` to write the file.

## D-008 SHA-256 over TLS, no signatures in the first cut (plan 18 #4)
A key kept where the releases are made adds nothing. Would change it: the owner (question 5).

## D-009 No REST API (plan 18 #3)
Manifests and notes are release assets at `releases/latest/download/`; no rate limit, one
address per app. Consequence found while reading Inbound: `latest` must keep resolving to the
Windows release (APP-CHANGES.md, Inbound). Would change it: pre-releases (question 6).

## D-010 Hub never ends a process (plan 18 #7)
It posts `WM_CLOSE` to the app's visible top-level windows (mdedit and mdview: a `close` request
once it exists), waits up to 30 s, then says "waiting" and keeps waiting on the process; never
`TerminateProcess`. Would change it: nothing.

## D-011 Hub runs per user and never elevated (plan 18 #8, #15)
A machine-wide setup (Sprawl) runs through `ShellExecuteExW` `runas` at the owner's click, with
Windows' prompt every time; "update by itself" never applies to it.

## D-012 One previous version is kept per app for roll back (plan 18 #9)
For `files` installs the retired files; for `setup` installs a roll back is not offered by Hub
in this slice (re-running an older installer is the plan's idea; not built).

## D-013 Setups, not files, for Sprawl and Inbound; files for mdedit and mdview (plan 18 #6, #16)
The setup writes the uninstall entry, associations and registrations; Sprawl is in Program
Files; Inbound has several processes and a native-messaging registration, so its zip (which needs
`apply-update.ps1`) is never unpacked by Hub. `zip` is refused ("not supported").

## D-014 The window exists only while it is open; the loop sleeps (plan 18 #12)
Nothing polls; the first check is scheduled two minutes after start, never run at start.

## D-015 The catalog is four apps, compiled in (narrowing plan 18 #14)
mdedit, mdview, Sprawl, Inbound; the owner scoped this slice. NewsQueue, Postern and Caustica
are out for now (Caustica: D-005); thunderbird-jmap and Markdown Viewer C++ are out for good;
Spatial Commander is never in (its credits forbid sharing). `updates: self` stays in the format.

## D-016 A project reaches Hub only through public releases (plan 18 #17)
Hub sends no token; a private project is made public when it ships.

## D-017 Hub's own folder is `$HUB_DATA`, else `%LOCALAPPDATA%\Hub`
Created on demand, never at open; every write is atomic and under a lock file. Every test and
every run by an agent sets `HUB_DATA` (CLAUDE.md rule 4). The exe's own version resource wins over
Hub's record and over `DisplayVersion` when the three disagree (what is on disk is the truth).

## D-018 Closing the window keeps Hub in the tray (owner, 2026-11-22; refines D-014)
With the tray on (setting `tray`, default on, and no `--no-tray`) the window is created on demand
and destroyed on close; the process stays. "Quit Hub" in the tray menu (or `hub --quit`) ends it.
With the tray off, closing the window quits. Would change: an owner who wants the X to quit.

## D-019 The tray lives on a thread of its own, on a hidden top-level window
Not on the winit loop, and not a message-only window: a message-only window is not sent the
broadcast `TaskbarCreated`, so the icon would not come back after Explorer restarts. The thread
sleeps in `GetMessageW` (no timer), owns only a snapshot from the engine, and posts a command to
the winit loop. The menu is the system's (`TrackPopupMenu`); no tray or process crate.

## D-020 Start with Windows: the real Run key only for an installed exe
Value `Hub` = `"<path>\hub.exe" --tray` under `HKCU\...\Run`, off by default. A copy not under an
installed folder (cargo's release/debug/deps) refuses to write or delete it, so a development
build cannot put a build path into the owner's startup list. Test-hooks builds take
`HUB_RUN_KEY_PATH`, accepted only under `Software\HubTest\`. With `--tray` and the tray setting
off, `--tray` wins (a Run-key start still shows the icon).

## D-021 One balloon per check that found something; Hub's own update first
`3 updates: mdedit, Sprawl, Inbound`. Not again for what was told (a newer version of a told app,
or an app that went away and came back, is news again); memory is kept in `settings.ini`
(`notified`) and kept across a failed look. Never when the setting is off or no icon is up (told
later); focused window counts as told. Finished and failed updates are never notified. Click opens
the window. Hub's own update is listed first; "Update all (N)" counts apps only, the badge and
tooltip count Hub's too.

## D-022 Self-update: Hub's own `hub-update.txt`, checked first, swapped last
Format 1 of the shared updater, `uninstall_key` None, `HUB_UPDATE_MANIFEST` honoured. The URL
is one constant, `selfupdate::MANIFEST_URL` (`Kaan88/mdhub`, renamed from the working name hub-rs: plan question 1 is
unanswered). Downloads are pinned under the repository's releases path with no dot segments or
encoded separators. The exe is staged as `hub.exe.new`; the swap happens only on "Restart now" or
at quit, and only when no install runs (else deferred until the installs finish); `swap_in` puts
the old exe back on failure; restart uses `relaunch` and keeps `--tray` only if no window was
open. A staged exe not applied is deleted by `clean_up` at the next start and fetched again.
A copy that is not installed never asks for Hub's manifest.

## D-023 The settings card shows all three groups at once
General, Updates and About are all visible at once; the category list marks a group but no page
is swapped (the mock has pages). Would change: if the card grows past one screen.

## D-024 The icon has one drawing: `icon.rs`; `assets/hub.ico` is packed from it
The mark is a white hub joined by spokes to four nodes in the apps' colours (mdedit blue, mdview
teal, Sprawl orange, Inbound green: its teal pushed to green so two nodes do not read as one app
twice) on a dark indigo tile with a lighter rim, laid out in whole device pixels and mirrored, so
16 px is sharp. `tools/make-icon.ps1` runs the ignored test `icon::tests::dump_the_icon_sizes`
and packs its ten sizes as PNG entries; it draws nothing itself, so the file and the tray icon
cannot drift apart (before, System.Drawing and the painter each drew their own). Cost: the script
needs cargo, and the repository carries one `#[ignore]`d test that is a generator. With a count,
the badge takes the place of the fourth node. Replaces the blue tile with the download arrow.
Would change: an owner who wants another mark, or an icon file drawn by hand.

## D-025 The offer bar's yes is for one install; a question with no window opens one
"Close and update" lets the install it starts close the app, and nothing after it: the consent
goes when that install is cancelled or never queued, and the bar goes when its app is queued
another way. When the engine asks and there is no window (Update all in the tray menu with an
app open, a queued update whose app was opened meanwhile), the shell makes the window, since
only the bar can ask; closing the window on the question is "Not now". Refines D-010 and D-018.
Would change: an owner who prefers a balloon to a window appearing.

## D-026 A roll back is not started while its app is open
The bar asks about updates only, so a roll back of an open app says "Close <app> first, then roll
back" and starts nothing (at the click, and again at its turn in the queue). Before, it was the
one path that posted `WM_CLOSE` to an open app without a yes. Refines D-010 and D-012. Would
change: a bar that can ask about a roll back too.

## D-027 Each app shows its own icon; the catalog's colour and glyph are the fallback
The first slice drew a coloured tile with a generic glyph (copied from a mock); the plan (section
3.1) said each app's own icon, copied in at build time. One 128 px PNG per app is committed in
`assets/apps/` and compiled in: mdedit and mdview from the 128 px frame of their `.ico`, Sprawl from
`assets/sprawl-256.png` and Inbound from `assets/app.png` (both 256 px, downscaled with a bicubic
filter). Decoded lazily with the shared `decode_png_rgba`, scaled once per (app, side) into a
cache, blended with `fill_dev`; no new dependency, about 43 KB more in the exe, nothing on the
launch path. The copies are a snapshot: an app that changes its icon needs the file replaced in a
Hub release (no fetching of icons: Hub talks to GitHub releases only). Inbound's icon is line art
on a transparent ground, so on the dark theme its navy outline is faint. Replaces the generic tiles of the first slice (no earlier decision named them).
Would change: a request for icons that follow the apps' releases.

## D-028 The release notes view: one GET of `notes.file` when opened, cached by version, drawn with the shared painter
The owner asked for a changelog view that "could utilize the shared crate". It shows the notes of
the version Hub knows about (the latest of `hub.txt`) with the line `mdedit 1.2.0 -> 1.2.1` and
its date under them. Reached from the row menu ("Release notes"), the pane's "Release notes"
link, and the key `n`; Esc, Backspace or the back arrow return, the selection kept. It is a
page that takes the content area (title row, offer bar and status bar stay), not the pane grown,
because the notes are long and the pane is a strip under the list. Source: `hub.txt` `notes`
(always there, shown at once) and, when the manifest has `notes.file`, one GET of
`https://github.com/Kaan88/<repo>/releases/download/v<version>/<file>` through the same
`Transport` (64 KiB, 10 s, the catalog's `url_allowed`, a plain file name), started only when the
view is opened and the disk has no copy: never at start, at idle or in a check. The answer is kept
in `<data>
otes\<id>-<version>.md` (one file per app, an older version's removed), so opening
again, also after a restart, is offline. States: loading, loaded, failed (the reason, "Try again",
the one line stays), no notes file (the one line, and it says so). Rendering is Hub's own subset
reader in `bins/hub/src/changelog.rs` over `mdshared-ui` (`Painter`, `scroll`): no renderer crate,
no new dependency (D-006 holds). Everything fetched is hostile: lossy UTF-8, control and bidi
characters removed, an iterative parser with caps on lines, blocks, depth and length. No italic face
exists in the shared fonts, so italic is a softer colour; links are shown as text and are not
clickable (a "Release page" button opens the release on github.com/Kaan88 instead).
Would change: a markdown crate if the notes grow tables and images; a notes file fetched ahead of
time if the owner wants them offline for versions never opened; clickable links.

## D-029: an app whose own setup closes it is not refused for having no window to close

Found by the owner on 0.1.1 (2026-11-23): updating Inbound failed with "DownloadManager.Host.exe is
still open" and Sprawl the same. Both run as background or tray processes with no top-level window
(read-only look: Inbound's Host and Server, six `sprawl.exe`), so the install's polite `WM_CLOSE`
closes nothing and the install refused (`InstallError::Running`), as D-018's rule of never killing
requires. But both vendors' installers stop their own app: Sprawl's Inno `CloseApplications=yes`,
Inbound's `stop-for-setup.ps1` in `PrepareToInstall`. So the catalog says which apps do
(`setup_closes_app`: Sprawl and Inbound; mdedit and mdview are `files` installs and stay strict).
For such an app Hub still asks windows to close and waits up to 3 seconds, then runs the setup
(the owner's yes to "close it and update" is already given); the row says "The installer will
close <app>" and no longer "close it to continue". If that setup exits non-zero, the error is
"Setup could not close <app>: exit code N; quit it from its tray icon and try again", the old
version stays and the verified installer is kept for a retry. Hub itself still never ends a
process. Inbound's `DownloadManager.Server.exe` joins its wait list. Roll back stays strict
(D-026): it has no installer.
Would change: a setup that kills without asking (then the flag goes); an app that gains a quit
request Hub can send (then ask it instead).

## D-030 Purge joins the catalog, installed for all users like Sprawl
The owner asked for Purge (`Kaan88/purge`, an uninstaller that also removes what a program leaves
behind) in Hub's catalog. It is installed like Sprawl: Inno, `PrivilegesRequired=admin`,
`{autopf}\Purge`, so `folder` and `machine_folder` are both `Purge` and an install or update runs
through Windows' prompt (`scope: machine`). Uninstall key `{7C1B9E64-2D58-4F1A-9B03-6E4C5A8D7F21}_is1`
(its Inno `AppId`), exe `Purge.exe`, `wait` `purge-engine.exe` (a windowless child) and
`purge-broker.exe` (an elevated helper with no window). Neither helper can be asked to close and
Purge's setup has `CloseApplications=yes`, so `setup_closes_app` is set (D-029). No legacy
manifest. Its icon is the 256 px frame of its `.ico` downscaled to 128 px (D-027); the fallback
tile is the icon's blue with the `delete` glyph. Its first release, v0.1.0 (alpha, 2026-11-23,
`releases/latest`, not a pre-release), already carries `hub.txt` written with Hub's
`tools/write-hub-txt.ps1`, and `Purge.exe` has `FILEVERSION` 0.1.0.0; `tools/samples/purge/hub.txt`
is that file. So once a Hub release carries this entry, Hub can install, detect and update Purge.
Would change: Purge's release workflow writing a different exe, key or wait list than the sample
(then the catalog follows the release); the owner taking Purge out.

## D-031 Barbican is the sixth catalog app
The owner added Barbican beside Purge in the compiled catalog. This entry replaces D-015's "the
catalog is four apps" and the scope D-030 closes at five; neither older entry is edited. Barbican
(`Kaan88/barbican`) is a JMAP mail, calendar, contacts and files client that runs in the tray. A
per-user Inno install in `%LOCALAPPDATA%\Programs\Barbican` (unlike Purge and Sprawl, so no Windows
prompt), uninstall key its Inno `AppId` `{ACE04D99-4564-4E70-9A43-FE47A6A0E3B5}_is1`, exe
`barbican.exe`, `wait` is `barbican.exe` (its updater helper is a hidden PowerShell started from
it), `setup_closes_app` true (its installer closes the running tray app with `CloseApplications`,
the same reason as D-029). Own icon in `assets/apps/barbican.png`; fallback tile purple with the
`globe` glyph (the shared set has no mail glyph). The Hub tray/app icon keeps its four nodes
(D-024); it was not redrawn. No `tools/samples` entry and none possible: a sample carries a real
release's sizes and hashes, and Barbican has no release. Its installer work is in the Barbican
repository, branch `installer-inno`, unreleased, so until a release carries `hub.txt` the row shows
"no release information" and Hub never offers a wrong install. Its own updater does not stand down
for Hub (D-007 not built).
Would change: Barbican's release naming or AppId; a mail glyph in the shared set; a Barbican
release, which adds its sample to `tools/samples` and takes it out of `tests/samples.rs`'s
no-sample list.

## D-032 Buttons draw hover and press from the pointer the shell keeps
The owner saw no hover on any button. The `Vm` carries `pointer: Pointer { hot, held }`, the
`Hit` under the pointer and the one the button went down on; every button draws from it (the
shared `draw_win_button`, `OfferBarState` and `button_fill` where they apply, Hub's own `ui`
helpers for the row, tile and accent buttons). The engine keeps it and says when it changed, and
the window redraws only then, so a resting pointer costs nothing. A held control looks pressed only
while the pointer is still on it. The offer bar's X became a hit of its own (`Hit::BannerClose`,
the same command as No) so it lights alone. The "close it and update" question sets `warn: true`;
the warning-tinted bar it really wants is left to the owner (an opt-in variant of the shared offer
bar). Would change: the shared crate gaining a hover state of its own for these controls; the
owner choosing a different way to ask before closing an app.
## D-033 The window backdrop comes from shared; Hub's chrome is the title row, the command bar and the status bar
The owner asked for Mica and Acrylic "brought in from shared". Hub uses what mdedit and mdview
use, with nothing of it copied: `mdshared_app::backdrop::Backdrop` (the type, its settings value,
`TITLE`, `LABELS`), `mdshared_ui::frost` (`Frost::sync` decides when the chrome is frosted and calls
`platform::set_backdrop`; `mark` and `key_out` do the pixels) and `platform::backdrop_available`.
Nothing was missing in shared for this. The setting is `backdrop` in `[settings]` of
`settings.ini` (`off`, `mica`, `acrylic`; anything else is off), **off by default**, a choice row
"Window backdrop" in the General group of the card. Its help line is Hub's own, because the shared
one names a side pane Hub does not have.
What lets the backdrop through: the title row, the command bar (absent in the release notes view,
whose page takes its place) and the status bar, the three regions drawn in the flat chrome
colours. `view::draw_full` lists them in `Layout::chrome` as it draws them, so the marks follow
the layout and not a second copy of its arithmetic. Opaque: the list with its column heads (the
content people read), the pane under it (part of the list's content: notes and details of the
selected app, unlike mdedit's side pane of files), the offer bar (a call to act, drawn in its own
colours), the release notes page, the row menu and the settings card (drawn with opaque fills over
the chrome; the card's dimming leaves the chrome under it frosted, as in mdedit).
As the shared rule has it, the chrome is frosted only while the setting names a backdrop, the
system shows one (Windows 11 22H2 and later, transparency effects on, no high contrast, no
battery saver) and the window has the keyboard; otherwise it is the flat colours. With the setting
off, a frame costs one comparison, nothing is marked, no system call is made and every pixel is
what it was. The `Frost` is made anew with each window (a closed window is given back whole and the
tray holds none), so a new window is asked again.
A shot has no window and no compositor: `--demo backdrop-mica` and `backdrop-acrylic` draw the main
window keyed out exactly as the present does, laid over a made-up stand-in (a blue-to-rose
diagonal), to show which regions let it through. They are not what Windows draws.
Would change: the owner wanting the pane or the list frosted too (then `Layout::chrome` gains
them); a light Hub wanting no backdrop at all; shared changing which regions it calls chrome.

## D-034 The settings card is one page, with no category list
The owner (2026-11-24, from a picture of 0.1.4's card): "remove categories ... everything fix in one
page." The card uses the shared form's `single_page()`: General, Updates and About stay as headings
in the body, there is no list on the left, and the card is 600 px wide. Opening it at a group
(`settings::form`'s `page`, the tray's way in) scrolls to that heading, which on a card that fits
whole changes nothing. Replaces D-023, whose list marked a group but swapped no page. Would change:
the card growing past one screen, where a list to jump with earns its room again.

## D-035 Claude Switch is the seventh catalog app
The owner adds `Kaan88/claude-switch`, extending the catalog scope of D-031. It ships only
`claude-switch.exe` in its per-user Inno setup (`%LOCALAPPDATA%\Programs\Claude Switch`,
uninstall key `{4F2C369B-84AF-4A46-AE8B-D22103025F78}_is1`), with no helper exes in `wait`.
Closing Switch ends the live programs in its embedded terminals. Inno explicitly sets
`CloseApplications=no`, so Hub leaves `setup_closes_app` false: after consent it asks visible
windows to close and will not run Setup while Switch remains open (D-010, D-025; no D-029
fallback). The app asks before quitting with live tabs. If it hides to the tray, quit there.
The 128 px app icon is from `assets/logo-256.png`; the fallback is Claude clay and `code`.
Hub's own icon is unchanged (D-024). No release or sample yet; no legacy manifest or self-updater.
Would change: an installer path or AppId change, or an app-provided safe quit request.

## D-036 Pin shared at 5665039 and adopt its event-driven motion (2026-11-26)
Moves all four crates together from `b4b21824` to `5665039d08d1e97964685ac3ed6e050b69bac467`,
superseding D-006's pin but not its no-new-direct-dependency rule. The new lock entries are
`libloading 0.9.1` (the shared fetcher on non-Windows), `zune-jpeg` and `zune-core` (shared text).
Refines D-032: hover fades using shared motion, while pressed looks and action state remain
immediate. Menus and settings use shared presence; notes and views use a content-region
`PageTransition`; progress uses Fluent movement. Use the single-page form of D-034, not a new
settings navigation. The form's private dropdown cannot leave visibly on its own, so Hub keeps
the shared menu in a presence slot and hands its selection back to the form.
Refines D-014: the loop still sleeps; an animator deadline joins the same one-shot wake as checks
and close. Reduced motion uses the system setting; shots force settled endpoints. No repeating
animation and no change to the tray's system menu or balloons. Would change: shared exposing
dropdown presence through `Form`, or the owner asking for different motion.

## D-037 Hub remembers its normal window rectangle, and the default is 860×660
Refines D-017 (Hub's own files) and D-018 (destroy the window when closing to the tray).
`settings.ini` holds `[window]` `placement=x y width height maximized`, with a signed physical
desktop origin, logical client width and height, and `0`/`1` for maximized. A different DPI can
then change how many device pixels the client takes without changing its logical size. Moves and
resizes update memory; close-to-tray and normal exit save under the existing store lock. A minimized
window saves its last visible state, not an iconic rectangle. Nothing polls.
Restore needs a reachable part of the draggable title bar in a current monitor's work area; if
none fits, use the unmaximized default centered on the primary work area. A restored size is
bounded by the destination work area and the existing 520×360 logical minimum. `--size` ignores
and does not replace saved placement; demos and shots do not use it. The extra 60 logical pixels
let the seven-app list show its details pane above the status bar; no padding change was needed.
Would change: evidence that a visible title bar is not enough to recover the window after a
display change, or a catalog large enough to need a different default layout.

## D-038 A pointer button activates on release over the same semantic target
Refines D-032/D-036: press lights the control; moving off it and releasing cancels. Dragging the
title, resizing and scrollbar track/thumb gestures still start on press. Row actions are keyed by
app ID and action, and offer controls by owner, release version and issuance, not by a row index
or the painted Yes slot. Resolve the release against the painted frame and the live engine;
focus loss or target replacement disarms it. This preserves D-025: a replacement prompt never
inherits consent. Row menus likewise require the same choice at press and release. Would change:
an explicit new gesture that needs to act on press.

## D-039 Notes owns its keyboard; the app filter stays available at every window width
Refines D-028: notes keeps the background selection, drops filter focus and consumes keys before
the hidden app list. Tab and Shift+Tab reach Back, Try again when available, and the release page;
Enter activates only the focused notes control. Ctrl+F or the compact search button opens the
filter below 900 logical px; Clear stays visible while a query is active and Esc in the field
clears before blurring. The filter affects what is shown, not the scope of Update all (D-021).
Would change: a different keyboard focus model across the notes and list or a visible-only bulk
action with a separate name.

## D-040 The app-close offer uses Hub's warning colours
Replaces D-032's deferred warning tint. Hub passes a local palette copy to the shared offer painter,
using `warn_bg` for the background and `warn` for the muted ink on that offer only. The wording
states the consequence (“Updating … will close it.”); the consent buttons and Hub's restart offer
are unchanged. Would change: shared offering the same opt-in warning style.
