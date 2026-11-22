# Release

**No release of Hub exists, and none is cut without the owner saying so.** Nothing is pushed,
no tag exists, `ci` has not run, and no GitHub repository (`Kaan88/hub-rs` is a working name,
plan question 1, unanswered) has been created.

## What Hub's own release carries (self-update is built, the release is not)

- `hub-update.txt`: format 1 of the shared updater (`mdshared_app::updater`), one executable, no
  engine, no uninstall entry. Hub reads it from the single constant `selfupdate::MANIFEST_URL`,
  `https://github.com/Kaan88/hub-rs/releases/latest/download/hub-update.txt`; `HUB_UPDATE_MANIFEST`
  names another to try a release before it is the latest. Downloads must lie under that
  repository's `releases/download/` path.
- `hub.txt`: Hub's own entry in the family's format, written by `tools/write-hub-txt.ps1`.
- `hub.exe`, and the installer once it exists.
- The release workflow that writes both files and checks their hashes is not written.
- Only an installed copy (folder not named release, debug or deps) updates itself, and only it
  may write the Run key.
- **The installer is still not built** (Inno, per user, `%LOCALAPPDATA%\Programs\Hub`, a new
  AppId, start at login as an option). Until it exists nothing installs Hub, so the self-update
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
