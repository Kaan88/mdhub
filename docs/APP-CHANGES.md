# What each app must change for Hub

For the owner to approve, app by app. **Nothing here has been applied**: the first four projects were
read (mdedit and mdview from their `main`, Sprawl and Inbound through `gh api`, all read only) and
their files are as they were. Read 2026-11-22.

What Hub needs from every catalog app, whatever its row says:

1. Its release carries a `hub.txt` (format in `docs/INTERFACES.md`, writer `tools/write-hub-txt.ps1`).
2. Its exe has a version resource (Hub reads it first, `docs/DESIGN.md` detect).
3. Its Inno `AppId` is the uninstall key the catalog names, so detection and `DisplayVersion` work.
4. Its own updater stands down when Hub manages it (section "Standing down" below).
5. Mdedit and mdview only: a `close` request, so Hub can ask a running window to close.

`tools/samples/<app>/hub.txt` is what each app's latest release would carry; the sizes and SHA-256
are GitHub's own figures for those releases, and `hub-core`'s `manifest::parse` reads all four.

## Shared pieces (do once)

**The script.** `tools/write-hub-txt.ps1` (here, and copied to `markdown-shared-rs/tools/`, plan
A0). Works in Windows PowerShell 5.1 and PowerShell 7 (`pwsh` is on `ubuntu-latest` and
`windows-latest` runners). Each repository keeps a **copy** in its `tools/` with the shared commit
in its first line, so a release does not depend on another repository being reachable. It refuses,
writing nothing, anything the parser would refuse (version, date, names, silent-argument
characters, 9 files, 128 lines, 16 KiB), so a bad release fails in the workflow.

Until the parser moves to the shared crate (`docs/DECISIONS.md` D-004) an app's own test cannot parse its
`hub.txt` with Rust; the script's checks mirror the parser's, and Hub's repository parses the
four samples. The test a Rust crate can run on a folder holding `<app>/hub.txt` is in
`docs/CODE.md` ("parse the samples").

**Where the file is read.** `https://github.com/Kaan88/<repo>/releases/latest/download/hub.txt`.
It exists only from each app's next release; until then Hub shows that app as "no release
information" (mdedit and mdview are read through their old manifests meanwhile, D-003). To close
the gap at once, `gh release upload v<current> tools/samples/<app>/hub.txt --repo Kaan88/<repo>`
adds the sample to the current release of each (a change to a public release: your call, not done).

**Standing down (plan section 15, question 3).** The plan has Hub write `managed-by: hub` into the
app's install folder. That cannot work for Sprawl: Hub is never elevated and Sprawl lives in
`Program Files`. Proposal (a deviation, `docs/DECISIONS.md` D-007): the marker is **Hub's own file**,
`%LOCALAPPDATA%\Hub\managed\<app id>` (content `hub`), written when Hub installs or adopts the app
and deleted when Hub forgets it. An app's updater, before it looks for an update, tests whether
that file exists and does nothing, and shows nothing, if it does. No install folder is written to,
an uninstall leaves nothing behind in it, and Inbound's folder swaps cannot delete it. The test
is a few lines in each updater (shown per app). Hub must write the file in `state.rs` (not built).

**The `close` request (mdedit, mdview).** `mdshared_app::sister::Request` gets `close: bool`
(wire form `?close=1`, sent with `send` over the single-instance port, like `follow`). The window
that receives it runs the same path as its close button: unsaved tabs are asked about, the session
is saved, so tabs come back (`running-*`). A build that predates it answers "older", and Hub
treats it like any other app (WM_CLOSE, wait). Shared change 0.75 day, each app 0.25.
(`sister.rs` has no `close` today; read, not changed.)

## mdedit (`Kaan88/markdown-editor-rs`) , 1 day

Today: Inno per user, AppId `{C58A9AB1-8337-4056-9953-04E92C05F414}`, `PrivilegesRequired=lowest`,
`%LOCALAPPDATA%\Programs\mdedit`, setup `mdedit-setup-<v>.exe`; release assets
`mdedit-<v>-windows-x86_64.exe`, `mdview-engine.exe`, `mdedit-ai.exe`; `latest` is v1.2.1.

| Item | Value |
|---|---|
| kind | `files` (exe + two engines), and the `setup` listed for a first install |
| files | `mdedit.exe` (asset `mdedit-<v>-windows-x86_64.exe`), `mdview-engine.exe`, `mdedit-ai.exe` |
| setup, silent, scope | `mdedit-setup-<v>.exe`, `/VERYSILENT /SUPPRESSMSGBOXES /NORESTART`, `user` |
| uninstall | `{C58A9AB1-8337-4056-9953-04E92C05F414}_is1` |
| wait | `mdedit-ai.exe`, `mdview-engine.exe` (its own `mdedit.exe` is implied) |

**1. The workflow** (`.github/workflows/release.yml`, job `publish`, after "Manifest (both
platforms)"; `tools/write-hub-txt.ps1` copied in). The job is on `ubuntu-latest`; `pwsh` is there.

```yaml
      - name: hub.txt (for Hub), checked
        shell: pwsh
        run: |
          $all = 'target/release-files-all'
          $v = $env:VERSION
          $silent = '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART'
          tools/write-hub-txt.ps1 -App mdedit -Repo markdown-editor-rs -Version $v `
            -Notes (Get-Content notes/notes-line.txt -Raw) `
            -Kind files -Exe mdedit.exe -Uninstall '{C58A9AB1-8337-4056-9953-04E92C05F414}_is1' `
            -Wait mdedit-ai.exe, mdview-engine.exe `
            -Files @(
              @{ Name = 'mdedit.exe'; Asset = "mdedit-$v-windows-x86_64.exe"; Path = "$all/mdedit-$v-windows-x86_64.exe" },
              @{ Name = 'mdview-engine.exe'; Path = "$all/mdview-engine.exe" },
              @{ Name = 'mdedit-ai.exe'; Path = "$all/mdedit-ai.exe" },
              @{ Name = "mdedit-setup-$v.exe"; Role = 'setup'; Path = "$all/mdedit-setup-$v.exe"; Silent = $silent; Scope = 'user' }
            ) -Out "$all/hub.txt"
          Get-Content "$all/hub.txt"
```

and `hub.txt` added to the `gh release create` list (`... "mdedit-update-2.txt" "hub.txt" ...`).
The Windows files are uploaded by the `windows` job as artifacts and merged into `$all`, so the
paths above exist when this step runs. `tools/write-manifest.sh` and `mdedit-update*.txt` stay:
builds up to 1.2.1 read them.

**2. The version resource** (`bins/mdedit/build.rs`: today only `1 ICON`). `rc.exe` compiled this
exact script in a scratch folder (llvm-rc, with `-no-preprocess`, takes it too: numbers only, no
`#include`; not tried):

```diff
-    let text = format!(
-        "1 ICON \"{}\"\n",
-        icon.display().to_string().replace('\\', "/")
-    );
+    let (major, minor, patch) = (
+        std::env::var("CARGO_PKG_VERSION_MAJOR").unwrap_or_default(),
+        std::env::var("CARGO_PKG_VERSION_MINOR").unwrap_or_default(),
+        std::env::var("CARGO_PKG_VERSION_PATCH").unwrap_or_default(),
+    );
+    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
+    let numeric = format!("{major},{minor},{patch},0");
+    let text = format!(
+        "1 ICON \"{}\"\n\
+         1 VERSIONINFO\nFILEVERSION {numeric}\nPRODUCTVERSION {numeric}\nFILEOS 0x40004\nFILETYPE 0x1\n{{\n\
+         BLOCK \"StringFileInfo\" {{ BLOCK \"040904B0\" {{\n\
+         VALUE \"CompanyName\", \"Kaan88\"\nVALUE \"FileDescription\", \"mdedit\"\n\
+         VALUE \"FileVersion\", \"{version}\"\nVALUE \"ProductVersion\", \"{version}\"\n\
+         VALUE \"InternalName\", \"mdedit\"\nVALUE \"OriginalFilename\", \"mdedit.exe\"\nVALUE \"ProductName\", \"mdedit\"\n\
+         }} }}\nBLOCK \"VarFileInfo\" {{ VALUE \"Translation\", 0x0409, 0x04B0 }}\n}}\n",
+        icon.display().to_string().replace('\\', "/")
+    );
```

Cargo reruns a build script when the package version changes. Test: a `#[cfg(windows)]` test that reads the
built exe's version resource, or a CI step (`(Get-Item target\release\mdedit.exe).VersionInfo.FileVersion`).
The size of the exe moves by under 1 KiB.

**3. The uninstall key** (`bins/mdedit/src/updater.rs`). The key must carry its hive: the shared
`record_version` runs `reg query <key> /v DisplayVersion` and `reg add <key> ...` with it as
written, so `Software\Microsoft\...` alone (no `HKCU\`) is not found and nothing is recorded.
mdview already has the right form.

```diff
-    uninstall_key: None,
+    // The installer's entry in Windows' list of installed apps (`AppId` in `mdedit.iss`).
+    uninstall_key: Some(
+        r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\{C58A9AB1-8337-4056-9953-04E92C05F414}_is1",
+    ),
```

(and "No installer entry yet." in the comment above `APP` goes.) Hub writes `DisplayVersion` itself
after its own updates, so this matters for updates the app does on its own.

**4. Standing down** (`mdshared_app::updater::installed_files`, Windows; shared change, both apps):

```diff
     if built {
         return None;
     }
+    // Hub updates this copy: `%LOCALAPPDATA%\Hub\managed\<app>` exists (D-007).
+    if managed_by_hub(app) {
+        return None;
+    }
```

with `managed_by_hub` testing the file. `None` is "this copy cannot update itself", for which the
callers still run the look; before approving, check that the bar then offers nothing (an offer
that cannot be installed is worse than none), else `check_due` must also return false.
**5.** The `close` request, above. **Risks**: the `latest` manifest names stay `mdedit-update-2.txt`
for older builds; a release that fails `write-hub-txt.ps1` fails the whole publish job, which is
meant. Hub's `files` update of mdedit replaces the three executables as the app's own updater does.

## mdview (`Kaan88/markdown-viewer-rs`, here `..\mv`) , 0.75 day

Today: AppId `{B7E2F4A9-3D1C-4C58-A6E0-91D5C8F27B34}`, per user, `mdview-setup-<v>.exe`; assets
`mdview.exe` (no version in the name), `mdview-engine.exe`; `latest` is v1.0.4. Its updater **already**
has the uninstall key in the right form. Its `bins/mdv-app/build.rs` has the icon only.

| Item | Value |
|---|---|
| kind | `files` (exe + engine), `setup` listed |
| files | `mdview.exe`, `mdview-engine.exe` |
| setup, silent, scope | `mdview-setup-<v>.exe`, `/VERYSILENT /SUPPRESSMSGBOXES /NORESTART`, `user` |
| uninstall | `{B7E2F4A9-3D1C-4C58-A6E0-91D5C8F27B34}_is1` (already in `updater.rs`) |
| wait | `mdview-engine.exe` |

**Workflow** (`release.yml`, job `publish`, after "Manifest (both platforms)"; the Windows files
are `$all/mdview.exe`, `$all/mdview-engine.exe`, `$all/mdview-setup-$VERSION.exe`):

```yaml
      - name: hub.txt (for Hub), checked
        shell: pwsh
        run: |
          $all = 'target/release-files-all'
          $v = $env:VERSION
          tools/write-hub-txt.ps1 -App mdview -Repo markdown-viewer-rs -Version $v `
            -Notes (Get-Content notes/notes-line.txt -Raw) `
            -Kind files -Exe mdview.exe -Uninstall '{B7E2F4A9-3D1C-4C58-A6E0-91D5C8F27B34}_is1' `
            -Wait mdview-engine.exe `
            -Files @(
              @{ Name = 'mdview.exe'; Path = "$all/mdview.exe" },
              @{ Name = 'mdview-engine.exe'; Path = "$all/mdview-engine.exe" },
              @{ Name = "mdview-setup-$v.exe"; Role = 'setup'; Path = "$all/mdview-setup-$v.exe"; Silent = '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART'; Scope = 'user' }
            ) -Out "$all/hub.txt"
          Get-Content "$all/hub.txt"
```

and `hub.txt` added to the `gh release create` list (after `"mdview-update.txt"`).

**Version resource**: the same patch as mdedit in `bins/mdv-app/build.rs`, with "mdview", `mdview.ico`
and `mdview.exe`. **Standing down** and **close**: as mdedit (shared). Risks as mdedit; the
asset `mdview.exe` is unversioned, so `v<ver>/mdview.exe` is the URL, which the script builds.

## Sprawl (`Kaan88/sprawl`) , 0.75 day

Today (read): **no GitHub Actions workflow**. A release is made on the owner's PC by
`crates/sprawl-release` (`build`, then `publish` with `gh release create`). Inno `AppId=Sprawl`
(key `Sprawl_is1`), `PrivilegesRequired=admin`, `{autopf}\Sprawl`, `UsePreviousAppDir=no`; the
exe already has icon and version (`build.rs: icon_and_version`). Release `v1.0.5`: `sprawl.exe`,
`Microsoft.WindowsAppRuntime.Bootstrap.dll`, `Sprawl-Setup-1.0.5.exe` (5 544 567 bytes),
`sprawl-update.json(.sig)`. Setup's `[Run]` entries launch Sprawl only when not silent
(`skipifsilent`) or when `/RESTARTSPRAWL` is given, so a silent run from Hub starts nothing;
`CloseApplications=yes` asks a running Sprawl to close.

| Item | Value |
|---|---|
| kind | `setup` only (Hub, not elevated, cannot write Program Files: decision 15) |
| setup, silent, scope | `Sprawl-Setup-<v>.exe`, `/VERYSILENT /SUPPRESSMSGBOXES /NORESTART`, `machine` (Hub runs it with `runas`: Windows' prompt at every update) |
| exe | `sprawl.exe` |
| uninstall | `Sprawl_is1` (HKLM, 64-bit view) |
| wait | none (`CloseApplications`) |

**The release step is Rust, not YAML**: `sprawl-release build` writes `hub.txt` beside the rest, and
`publish` uploads it. The files list goes through a JSON file so no quoting crosses a command line.

```diff
     std::fs::write(out.join("notes.md"), notes).unwrap_or_else(|e| fail(e));
+    // hub.txt for Hub (tools/write-hub-txt.ps1, copied from the Hub repository)
+    let one_line: String = notes.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with('#')).unwrap_or("A new version of Sprawl.").chars().take(300).collect();
+    let list = serde_json::json!([{ "Name": setup.file_name().unwrap().to_string_lossy(), "Path": setup.display().to_string(), "Role": "setup", "Silent": "/VERYSILENT /SUPPRESSMSGBOXES /NORESTART", "Scope": "machine" }]);
+    std::fs::write(out.join("hub-files.json"), list.to_string()).unwrap_or_else(|e| fail(e));
+    run(Command::new("pwsh").args(["-NoProfile", "-File", "tools/write-hub-txt.ps1", "-App", "sprawl", "-Repo", "sprawl", "-Version", &v, "-Notes", &one_line, "-Kind", "setup", "-Exe", "sprawl.exe", "-Uninstall", "Sprawl_is1", "-FilesJson"]).arg(out.join("hub-files.json")).arg("-Out").arg(out.join("hub.txt")));
@@ publish()
-    assets.extend([out.join(format!("Sprawl-Setup-{v}.exe")), out.join(MANIFEST), out.join(SIGNATURE)]);
+    assets.extend([out.join(format!("Sprawl-Setup-{v}.exe")), out.join(MANIFEST), out.join(SIGNATURE), out.join("hub.txt")]);
```

(`notes` is moved by the `fs::write`; clone it first. Not compiled: sketch, the owner's tool.) A
test beside the existing three: the one-line extraction of a CHANGELOG section with a heading.
Sprawl's `sprawl-update.json` and its signature stay: copies up to 1.0.5 read them.

**Standing down**: `crates/sprawl/src/update.rs` asks `api.github.com` once a day. At its start,
return early when `%LOCALAPPDATA%\Hub\managed\sprawl` exists. This also removes the REST API
limit for a Hub user. **Risks**: every Hub update of Sprawl shows the Windows prompt (your answer,
question 7); Setup run by `ShellExecuteEx runas` from a non-elevated Hub when the user is a
standard user asks for an administrator's password; if Setup fails (exit code) the old Sprawl
stays. Not tried silently from Hub on a PC (this build never installs anything real).

## Inbound (`Kaan88/inbound`) , 1.5 days

Today (read): `.github/workflows/windows.yml` on a pushed `v*` tag builds Setup and the zip, signs
`manifest.json` (RSA, secret `UPDATE_SIGNING_KEY`), and creates a **draft** release with exactly
six files (`InboundSetup-<v>.exe`, the zip, `manifest.json`, `public.pem`, the Firefox add-on,
`SHA256SUMS`); the owner reads and publishes the draft, which starts the Linux and Docker
workflows. Inno `AppId {6F4B7C2E-3D91-4E8A-9B57-2C1D0A8E5F13}`, `PrivilegesRequired=lowest`,
`%LOCALAPPDATA%\Programs\Inbound Download Manager`, every `[Run]` entry `skipifsilent`. The exe is
.NET: the version comes from `Directory.Build.props`, so it has a version resource already.
`latest` is v0.9.0; its Setup is 72 028 167 bytes (68.7 MiB; the zip is 101 MB and not used).

| Item | Value |
|---|---|
| kind | `setup` only: the zip needs `apply-update.ps1` and the registrations Setup makes |
| exe | `app\DownloadManager.Host.exe` |
| setup, silent, scope | `InboundSetup-<v>.exe`, `/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /CURRENTUSER`, `user` |
| uninstall | `{6F4B7C2E-3D91-4E8A-9B57-2C1D0A8E5F13}_is1` |
| wait | the catalog's three (`Host`, `Desktop.WinUI`, `NativeHost`) |

**Workflow**: a step after "Sign the update file and gather the release files", and the file
count raised (the step "Put them in a draft release" refuses any count but 6 or 5):

```yaml
      - name: hub.txt (for Hub), checked
        if: env.TAG != ''
        shell: powershell
        run: |
          .\scripts\release-notes.ps1 -Version $env:VERSION -OutFile "$env:RUNNER_TEMP\hub-notes.md"
          $line = (Get-Content "$env:RUNNER_TEMP\hub-notes.md" | Where-Object { $_.Trim() -ne '' -and $_ -notmatch '^#' } | Select-Object -First 1)
          .\tools\write-hub-txt.ps1 -App inbound -Repo inbound -Version $env:VERSION -Notes $line `
            -Kind setup -Exe 'app\DownloadManager.Host.exe' -Uninstall '{6F4B7C2E-3D91-4E8A-9B57-2C1D0A8E5F13}_is1' `
            -Wait 'app\DownloadManager.Host.exe','app\DownloadManager.Desktop.WinUI.exe','app\DownloadManager.NativeHost.exe' `
            -Files @{ Name = "InboundSetup-$env:VERSION.exe"; Role = 'setup'; Path = "$env:RELEASE_DIR\InboundSetup-$env:VERSION.exe"; Silent = '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /CURRENTUSER'; Scope = 'user' } `
            -Out "$env:RELEASE_DIR\hub.txt"
```

(the notes line is the first non-heading line of the draft's own notes, cut to 1024 characters by
the script), and in "Put them in
a draft release": `$expected = if ($env:NO_FIREFOX_XPI) { 6 } else { 7 }`.

**`latest` and the Android releases (asked).** `releases/latest/download/hub.txt` resolves to the
newest published non-pre-release release **that GitHub marks latest**. Checked now: the repository
has `android-v0.0.9` (published 2026-11-20, `prerelease: false`, newer than `v0.9.0` of 2026-11-17),
yet `https://github.com/Kaan88/inbound/releases/latest` redirects to `v0.9.0` and the API's
`releases/latest` says v0.9.0. So that Android release was evidently created with
"make latest: false" (the API does not show the flag); `android-v0.0.2` to `0.0.8` and
`android-latest` and `macos-update-test` are pre-releases. It is fragile: the next Android release
created without `--latest=false`, or any non-pre-release created later, becomes `latest`, and then
`latest/download/hub.txt` is a 404 (Hub shows "no release information", never a wrong install),
and Inbound's **own** updater, which reads `latest/download/manifest.json`, breaks the same way.
Fix, in whatever creates the Android releases: always `--prerelease` (all but `0.0.9` are) or
`--latest=false`, and write that in `docs/release-builds.md`. If it ever bites, the fallback is
one permanent pre-release tag `hub` that each Windows release overwrites with its `hub.txt`
(`--clobber`), and the catalog's URL for Inbound pointing there: a code change in Hub, not built.

**Standing down**: Inbound's updater (`ReleaseUpdates.cs`) returns "nothing to do" when
`%LOCALAPPDATA%\Hub\managed\inbound` exists. **Risks**: the 68.7 MB Setup passes through Hub's
memory (`docs/DECISIONS.md` D-005, under the 128 MiB cap); the install tree also runs
`DownloadManager.Server.exe` (web UI) and `inbound-torrent.exe`, which the catalog's wait list
does not name: Setup's restart manager closes them, but listing them in the catalog is safer (a
catalog edit, Hub's side); an unattended silent upgrade re-selects the previous tasks, which the
Setup's `[Code]` already handles ("A silent upgrade shows no task page"); drafts are invisible to
`latest` until published, as today.

## Barbican (`Kaan88/barbican`)

Added to the catalog afterwards; read from the Barbican repository's own files, nothing applied
here. A Rust/Slint mail, calendar, contacts and files client for JMAP (Stalwart) servers. It runs in
the tray with no required window. **The work is in the Barbican repository, branch `installer-inno`,
and has not been released**: until a release carries `hub.txt`, Barbican shows "no release
information" in Hub, never a wrong install. There is no sample in `tools/samples/` because those
carry GitHub's real sizes and hashes and there is no release to take them from.

| Item | Value |
|---|---|
| kind | `setup` (per-user Inno, `%LOCALAPPDATA%\Programs\Barbican`) |
| exe | `barbican.exe` |
| uninstall | `{ACE04D99-4564-4E70-9A43-FE47A6A0E3B5}_is1` (the Inno `AppId`) |
| wait | `barbican.exe` (its updater helper is a hidden PowerShell started from it) |
| setup, silent, scope | `Barbican-Setup-<v>.exe` (name to be confirmed in the Barbican workflow), `/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /CURRENTUSER`, `user` |
| `setup_closes_app` | true: the installer stops the running app (`CloseApplications`) |

The release must carry `hub.txt` written with `tools/write-hub-txt.ps1` (`-App barbican -Repo
barbican`), and the exe a version resource. **Standing down**: Barbican's own updater does **not**
stand down for Hub yet; that is not built on either side (`docs/COMPLIANCE.md` section 6), so
until it is, Hub and Barbican's updater could both offer the same update.

## Days

| Where | Days |
|---|---|
| Script and samples (A0, done here) | 0.5 |
| Shared: `close` request, standing down | 1 |
| mdedit | 1 |
| mdview | 0.75 |
| Sprawl | 0.75 |
| Inbound | 1.5 |

Order: the script, then mdedit and mdview with the shared pieces (the first slice), then Sprawl and
Inbound; none of them has to wait for Hub's window.
