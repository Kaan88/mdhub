# What an app must do to work with Hub

**Read this first if you are an agent (or a person) working in another repository** (mdedit, mdview,
Sprawl, Inbound, Purge, or a later app) and the task says "for Hub", "hub.txt", "make it work with the
launcher". This is the contract. It is kept true: if the code in `crates/hub-core` disagrees with
this file, the code is right and this file is a bug, so say so in your pull request.

Hub (`Kaan88/mdhub`) is a tray launcher and updater. It shows the owner's apps, finds what is
installed, checks for updates, downloads and installs them, and starts them. It reads **only public
GitHub releases**, sends no token, and installs only apps compiled into its catalog (section 7).
Windows first; the format has room for Linux and macOS, but Hub reads Windows today.

## 1. The checklist

An app is Hub-compliant when all of these are true. Items marked (Hub) are done in this repository,
not in yours.

1. Its **release carries a `hub.txt`**, written by the release workflow from the files it uploads
   (section 2, section 3). The latest release is the one `releases/latest` resolves to.
2. Every file the manifest names is **a release asset of the same release**, with the SHA-256 and
   size of the uploaded file.
3. Its **executable has a version resource** (`FILEVERSION`/`ProductVersion` = the release version).
   Hub reads the exe's own version before anything else (section 5).
4. Its **installer is Inno Setup** with a stable `AppId`; the uninstall key `<AppId>_is1` is what
   the catalog names (section 5), and its `DisplayVersion` is right after every install and update.
5. Its **silent install works**: exit code 0 on success, no window, no prompt (section 4).
6. If it runs in the tray or in the background (no window), **its installer stops it itself**
   (Inno `CloseApplications=yes` or a stop script). Hub never ends a process. Tell the owner so the
   catalog flag `setup_closes_app` is set (Hub).
7. It has an entry in **Hub's catalog** (Hub; section 7).
8. Its own updater **stands down** when Hub manages it (not built yet: section 6).

## 2. Where Hub reads, and what "latest" means

- The manifest: `https://github.com/Kaan88/<repo>/releases/latest/download/hub.txt`. It is a plain
  download, not an API call, so there is no rate limit and no JSON.
- `releases/latest` names the **newest release that is not a draft and not a pre-release**. So:
  a release created as a draft is invisible to Hub until it is published; a repository that also
  publishes other kinds of releases (Inbound's `android-v*` tags) must keep those as pre-releases
  or Hub's `latest` may become the wrong release; tags are `v<version>` (`v1.2.3`).
- Optional: `notes.file`, a Markdown asset of the **same release** (at most 64 KiB), fetched only
  when the owner opens the release notes page. Without it Hub shows the one-line `notes`.
- Hub fetches `hub.txt` at most every 6 hours per app, 64 KiB cap, 10 s deadline.

## 3. The format of `hub.txt`

The family's `key: value` grammar. Strict: an unknown key, a bad value or a duplicate refuses the
**whole** manifest and Hub shows "no release information". The reference is
`crates/hub-core/src/manifest.rs` (header comment and `parse`); the writer a workflow should use is
`tools/write-hub-txt.ps1` (section 3.1).

```text
hub: 1
app: sprawl
version: 1.0.5
date: 2026-11-20
notes: Faster scan of network drives; the size column sorts again.
notes.file: release-notes-1.0.5.md
windows-x86_64.kind: setup
windows-x86_64.exe: sprawl.exe
windows-x86_64.uninstall: Sprawl_is1
windows-x86_64.wait: sprawl.exe
windows-x86_64.setup: Sprawl-Setup-1.0.5.exe
windows-x86_64.setup.url: https://github.com/Kaan88/sprawl/releases/download/v1.0.5/Sprawl-Setup-1.0.5.exe
windows-x86_64.setup.sha256: a268e03bb2d0150bd8b8b5725b738b312218ebe99d7429099d62b9c488f60196
windows-x86_64.setup.size: 5544567
windows-x86_64.setup.silent: /VERYSILENT /SUPPRESSMSGBOXES /NORESTART
windows-x86_64.setup.scope: machine
```

Examples for every catalog app that has a release to take them from: `tools/samples/<app>/hub.txt`
(Purge's is its v0.1.0 release's own `hub.txt`; Barbican has none until it has a release).
`crates/hub-core/tests/samples.rs` checks that each one parses, and that the only app without a
sample is the one it names.

| Key | Rule |
|---|---|
| `hub: 1` | First meaningful line. Any other number is refused. |
| `app` | The catalog id (`mdedit`, `mdview`, `sprawl`, `inbound`, `purge`, `barbican`). Must match. |
| `version` | `X.Y.Z` or `X.Y.Z-pre`. Hub compares versions SemVer-style. |
| `date` | `YYYY-MM-DD`, the release day. |
| `notes` | One line, at most 1024 **bytes**, no control characters. |
| `notes.file` | A plain file name of an asset of the same release. |
| `P.kind` | `setup` (run the installer) or `files` (place executables). `zip`, `appimage`, `app` are parsed but not installed yet. |
| `P.exe` | The executable Hub starts, relative to the install folder (`app\Host.exe` is fine). |
| `P.uninstall` | The uninstall subkey name (`{GUID}_is1`). |
| `P.wait` | Comma-separated exes (relative) of the app that must be gone before an install. List **every** process of the app, helpers included. At most 8. |
| `P.file`, `P.file2` .. `P.file9` | A file to place (kind `files`): each has `.url`, `.sha256`, `.size`. |
| `P.setup` | The installer: `.url`, `.sha256`, `.size`, `.silent` (arguments), `.scope` (`user` or `machine`). |
| `P.min`, `P.updates` | Parsed and ignored today. Do not rely on them. |

`P` is one of `windows-x86_64`, `windows-aarch64`, `linux-x86_64`, `linux-aarch64`,
`macos-aarch64`, `macos-x86_64`. Hub reads `windows-x86_64`.

Limits and refusals (all enforced; `write-hub-txt.ps1` repeats them so a bad release fails in the
workflow, not on a PC):

- At most 16 KiB and 128 lines. Lines end in `\n`. `#` starts a comment line.
- Every URL is `https://github.com/Kaan88/<repo>/releases/download/...` for **this app's own
  repository**, at most 512 bytes, no spaces, no `%`, `#`, `\`, no user info. Anything else refuses
  the manifest.
- `sha256`: 64 lower-case hex digits. `size`: bytes, at most 8 GiB; Hub checks both before it
  places or runs anything, and again after the download.
- File names: letters, digits, `.`, `_`, `-`, at most 128, not starting or ending with `.`, not a
  Windows device name (`CON`, `NUL`, `COM1`...), unique ignoring case.
- `silent` arguments: at most 200 bytes from `A-Z a-z 0-9 space / = _ . : -` only.
- At most 9 files. A `setup` entry needs both `.silent` and `.scope`.

### 3.1 Writing it: `tools/write-hub-txt.ps1`

Each app repository keeps a **copy** of Hub's script in its own `tools/`, with Hub's commit in its
first line, so a release never depends on another repository being reachable. Re-sync the copy when
Hub's script changes (`git log -- tools/write-hub-txt.ps1` in Hub). Run in the release workflow,
**after** the release files exist, on the **uploaded** files:

```powershell
$files = @(
  @{ Name = 'Sprawl-Setup-1.0.5.exe'; Path = 'dist/Sprawl-Setup-1.0.5.exe'; Role = 'setup';
     Silent = '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART'; Scope = 'machine' }
)
# In-process (&), not a child `pwsh -File`: the file list is hashtables, which a child gets as text.
& ./tools/write-hub-txt.ps1 -App sprawl -Repo sprawl -Version $v -Notes $oneLine -NotesFile "release-notes-$v.md" `
    -Kind setup -Exe sprawl.exe -Uninstall 'Sprawl_is1' -Wait sprawl.exe -Files $files -Out dist/hub.txt
```

It computes SHA-256 and size from `Path`, builds the URLs from `-Repo` and `-Version`, and exits 1
writing nothing if anything would be refused. It runs under PowerShell 7 and Windows PowerShell 5.1.
Upload `dist/hub.txt` as an asset named exactly `hub.txt`, and the notes file under exactly the name
passed to `-NotesFile`.

## 4. The installer, and what Hub does with it

**First install and every update of a `setup` app**: Hub downloads the installer, verifies it,
runs it with the manifest's `silent` arguments, waits for it, and reads the exit code: 0 is
success, anything else is a failed update (the old version stays, the verified installer is kept for
a retry). Then Hub looks for the install again and records it.

- **Per user is preferred** (`PrivilegesRequired=lowest`, `%LOCALAPPDATA%\Programs\<name>`); that is
  `scope: user`, and Hub needs no administrator rights.
- **`scope: machine`** (Sprawl and Purge, in `Program Files`): Hub starts the installer elevated, so Windows
  shows its consent prompt on every update. Hub itself is never elevated.
- **The app must not be a blocker.** Hub asks the app's visible windows to close (`WM_CLOSE`),
  waits a few seconds, and never ends a process. An app with **no window** (tray or background
  processes) cannot be asked, so **its installer must stop it**: Inno `CloseApplications=yes`, or a
  stop script in `PrepareToInstall` (Inbound). Hub then runs the setup anyway for apps flagged
  `setup_closes_app` in its catalog (decision D-029) and reports the exit code if the installer
  could not close it.
- **Silent means silent**: no dialog, no `pause`, no browser, no "Start now?" page (use
  `skipifsilent` on `[Run]` entries that open the app). An installer that ignores its silent
  arguments cannot be updated by Hub; the app then shows "Open the release page".
- An installer that installs **into another folder** than the previous one confuses detection; keep
  `DefaultDirName` and `AppId` stable across versions. Inno installs an older version over a newer
  one without complaint (roll back relies on it).

**Update of a `files` app** (mdedit, mdview): Hub downloads every file, verifies size and SHA-256
**before anything is placed**, then stages each next to its target and swaps them all or none
(`mdshared_app::selfupdate::swap_in`); the old files are kept for roll back. The app must be
closed first: Hub asks its windows to close (a `close` request in the sister protocol is planned),
and refuses to continue while any of its `wait` exes still runs.

## 5. How Hub finds an app and its version

In this order, the first that answers wins (`crates/hub-core/src/detect.rs`):

1. Hub's own record (`%LOCALAPPDATA%\Hub\installed.ini`), for apps Hub installed or adopted.
2. The uninstall entry under `HKCU` / `HKLM` `...\CurrentVersion\Uninstall` (and `WOW6432Node`),
   by the key the catalog names (`{AppId}_is1`): `InstallLocation`, `DisplayVersion`.
3. The folders installers use: `%LOCALAPPDATA%\Programs\<folder>`, `Program Files\<name>`.
4. The **exe's version resource**, which is read first for the version whichever step found the app.

So: **the exe needs a version resource** (`FILEVERSION` and `ProductVersion` = the release
version; in Rust, a `VERSIONINFO` block in `build.rs` as mdedit's and Hub's do). Without it Hub
falls back to `DisplayVersion`, which an app's own updater leaves stale. Hub never runs an app to
ask its version.

An app found by steps 2 to 4 and not recorded by Hub shows "Installed elsewhere" and gets no update
offer until the owner presses **Adopt** (which only records it; nothing of the app is touched).

## 6. The app's own updater

mdedit and mdview update themselves. If Hub and the app both offer the same update, two installers
may run. The planned rule (D-007): Hub writes `%LOCALAPPDATA%\Hub\managed\<app id>` (content `hub`)
when it installs or adopts an app, and the app's updater does nothing, and shows nothing, while that
file exists. **Neither side is built yet**; do not implement the app side without the owner saying
so. Sprawl and Inbound's own updaters also exist; leave them as they are.

## 7. Adding an app to Hub (done in this repository, by the owner's decision)

Only the owner decides which apps are in the catalog (`crates/hub-core/src/catalog.rs`); it is a
compiled list, not a download, and a new app arrives with a Hub release. An entry has: `id`, `name`,
`blurb`, `repo` (under `Kaan88`), `exe`, `launch_args`, `folder` (under `Programs`),
`machine_folder`, `uninstall_keys`, `wait`, `setup_closes_app`, `legacy_manifest` (only for apps
that publish an older format Hub reads meanwhile), and a tile colour and glyph used only if the
app's own icon (`assets/apps/<id>.png`, 128 px) is missing. Never in the catalog: anything private,
and anything whose licence forbids sharing.

## 8. Do and do not

- **Do** change only your own repository, in a pull request. Work in a fresh worktree or clone, not
  in the owner's checkout.
- **Do** run the changed workflow step locally against scratch files (PowerShell 7 **and** Windows
  PowerShell 5.1) and compare the produced `hub.txt` with `tools/samples/<app>/hub.txt`.
- **Do** say plainly what you could not verify: a release workflow only truly runs on a release.
- **Do not** cut a release, push a tag, publish a draft, or run a release workflow. The owner
  releases.
- **Do not** install, update or uninstall anything on the owner's PC to test. Hub's tests use
  scratch folders and fake releases.
- **Do not** edit Hub's parser to accept what your release writes. Fix the release.
- **Do not** put a token, a secret or a signing key in `hub.txt`; Hub does not use signatures yet
  (decision D-004: SHA-256 over TLS from github.com).

## 9. Mistakes already made (so you do not repeat them)

- A hashtable list passed to a **child** `pwsh -File` arrives as text: call the script in-process.
- Notes limit is **bytes**, not characters (`é` is two).
- A relative path with a space in it was accepted by an early script and refused by the parser.
- A one-item `-FilesJson` list read as nothing under Windows PowerShell 5.1.
- A release workflow that builds with `--locked` fails if `Cargo.lock` was not updated after a
  version bump.
- A tray or background app has no window: asking it to close does nothing. The installer must stop it.
- `DisplayVersion` left stale by an app's own updater made Hub think an old version was installed.
- Files uploaded to a **draft** release are invisible to Hub until it is published.

## 10. Paste this into an app repository's CLAUDE.md or README

```markdown
## Hub

This app is installed and updated by Hub (`Kaan88/mdhub`). What a release, the installer and the exe
must do for that is in https://github.com/Kaan88/mdhub/blob/main/docs/COMPLIANCE.md. Before changing
the release workflow, the installer, the exe's version resource or `tools/write-hub-txt.ps1`, read it.
```
