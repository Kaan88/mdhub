# Release

**No release of Hub exists, and none is cut without the owner saying so.** The repository has not
been pushed anywhere, `ci` has not run, no tag exists, and no GitHub repository (`Kaan88/hub-rs`
is the working name, plan question 1) has been created. Nothing in this file has been tried.

## How one will be cut, later

Hub's release is the family's way (mdedit's `docs/RELEASE.md`): **pushing the tag `v<version>`**
after `ci` is green on `main`, not a local script. A version that failed is not used again.

1. `ci` is green on `main` (fmt, clippy, tests, the shots, on `windows-latest`). Start the run
   once and look once; do not poll (`gh run watch` once used up an account's API quota).
2. The version in `Cargo.toml` (`[workspace.package]`) is the new one; `docs/STATE.md` and the
   release notes (`docs/release-notes/<version>.md`, whose first line is the one-line note) are
   written; the budgets are measured and written down.
3. Push the tag. The release workflow (to be written, phase 6 of the plan; it does not exist yet)
   builds `hub.exe` natively on `windows-latest` with `--locked`, builds Hub's own installer
   (Inno, per user, `%LOCALAPPDATA%\Programs\Hub`, a new AppId, start at login as an option) and
   the portable exe, writes **its own `hub.txt`** with `tools/write-hub-txt.ps1` (Hub updates
   itself like any app: plan section 7), checks every hash against the files, and creates the
   release with `gh release create` from the tagged commit, `--latest`.
4. The release carries `Hub-Setup-<v>.exe`, `hub.exe` and `hub.txt`.
5. A release is not replaced (a copy that has the version never takes it again): the patch
   number goes up.

## Before the first one

- The owner's go-ahead, the repository and its name, and a decision about code signing
  (SmartScreen and antivirus meet an unsigned installer run silently by an unsigned program:
  plan section 15).
- Phase 6 of the plan: the installer, the self-update, the release workflow.
- The by-hand pass on Windows (plan section 12), which no test here has done: a real installer
  run silently by Hub, a real UAC prompt for a machine-scope setup, an app that is open, Explorer
  restarting under the tray icon. Nothing counts as working on Windows until it has run on Windows.
- The four apps' own changes (`docs/APP-CHANGES.md`) are releases of those apps, separately.
