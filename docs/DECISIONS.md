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
