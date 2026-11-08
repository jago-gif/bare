$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Drawing

$root = Split-Path -Parent $PSScriptRoot
$assets = Join-Path $root "assets"
New-Item -ItemType Directory -Force $assets | Out-Null

$iconSrc = Join-Path $root "icono.jpg"
$logoSrc = Join-Path $root "Logotipo .png"

function New-ResizedBmp([System.Drawing.Image]$src, [int]$size) {
  $bmp = New-Object System.Drawing.Bitmap($size, $size, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
  $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
  $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
  $g.DrawImage($src, 0, 0, $size, $size)
  $g.Dispose()
  return $bmp
}

$img = [System.Drawing.Image]::FromFile($iconSrc)

[IO.File]::WriteAllBytes((Join-Path $assets "icono.ico"), [byte[]]::new(0))
$ms = New-Object System.IO.MemoryStream
$w = New-Object System.IO.BinaryWriter($ms)
$w.Write([uint16]0); $w.Write([uint16]1); $w.Write([uint16]4)

$pngs = @()
foreach ($s in @(16, 32, 48, 256)) {
  $bmp = New-ResizedBmp $img $s
  $pms = New-Object System.IO.MemoryStream
  $bmp.Save($pms, [System.Drawing.Imaging.ImageFormat]::Png)
  $pngs += ,@($s, $pms.ToArray())
  $pms.Dispose(); $bmp.Dispose()
}

$offset = 6 + 16 * $pngs.Count
foreach ($p in $pngs) {
  $s = $p[0]; $data = $p[1]
  $w.Write([byte]($(if ($s -ge 256) { 0 } else { $s })))
  $w.Write([byte]($(if ($s -ge 256) { 0 } else { $s })))
  $w.Write([byte]0); $w.Write([byte]0)
  $w.Write([uint16]1); $w.Write([uint16]32)
  $w.Write([uint32]$data.Length); $w.Write([uint32]$offset)
  $offset += $data.Length
}
foreach ($p in $pngs) { $w.Write($p[1]); $p[1] = $null }
$w.Flush()
[IO.File]::WriteAllBytes((Join-Path $assets "icono.ico"), $ms.ToArray())
$w.Dispose(); $ms.Dispose()
Write-Host "ICO: $((Get-Item (Join-Path $assets 'icono.ico')).Length) bytes"

$bmp32 = New-ResizedBmp $img 32
$rect = New-Object System.Drawing.Rectangle(0, 0, 32, 32)
$bd = $bmp32.LockBits($rect, [System.Drawing.Imaging.ImageLockMode]::ReadOnly, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
$bytes = New-Object byte[] (32 * 32 * 4)
[System.Runtime.InteropServices.Marshal]::Copy($bd.Scan0, $bytes, 0, $bytes.Length)
$bmp32.UnlockBits($bd)
$bmp32.Dispose()
for ($i = 0; $i -lt $bytes.Length; $i += 4) {
  $b = $bytes[$i]; $bytes[$i] = $bytes[$i + 2]; $bytes[$i + 2] = $b
}
[IO.File]::WriteAllBytes((Join-Path $assets "icon.window.rgba"), $bytes)
Write-Host "RGBA ventana: 32x32 => $((Get-Item (Join-Path $assets 'icon.window.rgba')).Length) bytes"

$logoImg = [System.Drawing.Image]::FromFile($logoSrc)
$logoW = 800
$logoH = [int]([math]::Round($logoImg.Height * $logoW / $logoImg.Width))
$lb = New-Object System.Drawing.Bitmap($logoW, $logoH, [System.Drawing.Imaging.PixelFormat]::Format24bppRgb)
$lg = [System.Drawing.Graphics]::FromImage($lb)
$lg.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
$lg.DrawImage($logoImg, 0, 0, $logoW, $logoH)
$lg.Dispose()
$lms = New-Object System.IO.MemoryStream
$lb.Save($lms, [System.Drawing.Imaging.ImageFormat]::Png)
$lb.Dispose()
[Convert]::ToBase64String($lms.ToArray()) | Set-Content (Join-Path $assets "logo.b64") -NoNewline -Encoding Ascii
$lms.Dispose()
Write-Host "Logo base64: $((Get-Item (Join-Path $assets 'logo.b64')).Length) bytes"

$img.Dispose(); $logoImg.Dispose()
