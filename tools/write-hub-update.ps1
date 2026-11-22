<#
.SYNOPSIS
  Writes hub-update.txt, the manifest Hub's own self-update reads (format 1 of mdshared_app::update:
  version, notes, one Windows executable with its URL and SHA-256). Called by the release workflow.
.EXAMPLE
  pwsh tools/write-hub-update.ps1 -Version 0.1.0 -Notes "First release." -Exe target/release/hub.exe -AssetName hub.exe -Out dist/hub-update.txt
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Version,
    [string]$Notes = '',
    [Parameter(Mandatory)][string]$Exe,
    [Parameter(Mandatory)][string]$AssetName,
    [string]$Repo = 'hub-rs',
    [Parameter(Mandatory)][string]$Out
)
$ErrorActionPreference = 'Stop'
function Fail([string]$m) { [Console]::Error.WriteLine("write-hub-update: $m"); exit 1 }
if ($Version -notmatch '^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$') { Fail "bad version $Version" }
if ($AssetName -notmatch '^[A-Za-z0-9._-]{1,128}$') { Fail "bad asset name $AssetName" }
if (-not (Test-Path $Exe)) { Fail "$Exe does not exist" }
if ($Notes -eq '') { $Notes = 'A new version of Hub.' }
$Notes = ($Notes -replace '[\x00-\x1f\x7f]', ' ').Trim()
if ([Text.Encoding]::UTF8.GetByteCount($Notes) -gt 1024) { Fail 'notes: at most 1024 bytes' }
$sha = (Get-FileHash -Algorithm SHA256 $Exe).Hash.ToLowerInvariant()
$url = "https://github.com/Kaan88/$Repo/releases/download/v$Version/$AssetName"
$text = "manifest: 1`nversion: $Version`nnotes: $Notes`nwindows-x86_64.asset: $AssetName`nwindows-x86_64.url: $url`nwindows-x86_64.sha256: $sha`n"
$dir = Split-Path -Parent $Out
if ($dir) { New-Item -ItemType Directory -Force $dir | Out-Null }
[IO.File]::WriteAllText($Out, $text, (New-Object Text.UTF8Encoding $false))
