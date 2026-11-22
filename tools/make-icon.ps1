# Regenerates assets/hub.ico. Run from the repo root:  pwsh tools/make-icon.ps1
#
# The icon has one drawing, bins/hub/src/icon.rs, which also draws the tray icon at run time. This
# script runs that module's ignored test `dump_the_icon_sizes`, which writes the ten sizes as raw
# BGRA, each drawn at its own size on the pixel grid (not one picture scaled down), and packs them
# as PNG entries of an .ico.
#
#   -Sheet <file.png>  also writes a contact sheet to look at: 16, 24, 32, 48 and 256 px, plain and
#                      with the badge "3", on a light and a dark background, the small ones
#                      enlarged beside themselves.
param([string]$Sheet = '')
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$root = Split-Path -Parent $PSScriptRoot
$sizes = 16, 20, 24, 32, 40, 48, 64, 96, 128, 256

function Dump([int]$count) {
  $dir = Join-Path ([IO.Path]::GetTempPath()) ("hub-icon-" + [guid]::NewGuid().ToString('N'))
  $env:HUB_ICON_DUMP = $dir; $env:HUB_ICON_COUNT = "$count"
  try {
    & cargo test --quiet --manifest-path (Join-Path $root 'Cargo.toml') -p hub icon::tests::dump_the_icon_sizes -- --ignored --exact | Out-Host
    if ($LASTEXITCODE -ne 0) { throw "cargo test failed ($LASTEXITCODE)" }
  } finally { Remove-Item Env:HUB_ICON_DUMP, Env:HUB_ICON_COUNT }
  $dir
}
# A raw straight-alpha BGRA dump as a bitmap.
function Load([string]$dir, [int]$s) {
  $raw = [IO.File]::ReadAllBytes((Join-Path $dir "$s.bgra"))
  if ($raw.Length -ne $s * $s * 4) { throw "$s.bgra has $($raw.Length) bytes" }
  $b = New-Object System.Drawing.Bitmap $s, $s, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
  $d = $b.LockBits([System.Drawing.Rectangle]::new(0, 0, $s, $s), 'WriteOnly', $b.PixelFormat)
  for ($y = 0; $y -lt $s; $y++) {
    [Runtime.InteropServices.Marshal]::Copy($raw, $y * $s * 4, [IntPtr]::Add($d.Scan0, $y * $d.Stride), $s * 4)
  }
  $b.UnlockBits($d); $b
}

$plain = Dump 0
$imgs = @($sizes | ForEach-Object {
    $b = Load $plain $_; $ms = New-Object System.IO.MemoryStream
    $b.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png); $b.Dispose(); , $ms.ToArray()
  })
$out = New-Object System.IO.MemoryStream; $w = New-Object System.IO.BinaryWriter $out
$w.Write([uint16]0); $w.Write([uint16]1); $w.Write([uint16]$sizes.Count)
$off = 6 + 16 * $sizes.Count
for ($i = 0; $i -lt $sizes.Count; $i++) {
  $n = if ($sizes[$i] -ge 256) { 0 } else { $sizes[$i] }
  $w.Write([byte]$n); $w.Write([byte]$n); $w.Write([byte]0); $w.Write([byte]0)
  $w.Write([uint16]1); $w.Write([uint16]32); $w.Write([uint32]$imgs[$i].Length); $w.Write([uint32]$off)
  $off += $imgs[$i].Length
}
foreach ($im in $imgs) { $w.Write([byte[]]$im) }
New-Item -ItemType Directory -Force (Join-Path $root 'assets') | Out-Null
[IO.File]::WriteAllBytes((Join-Path $root 'assets\hub.ico'), $out.ToArray())
"assets\hub.ico: $($out.Length) bytes, $($sizes.Count) sizes"

if ($Sheet) {
  $badged = Dump 3
  $shown = 16, 24, 32, 48, 256
  $zoom = @{ 16 = 8; 24 = 6; 32 = 4; 48 = 3 }   # the small ones again, enlarged, pixels kept square
  $pad = 24; $rowH = 256 + 2 * $pad
  $width = $pad; foreach ($s in $shown) { $width += $s + $pad; if ($zoom[$s]) { $width += $s * $zoom[$s] + $pad } }
  $sheetBmp = New-Object System.Drawing.Bitmap $width, (4 * $rowH)
  $g = [System.Drawing.Graphics]::FromImage($sheetBmp)
  $g.InterpolationMode = 'NearestNeighbor'; $g.PixelOffsetMode = 'Half'
  $row = 0
  foreach ($bg in 0xF3F3F3, 0x202020) {
    foreach ($dir in $plain, $badged) {
      $y = $row * $rowH
      $brush = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, ($bg -shr 16) -band 255, ($bg -shr 8) -band 255, $bg -band 255))
      $g.FillRectangle($brush, 0, $y, $width, $rowH); $brush.Dispose()
      $x = $pad
      foreach ($s in $shown) {
        $b = Load $dir $s
        $g.DrawImage($b, $x, $y + $pad, $s, $s); $x += $s + $pad
        if ($zoom[$s]) { $z = $s * $zoom[$s]; $g.DrawImage($b, $x, $y + $pad, $z, $z); $x += $z + $pad }
        $b.Dispose()
      }
      $row++
    }
  }
  $g.Dispose()
  $full = [IO.Path]::GetFullPath($Sheet, (Get-Location).Path)
  New-Item -ItemType Directory -Force (Split-Path -Parent $full) | Out-Null
  $sheetBmp.Save($full, [System.Drawing.Imaging.ImageFormat]::Png); $sheetBmp.Dispose()
  Remove-Item -Recurse -Force $badged
  "sheet: $full"
}
Remove-Item -Recurse -Force $plain
