# Draws assets/hub.ico (a blue rounded tile with a white download arrow). Run from the repo root.
Add-Type -AssemblyName System.Drawing
function Draw([int]$s){
  $b = New-Object System.Drawing.Bitmap $s,$s
  $g = [System.Drawing.Graphics]::FromImage($b); $g.SmoothingMode='AntiAlias'; $g.Clear([System.Drawing.Color]::Transparent)
  $r = [int]($s*0.22); $d=2*$r
  $p = New-Object System.Drawing.Drawing2D.GraphicsPath
  $p.AddArc(0,0,$d,$d,180,90); $p.AddArc($s-$d-1,0,$d,$d,270,90); $p.AddArc($s-$d-1,$s-$d-1,$d,$d,0,90); $p.AddArc(0,$s-$d-1,$d,$d,90,90); $p.CloseFigure()
  $g.FillPath((New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255,0x3b,0x5b,0xdb))),$p)
  $w=[single]($s*0.09); $pen = New-Object System.Drawing.Pen ([System.Drawing.Color]::White),$w; $pen.StartCap='Round'; $pen.EndCap='Round'; $pen.LineJoin='Round'
  $cx=[single]($s/2); $g.DrawLine($pen,$cx,[single]($s*0.22),$cx,[single]($s*0.60))
  [System.Drawing.PointF[]]$pts = @([System.Drawing.PointF]::new([single]($s*0.32),[single]($s*0.46)),[System.Drawing.PointF]::new($cx,[single]($s*0.64)),[System.Drawing.PointF]::new([single]($s*0.68),[single]($s*0.46)))
  $g.DrawLines($pen,$pts)
  $g.DrawLine($pen,[single]($s*0.28),[single]($s*0.78),[single]($s*0.72),[single]($s*0.78))
  $g.Dispose(); $b
}
# Drawn at 4x and scaled down with a high-quality filter, so small sizes keep clean edges.
function Make([int]$s){
  $big = Draw ($s*4)
  $b = New-Object System.Drawing.Bitmap $s,$s
  $g = [System.Drawing.Graphics]::FromImage($b); $g.InterpolationMode='HighQualityBicubic'; $g.PixelOffsetMode='HighQuality'; $g.CompositingMode='SourceCopy'
  $g.DrawImage($big,0,0,$s,$s); $g.Dispose(); $big.Dispose()
  $ms = New-Object System.IO.MemoryStream; $b.Save($ms,[System.Drawing.Imaging.ImageFormat]::Png); $b.Dispose(); ,$ms.ToArray()
}
$sizes=16,20,24,32,40,48,64,96,128,256; $imgs = @($sizes | ForEach-Object { ,(Make $_) })
$out = New-Object System.IO.MemoryStream; $w = New-Object System.IO.BinaryWriter $out
$w.Write([uint16]0); $w.Write([uint16]1); $w.Write([uint16]$sizes.Count)
$off = 6 + 16*$sizes.Count
for($i=0;$i -lt $sizes.Count;$i++){ $sz=$sizes[$i]; $n = if($sz -ge 256){0}else{$sz}; $w.Write([byte]$n);$w.Write([byte]$n);$w.Write([byte]0);$w.Write([byte]0);$w.Write([uint16]1);$w.Write([uint16]32);$w.Write([uint32]$imgs[$i].Length);$w.Write([uint32]$off); $off += $imgs[$i].Length }
foreach($im in $imgs){ $w.Write([byte[]]$im) }
New-Item -ItemType Directory -Force assets | Out-Null
[IO.File]::WriteAllBytes((Join-Path (Get-Location) 'assets\hub.ico'),$out.ToArray())
