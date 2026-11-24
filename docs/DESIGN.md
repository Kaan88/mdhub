# Design

How each part is built, one section per part, written from reading the code. First screen:

```
                      catalog (6 apps, compiled in)
                         |
 transport ----> check ----> manifest (hub.txt)         state (Hub's own files)
   (bytes)    what is new?    parse / from_legacy        installed.ini, settings.ini,
      |            |                  |                   checks.ini, latest\<id>.txt, cache\
      v            v                  v                          ^
   install  <------ model (Installed, Latest, Row) ---- detect (what is on this PC)
 (download, verify, place or run setup, launch, roll back)
                         |
 bins/hub:  glue (Engine: owns what Hub knows, runs work on threads, builds the Vm)
            vm -> view / grid / panes / ui (draw a Vm, return a Layout of hit rects)
            app (winit + softbuffer window), shot (--shot), demo (fake Vms), cli, trace
```

`hub-core` has no window and no thread of its own (workers are `bins/hub/src/work.rs`); the
window draws a `Vm` and returns what a pointer can hit; `glue.rs` is the only place that joins
them, so every behaviour is tested without a window.

## catalog.rs
Six `CatalogApp` constants (mdedit, mdview, Sprawl, Inbound, Purge, Barbican): id, name, blurb, repository under `Kaan88`, exe (relative; Inbound's
is `app\DownloadManager.Host.exe`), launch args (`--open` for Inbound), folder under
`%LOCALAPPDATA%\Programs`, machine folder (Sprawl, Purge), Inno uninstall keys, `wait` (executables that
must be gone before an install), `setup_closes_app` (Sprawl, Inbound, Purge, Barbican; D-029), the legacy manifest file name (mdedit, mdview), tile colour and
glyph. `latest_url(file)` and `url_allowed(url)` (a manifest may name only
`https://github.com/Kaan88/<repo>/releases/download/...`). Adding an app is an entry and a
release of Hub.

## model.rs
The types the others share: `Installed {folder, exe, version, source, kind}` with `Source`
(Hub's record, an uninstall entry, a folder), `Kind` (Setup, Files, Zip), `Scope` (User,
Machine), `FileSpec`, `SetupSpec`, `PlatformEntry`, `HubManifest`, `Latest` (Unknown,
NoManifest(reason), Found), and `Row`, one row's state for the window (NotInstalled, UpToDate,
Update, External, Downloading, Verifying, Waiting, Failed, Done, NoInfo).

## manifest.rs
`parse(bytes, app)`: at most 16 KiB and 128 lines; `key: value` split at the first colon, both
sides trimmed, `#` comments and blank lines ignored; `hub: 1` first; no duplicate keys; no unknown
key (platform-prefixed keys of the six known platforms are kept: `windows-x86_64.kind`, `.exe`,
`.file`..`.file9` with `.url/.sha256/.size`, `.zip`, `.setup` with `.silent/.scope`, `.uninstall`,
`.min`, `.wait`, `.updates`); https only; every URL under the app's own releases, no `..`, `%`,
`?`, `#`, `\`, `@`; lower-case 64-hex SHA-256; plain file names (no device names, no trailing dot);
`exe` and `wait` relative with no `..`; `silent` from `[A-Za-z0-9 /=_.:-]`; the kind's own needs
(a `setup` kind needs a setup, `files` needs a file and an exe); duplicate file names, compared
case-insensitively, refused. Never indexes, unwraps or slices on input (release aborts on a
panic). `from_legacy` (D-003) and `write` (a manifest back to text such that `parse(write(m)) ==
m`; the tests and the samples use it).

## state.rs
`Store {dir}`, `Store::open()` (`$HUB_DATA`, else `%LOCALAPPDATA%\Hub`; created on demand),
`Store::at(dir)` for tests. `installed.ini` (a section per app: folder, exe, version, kind, date),
`settings.ini` (`view`, `interval_hours` default 6, `backdrop` default off, `[skipped]`), `checks.ini` (last attempt,
failure count), `latest\<id>.txt` (the cached manifest text, so the window has rows with no
network), `cache\<id>-<version>` and a `rollback_dir`. Every read-modify-write holds
`.hub.lock`, a stale lock is broken, temp files are named per process and write, and writes
rename. A file over its limit or damaged reads as empty; nothing panics.

## detect.rs
The plan's 3.2 order: Hub's record (while its exe still exists), then the uninstall entries
(`HKCU` and `HKLM`, native and `WOW6432Node`: `InstallLocation`, `DisplayVersion`), then the
folders installers use (`<programs>\<folder>`, each Program Files `machine_folder`), then the exe's
version resource (`GetFileVersionInfoW`, never by running it); the resource wins over the record
and `DisplayVersion`. `Env` names where to look; `Env::custom` points it at scratch folders and a
test registry path. `running(exe)` and `any_running` read the process list and compare full
paths case-insensitively. `adopt` records a found install without touching the app. Windows
calls are `#[cfg(windows)]` with a harmless stub elsewhere.

## transport.rs
`trait Transport { get(url, max_bytes, timeout) -> Result<Vec<u8>, String> }`. `Net` is
`mdshared_fetch::fetch_file` (https only, redirects followed by hand, only a 200 counts).
`FileRoot` stands in for github.com with a folder (`https://github.com/X/Y` reads `<root>/X/Y`;
`..`, drive letters, device names, backslashes refused). `default_transport()` returns `FileRoot`
when the `test-hooks` feature is on and `HUB_FETCH_ROOT` is set; a release has no such feature.
Whole bodies in memory: D-005.

## check.rs
`check_one(app, t)`: GET `hub.txt` (64 KiB, 10 s); if absent and the app has a legacy manifest,
one more GET and `from_legacy`; the answer is `Found`, or `NoManifest(reason)` (a wrong `app`, a
parse error, a 404). `check_all` asks at most two at a time, caches successes in the store, sets
the last attempt and the failure count, returns the cached row for a failed app. `due` and
`next_due`: the interval (6 h, 0 = never) is measured from the last attempt; after a failed run
retry in 30 minutes, doubling up to the interval; a clock that went backwards is due.
`row_state(installed, latest, skipped)`: NotInstalled, UpToDate, Update (only if latest >
installed; an unknown installed version is an Update), External (found, not recorded by Hub,
until adopted), NoInfo. `age_label` ("Checked 4 minutes ago").

## install.rs
`install(app, manifest, current, store, transport, opts, progress)`. **Files**: every file is
downloaded to `cache_dir` (`.part`), its size and SHA-256 checked, all of them, before anything
is placed; staged beside the targets; `mdshared_app::selfupdate::swap_in`; the retired files kept
in `rollback_dir` with a note. **Setup**: downloaded, verified, run with the manifest's silent
arguments (a user scope as a plain process, a machine scope through `ShellExecuteExW` `runas`),
waited on, the exit code read (0 is success), then re-detected and recorded. **Zip** is refused.
Before placing, running programs (`wait` and the exe) are asked to close with `WM_CLOSE` through
a `Closer` (a test supplies one), waiting up to 30 s, else `InstallError::Running`; never
`TerminateProcess`. Every failure leaves the old version and deletes the bad download; `Cancel`
is an `AtomicBool` read between files. `InstallError`: Unsupported, Invalid, Cancelled, Download,
Mismatch (expected and received), Running, Io, Setup (with the exit code), NoRollback. `rollback` puts a Files install's retired
files back. `launch` starts the exe detached, in its folder, with `launch_args`. Tests use small
programs the test builds and a `SetupRunner`, never a real installer.

## bins/hub: vm.rs, view.rs, grid.rs, panes.rs, ui.rs
`Vm` is everything the window shows (apps with their `Row`, versions, date, size, notes, details;
selection; view; offer bar; status text; a busy text); `draw(painter, palette, w, h, vm) ->
Layout` paints the title row, command bar, the list or the grid (large tiles carry versions,
status and one action; small tiles an icon, a name and one word), the pane under it ("What's
new" and the details of the release) and the status bar, and returns every hit rect. Drawn with
`mdshared-ui` (frameless title, buttons, theme `LIGHT`/`DARK`). Every number is a constant.

`appicon.rs`: each catalog app's own icon (`assets/apps/<id>.png`, 128 px, `include_bytes!`).
`ui::app_tile` calls `appicon::draw`, which decodes the PNG on the first draw that needs it
(`decode_png_rgba`), scales it to the tile's side in device px (area average with alpha
weighting when shrinking, bilinear when growing) into a cache keyed by (app, side), and blends it
pixel by pixel with `Painter::fill_dev`, so the painter's clip holds. `dim` (not installed)
fades it and pulls it to grey. A PNG that does not decode returns `false` and `ui::tile` draws
the catalog's colour and glyph; the title bar's own mark still uses the glyph tile. The badges are
drawn over the icon as before.

Hover and press (D-032): `Vm.pointer` is a `Pointer { hot, held }`, the `Hit` under the pointer
and the one the left button went down on. Every button draws from it through `Pointer::look(hit)`:
the window buttons and the offer bar through the shared `draw_win_button` / `OfferBarState`, the
command bar's buttons, the sliders button, the view switch and the notes page's back arrow through
`button_fill` (`ui::quiet_fill`), the accent buttons a step towards the page (`ui::accent_fill`,
as the shared offer bar does), the row and tile buttons (`ui::sbtn`) a step past `pal.hover` with a
stronger frame (the row under them is lit too), the "..." menus a step past their row or tile
(`ui::icon_fill`); the pane's "Release notes" link is underlined. A held control looks pressed only
while the pointer is still on it. The offer bar's X is a hit of its own (`Hit::BannerClose`, the
same command as its No) so it lights alone. The engine keeps the pointer (`set_hot`, `set_held`,
true only on a change); `app.rs` sets it on a move (none on a resize edge, under the card or a
menu), on leaving, on press and release, on losing focus, and after each frame, and redraws only
when it changed: nothing runs while the pointer rests. The close question (`glue::close_question`)
is a warning: `warn: true`, the offer bar's warning sign in place of the download arrow.

## bins/hub: demo.rs, shot.rs
`hub --shot out.png [--size WxH] [--scale N] [--light|--dark] [--view list|large|small] [--demo
NAME]` renders a `Vm` with no window and no read of this PC. `demo.rs` builds the made-up Vms
(`main, first-run, updating, failed, done, noinfo, running, banner, all, empty`, the notes and
settings states, and the pointer states `hover-close hover-min hover-bar hover-primary
pressed-primary hover-open hover-menu close-question close-question-yes close-question-no`);
`tests/shots.sh`
renders all of them. A shot is limited to 64 million pixels. `backdrop-mica` and
`backdrop-acrylic` are the main window with its chrome keyed out over a stand-in backdrop
(`frost::preview`); every other state is drawn with the backdrop off.

## bins/hub: glue.rs, work.rs, app.rs, cli.rs, trace.rs
Written after the pieces above, by another hand; read here only for their headers, not reviewed
(STATE.md). `glue.rs`: the `Engine` that owns what Hub knows, turns a hit or a key into work, runs
one install at a time on worker threads (`work.rs`) and builds the `Vm`; `HUB_PROGRAMS` /
`HUB_PROGRAM_FILES` stand in for the programs folders and `HUB_FETCH_LOG` records the URLs asked
for, both for tests. `app.rs`: winit and softbuffer, a frameless window, a loop that sleeps (no
CPU at idle). `cli.rs`: `--behind --no-session --exit-after-ms --trace --demo --check-now --size
--light/--dark --view`; `HUB_FIRST_CHECK_MS` sets the first check's delay (default 2 minutes).
`trace.rs`: `--trace` prints stage times, memory and CPU time.
Closing an app: an install of an open app waits for the offer bar's yes, which covers that one
install only (D-025); a roll back of an open app is not started (D-026); when the bar asks and
there is no window, `app.rs` makes one (`tray::wants_window`), and closing it answers "Not now".
A check is told which version of Hub is already downloaded; the engine remembers what it told
(`Own::told_ready`) and shows "Downloading" only for a download the check really makes.

## notes.rs (hub-core), changelog.rs (bins/hub): the release notes view
`notes::url(app, version, file)` builds `https://github.com/Kaan88/<repo>/releases/download/v<version>/<file>`
and refuses a file name that is not plain (`manifest::valid_name`), a version with odd characters,
or an address `CatalogApp::url_allowed` does not allow. `notes::fetch` makes the one GET (64 KiB,
10 s) and `notes::clean` turns the bytes into text (lossy UTF-8, no CR, control, bidi or
zero-width characters, at most 64 KiB); a GET error becomes a plain reason ("... (404)", "bigger
than 64 KiB"). `Store::cached_notes/set_cached_notes` keep the raw bytes in `notes\<id>-<version>.md`.
The engine (`glue.rs`) holds `notes: Option<NotesOpen>`: `Cmd::OpenNotes` (pane link, row menu,
key `n`) fills it from the manifest Hub knows and the disk, else starts `work::spawn_notes`, whose
`Event::Notes {id, version, result}` is taken only by a view still waiting for exactly that
version (a closed or moved view drops it; the worker has cached it anyway). `Cmd::NotesRetry`
fetches again from Failed only; `Cmd::NotesPage` opens the release page; Esc and `NotesClose` leave.
`Vm.notes: Option<NotesVm>` makes `view::draw_full` draw the page instead of the command bar, list
and pane; `Layout` reports `notes_max_scroll` and `notes_view_h` and new hits `Notes(i)`,
`NotesBack`, `NotesRetry`, `NotesPage`, `NotesTrack`, `NotesThumb` (the thumb front-most), and
`notes_bar` (track, thumb) as drawn. `app.rs` keeps the page's own scroll (the list's is kept
while it is open): wheel, arrows, PageUp/PageDown/Space, Home/End (`changelog::scrolled`, pure).
The scrollbar, from the last frame's `Layout` (pure, in `changelog.rs`): `bar_press` turns a press
on the thumb into `Grab(dy)` (where it was taken) and one on the track off the thumb into a page
toward it; `bar_drag` puts the thumb's top at pointer y less dy and turns that into the text's
offset with the shared `scroll::offset_for_thumb`, clamped, None on a closed page or a non-finite
number. `app.rs` holds `ngrab` from the press to the release (or the window losing focus), and
sets `NotesVm.bar_active` before each frame while the pointer is on the bar or drags it: the
shared bar is then drawn wide (as mdedit's).
The page: header (back arrow, "Release notes of X", the version), the scrolling body in a column
at most 740 px wide with the shared overlay scrollbar, a footer with `mdedit 1.2.0 -> 1.2.1`, the
date and "Release page".
`changelog.rs`: `parse` (headings `#`..`###`, bullet and numbered lists with nesting up to 3 and
continuation lines, paragraphs, fenced code, rules, table rows as mono lines; the rest is text),
`inline` (bold, italic, code, links as their text, escapes; an opener with no closer is text;
`snake_case` is not italic), `layout` (words measured once, wrapped to the column, a word wider
than the column cut by characters, runs of one style merged, inline code with a fill) and `laid`
(the layout kept for the text, width and scale), `draw_page`. A scroll frame walks the laid
primitives and draws those in view. Limits: 20,000 lines, 3,000 blocks, 1,500 characters a line,
300 lines a code block, 4,000 characters an item, depth 3. Shots: `notes`, `notes-loading`,
`notes-failed`, `notes-nofile`, `notes-long`, `notes-bar-active` (the long page, its bar under
the pointer), list view only (the view does not depend on it).
Not covered: a track held down repeating its page, an image or a table drawn as a grid, a link
that opens.

## tools/
`write-hub-txt.ps1` writes a `hub.txt` from a version, app, repository, notes and a list of files
(name, role file|setup|zip, optional asset name, silent arguments, scope): SHA-256 and sizes from
the files, URLs `https://github.com/Kaan88/<repo>/releases/download/v<version>/<asset>`, and
refuses (exit 1, nothing written) whatever the parser would refuse. Windows PowerShell 5.1 and
7. A test (`manifest::tests::the_release_script_writes_only_what_this_parser_reads`) runs it under
`powershell.exe` and parses what it wrote. `make-samples.ps1` regenerates `tools/samples/<app>/hub.txt` from GitHub's own sizes and hashes
of each project's latest release.

## autostart.rs (hub-core)
"Start with Windows": value `Hub` under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`,
data `"<exe>" --tray`. Every function takes the key path and value name, so tests use
`HKCU\Software\HubTest\<unique>`; only `enable`/`disable`/`is_enabled` name the real key. The
exe path must be absolute with no quote, `%` or control character. `remove_test_key` deletes a
test key tree and its `HubTest` parent when empty, and refuses anything outside
`Software\HubTest\<name>`. `glue.rs` decides which key a build gets (D-020).

## selfupdate.rs (hub-core)
Hub's own update on the shared `mdshared_app::updater` (format 1, one exe). `check` reads
`MANIFEST_URL`; `stage` downloads and verifies into `hub.exe.new` and replaces nothing;
`apply_when_idle` swaps only when `busy` is false; `restart` starts the new exe with `relaunch`.
`under(url, prefix)` pins downloads (D-022). The engine (`glue.rs`) holds a `Status` (None,
Downloading, Ready, Failed); Ready shows the banner "Hub X is ready: restart to finish" with
`Cmd::HubRestart` / `Cmd::HubLater`, and the status bar "Hub 0.1.0 (0.2.0 ready)". Quitting also
swaps a downloaded Hub in. No new check starts while Hub's own download runs.

## notify.rs (hub-core)
Text and the rule, no Windows call. `Pending` (name, version, own), `Memory::decide` (what is
news), `decide_keeping` (what was told about something not seen this time is kept, so a failed
look does not make it news again), the balloon body (at most 4 names, then "+N more"), the
tooltip. Limits follow `NOTIFYICONDATAW` (body 200, tip 120 UTF-16 units).

## tray.rs, icon.rs (bins/hub)
`tray.rs`: pure half (`menu` for a snapshot, `classify` a click, `step` the window's life) and
`os` (Windows): a thread with a hidden top-level window and a `GetMessageW` loop, the icon
(`Shell_NotifyIconW`), the system menu, the balloon (`NIF_INFO`), `TaskbarCreated` re-adds the
icon. A start that times out sets `cancelled` and leaves no thread. A balloon waits for an
icon instead of being dropped. `icon.rs`: the tile and the badge are drawn with the shared
painter twice (over black and over white) and the alpha solved from both, digits from a 3 x 5
pixel font, one `HICON` per change of the count. The mark is a white hub joined to four nodes in
the apps' colours on a dark tile, laid out in whole device pixels and mirrored, so 16 px is
sharp; the badge takes the place of the fourth node. It is the only drawing: `tools/make-icon.ps1`
packs `assets/hub.ico` from this module's output (`-Sheet file.png` for a sheet to look at). Not covered: the real Explorer restart and the
icon seen live (CLAUDE.md rule 6).

## settings.rs (bins/hub)
The rows of the shared `mdshared_ui::settings::Form`: Start Hub with Windows, Show an icon in the
tray, Tell me when updates are found, Look for updates (Every hour / 6 hours / day / week /
Never), About line and "Update Hub" (becomes "Restart now" once ready). The engine applies and
saves a change (`state.rs`); the tray menu changes the same settings and the card is rebuilt
when its rows differ. Opened by the sliders button (`Hit::Settings`). Shots: `settings-general`,
`settings-updates`, `settings-about` (the last over the banner state).
"Window backdrop" (Off / Mica / Acrylic, General group, `state.rs` `backdrop`, off by default) is
the shared `mdshared_app::backdrop::Backdrop`; the window follows it (`frost.rs` below).

## frost.rs (bins/hub): the window backdrop
Hub's part of the shared backdrop (D-033); the rest is `mdshared_ui::frost`. `view::draw_full`
lists the chrome it drew in `Layout::chrome`: the title row, the command bar (not in the notes
view) and the status bar. `app.rs` keeps a `Frost` per window and calls `Frost::sync` at the
start of each frame with the setting, the window's focus and the palette (it asks
`backdrop_available` and `set_backdrop` only when the setting or the focus changed; off, one
comparison). While `Frost::key()` is `Some`, `mark_chrome` stamps the chrome's alpha byte after
the view is drawn and before the row menu or the card, and `present` runs `key_out_areas` over the
window's copy of the frame (the bitmap keeps what was drawn). A focus change redraws only when
the setting concerns the frost. `preview` lays a keyed-out frame over a made-up stand-in for the
shots `backdrop-mica` and `backdrop-acrylic`; `shot::render_with(.., Backdrop::Off)` is
`render`.
