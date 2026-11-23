# Release

**Every release is cut by the owner saying so, and pushed as a tag.** `Kaan88/mdhub` exists and is
still private (a working name, plan question 1; make it public with
`gh repo edit Kaan88/mdhub --visibility public --accept-visibility-change-consequences` before Hub
updates itself from it). `v0.1.0` to `v0.1.3` are published from it, each with the four assets below.

## What Hub's own release carries (self-update is built and has run once)

- `hub-update.txt`: format 1 of the shared updater (`mdshared_app::updater`), one executable, no
  engine, no uninstall entry. Hub reads it from the single constant `selfupdate::MANIFEST_URL`,
  `https://github.com/Kaan88/mdhub/releases/latest/download/hub-update.txt`; `HUB_UPDATE_MANIFEST`
  names another to try a release before it is the latest. Downloads must lie under that
  repository's `releases/download/` path.
- `hub.txt`: Hub's own entry in the family's format, written by `tools/write-hub-txt.ps1`.
- `Hub-Setup-<v>.exe` and `hub.exe` (the portable one).
- `.github/workflows/release.yml` writes both text files with `tools/write-hub-update.ps1` and
  `tools/write-hub-txt.ps1`, checks every hash, and creates the release on a pushed `v*` tag (or on
  a manual run with publish=true); a manual run with publish=false builds and checks only. It has
  run for every release so far.
- Only an installed copy (folder not named release, debug or deps) updates itself, and only it
  may write the Run key.
- The installer is `packaging/windows/hub.iss` (Inno, per user, `%LOCALAPPDATA%\Programs\Hub`,
  AppId `{A3D5E8C1-7B42-4F96-8E1D-5C0B2F7A9D64}`, start at login as a task), built by the release
  workflow; `tools/build-installer.ps1` builds the same file by hand. The owner ran the installer and
  a real self-update from 0.1.0 to 0.1.1 on 2026-11-23: both worked. "Start with Windows" has only
  run against test keys.

## How one is cut

Hub's release is the family's way (mdedit's `docs/RELEASE.md`): **pushing the tag `v<version>`**
after `ci` is green on `main`, not a local script. A version that failed is not used again.

1. `ci` is green on `main` (fmt, clippy, tests, the shots, on `windows-latest`). Start the run
   once and look once; do not poll (`gh run watch` once used up an account's API quota).
2. The version in `Cargo.toml` (`[workspace.package]`) is the new one; `docs/STATE.md` and the
   release notes (`docs/release-notes/<version>.md`, first line the one-line note) are written;
   the budgets are measured and written down.
3. Push the tag. The release workflow builds `hub.exe` natively on
   `windows-latest` with `--locked`, builds the installer and the portable exe, writes
   `hub-update.txt` and `hub.txt`, checks every hash against the files, and creates the release
   with `gh release create` from the tagged commit, `--latest`.
4. The release carries `Hub-Setup-<v>.exe`, `hub.exe`, `hub-update.txt` and `hub.txt`.
5. A release is not replaced (a copy that has the version never takes it again): the patch
   number goes up.

## Still not done by hand

- A decision about code signing (SmartScreen and antivirus meet an unsigned installer run silently
  by an unsigned program: plan section 15).
- The by-hand pass on Windows (plan section 12): a real installer run silently by Hub, a real
  UAC prompt for a machine-scope setup, an app that is open, Explorer restarting under the tray
  icon, a real self-update from a published release. Nothing counts as working on Windows until
  it has run on Windows.
- The apps' own changes (`docs/APP-CHANGES.md`) are releases of those apps, separately.
