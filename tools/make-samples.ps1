<#
  Regenerates tools/samples/<app>/hub.txt with tools/write-hub-txt.ps1: what each app's release
  would carry for Hub. The sizes and SHA-256 are those GitHub reports for the real latest release
  of each project (read 2026-11-22 with the API's asset `digest`); the notes lines are the ones
  Hub's demo uses. Nothing is downloaded. Run from the repository root:
    pwsh tools/make-samples.ps1
#>
$ErrorActionPreference = 'Stop'
$w = Join-Path $PSScriptRoot 'write-hub-txt.ps1'
$out = Join-Path $PSScriptRoot 'samples'
$silent = '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART'

& $w -App mdedit -Repo markdown-editor-rs -Version 1.2.1 -Date 2026-11-21 `
    -Notes 'An option to leave out tabs you barely looked at, and a tidier AI menu.' `
    -Kind files -Exe mdedit.exe -Uninstall '{C58A9AB1-8337-4056-9953-04E92C05F414}_is1' `
    -Wait 'mdedit-ai.exe', 'mdview-engine.exe' -Files @(
        @{ Name = 'mdedit.exe'; Asset = 'mdedit-1.2.1-windows-x86_64.exe'; Size = 8403968; Sha256 = 'c14662b07889b9a0a55e59f3eb383c1f2c6d115690bb7b97c0e147b0b763fd5e' },
        @{ Name = 'mdview-engine.exe'; Size = 26291712; Sha256 = 'a2433a55aced240d3b1c6d3d3b114aec3d72f32cda95b842750141a8ce6ff5cd' },
        @{ Name = 'mdedit-ai.exe'; Size = 2265600; Sha256 = '0583c6ad91e21b4d72ff40ee5d5e24a1b8103b3f8640512c783a23e1870d0193' },
        @{ Name = 'mdedit-setup-1.2.1.exe'; Role = 'setup'; Size = 12594139; Sha256 = '61d052cd43a4b77e2c62d7a5b4bfbab1a9c27bc6273e3e53dd21fc9524e879db'; Silent = $silent; Scope = 'user' }
    ) -Out (Join-Path $out 'mdedit\hub.txt')

& $w -App mdview -Repo markdown-viewer-rs -Version 1.0.4 -Date 2026-11-21 `
    -Notes 'Leave out tabs you barely looked at, and Settings in categories.' `
    -Kind files -Exe mdview.exe -Uninstall '{B7E2F4A9-3D1C-4C58-A6E0-91D5C8F27B34}_is1' `
    -Wait 'mdview-engine.exe' -Files @(
        @{ Name = 'mdview.exe'; Size = 9415680; Sha256 = 'bcbf26fd20eea0fd3b3d6d88313e6529705bb74377419059f5e61e0c0601e369' },
        @{ Name = 'mdview-engine.exe'; Size = 26292736; Sha256 = '17a839c670b6353e947972f6c3d1bd423de7cc7cd39cf3da1083e423f19450ac' },
        @{ Name = 'mdview-setup-1.0.4.exe'; Role = 'setup'; Size = 11966778; Sha256 = 'd32499f2bb483eebe204dd5c51588972b576fbc04297f72a7126bb3b36459bfe'; Silent = $silent; Scope = 'user' }
    ) -Out (Join-Path $out 'mdview\hub.txt')

# Sprawl stays installed for all users (plan 3A): a setup only, run elevated at the owner's click.
& $w -App sprawl -Repo sprawl -Version 1.0.5 -Date 2026-11-20 `
    -Notes 'The AI can propose recognisers and cleaners through Codex and Claude Code, and Sprawl checks each one.' `
    -Kind setup -Exe sprawl.exe -Uninstall Sprawl_is1 -Files @(
        @{ Name = 'Sprawl-Setup-1.0.5.exe'; Role = 'setup'; Size = 5544567; Sha256 = 'a268e03bb2d0150bd8b8b5725b738b312218ebe99d7429099d62b9c488f60196'; Silent = $silent; Scope = 'machine' }
    ) -Out (Join-Path $out 'sprawl\hub.txt')

# Inbound: its setup only (the zip needs apply-update.ps1 and the setup's registrations).
& $w -App inbound -Repo inbound -Version 0.9.0 -Date 2026-11-17 `
    -Notes 'Sites become Remote files: an All remotes page, two panes, copies between remotes, one-way sync with a preview.' `
    -Kind setup -Exe 'app\DownloadManager.Host.exe' -Uninstall '{6F4B7C2E-3D91-4E8A-9B57-2C1D0A8E5F13}_is1' `
    -Wait 'app\DownloadManager.Host.exe', 'app\DownloadManager.Desktop.WinUI.exe', 'app\DownloadManager.NativeHost.exe' -Files @(
        @{ Name = 'InboundSetup-0.9.0.exe'; Role = 'setup'; Size = 72028167; Sha256 = 'cd496332e897774c6fe6a57aefaef6dfd2df0270c929c415b3aa869c863c1ac9'; Silent = "$silent /CURRENTUSER"; Scope = 'user' }
    ) -Out (Join-Path $out 'inbound\hub.txt')

# Purge: for all users like Sprawl (Program Files, PrivilegesRequired=admin), run elevated at the
# owner's click. Size and SHA-256 of its v0.1.0 (alpha) release, read 2026-11-23; the file equals
# that release's own hub.txt.
& $w -App purge -Repo purge -Version 0.1.0 -Date 2026-11-23 `
    -Notes 'Alpha: first public build. Uninstall, leftover scan, quarantine with undo.' `
    -NotesFile 'release-notes-0.1.0.md' `
    -Kind setup -Exe Purge.exe -Uninstall '{7C1B9E64-2D58-4F1A-9B03-6E4C5A8D7F21}_is1' `
    -Wait 'Purge.exe', 'purge-engine.exe', 'purge-broker.exe' -Files @(
        @{ Name = 'Purge-Setup-0.1.0.exe'; Role = 'setup'; Size = 43420740; Sha256 = 'ddbeecf68b6b1c251c1d4023e31348c7ccc6cafc2bb03fe013126e7330ee8220'; Silent = $silent; Scope = 'machine' }
    ) -Out (Join-Path $out 'purge\hub.txt')
