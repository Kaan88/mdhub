# Release

**No release of Hub exists, and none is cut without the owner saying so.** The private repository
`Kaan88/mdhub` exists (a working name, plan question 1; make it public with
`gh repo edit Kaan88/mdhub --visibility public --accept-visibility-change-consequences` before Hub
updates itself from it). No tag exists.

## What Hub's own release carries (self-update is built, the release is not)

- `hub-update.txt`: format 1 of the shared updater (`mdshared_app::updater`), one executable, no
  engine, no uninstall entry. Hub reads it from the single constant `selfupdate::MANIFEST_URL`,
  `https://github.com/Kaan88/mdhub/releases/latest/download/hub-update.txt`; `HUB_UPDATE_MANIFEST`
  names another to try a release before it is the latest. Downloads must lie under that
  repository's `releases/download/` path.
- `hub.txt`: Hub's own entry in the family's format, written by `tools/write-hub-txt.ps1`.
- `hub.exe`, and the installer once it exists.
- `.github/workflows/release.yml` writes both files with `tools/write-hub-update.ps1` and
  `tools/write-hub-txt.ps1`, checks every hash, and creates the release on a pushed `v*` tag (or on
  a manual run with publish=true); a manual run with publish=false builds and checks only. Not run yet.
- Only an installed copy (folder not named release, debug or deps) updates itself, and only it
  may write the Run key.
- The installer is `packaging/windows/hub.iss` (Inno, per user, `%LOCALAPPDATA%\Programs\Hub`,
  AppId `{A3D5E8C1-7B42-4F96-8E1D-5C0B2F7A9D64}`, start at login as a task; built locally by
  `tools/build-installer.ps1`, not yet run by hand). Until it exists nothing installs Hub, so the self-update
  and "Start with Windows" have only run against scratch copies and test keys.

## How one will be cut, later

Hub's release is the family's way (mdedit's `docs/RELEASE.md`): **pushing the tag `v<version>`**
after `ci` is green on `main`, not a local script. A version that failed is not used again.

1. `ci` is green on `main` (fmt, clippy, tests, the shots, on `windows-latest`). Start the run
   once and look once; do not poll (`gh run watch` once used up an account's API quota).
2. The version in `Cargo.toml` (`[workspace.package]`) is the new one; `docs/STATE.md` and the
   release notes (`docs/release-notes/<version>.md`, first line the one-line note) are written;
   the budgets are measured and written down.
3. Push the tag. The release workflow (to be written) builds `hub.exe` natively on
   `windows-latest` with `--locked`, builds the installer and the portable exe, writes
   `hub-update.txt` and `hub.txt`, checks every hash against the files, and creates the release
   with `gh release create` from the tagged commit, `--latest`.
4. The release carries `Hub-Setup-<v>.exe`, `hub.exe`, `hub-update.txt` and `hub.txt`.
5. A release is not replaced (a copy that has the version never takes it again): the patch
   number goes up.

## Before the first one

- The owner's go-ahead, the repository and its name (then the one constant), and a decision about
  code signing (SmartScreen and antivirus meet an unsigned installer run silently by an unsigned
  program: plan section 15).
- The installer and the release workflow (phase 6 of the plan).
- The by-hand pass on Windows (plan section 12): a real installer run silently by Hub, a real
  UAC prompt for a machine-scope setup, an app that is open, Explorer restarting under the tray
  icon, a real self-update from a published release. Nothing counts as working on Windows until
  it has run on Windows.
- The four apps' own changes (`docs/APP-CHANGES.md`) are releases of those apps, separately.
