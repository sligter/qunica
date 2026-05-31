Add-Type -AssemblyName System.Drawing

$ErrorActionPreference = 'Stop'
$root = Resolve-Path (Join-Path $PSScriptRoot '..')
$iconDir = Join-Path $root 'frontend/src-tauri/icons'
New-Item -ItemType Directory -Force -Path $iconDir | Out-Null

function New-AppIconBitmap {
  param([int]$Size)

  $bitmap = New-Object System.Drawing.Bitmap $Size, $Size, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
  $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
  $graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
  $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
  $graphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality

  $rect = New-Object System.Drawing.Rectangle 0, 0, $Size, $Size
  $bg = New-Object System.Drawing.Drawing2D.LinearGradientBrush $rect,
    ([System.Drawing.Color]::FromArgb(255, 18, 31, 42)),
    ([System.Drawing.Color]::FromArgb(255, 14, 62, 69)),
    ([System.Drawing.Drawing2D.LinearGradientMode]::ForwardDiagonal)
  $graphics.FillRectangle($bg, $rect)

  $margin = [Math]::Max(2, [int]($Size * 0.09))
  $inner = New-Object System.Drawing.Rectangle $margin, $margin, ($Size - $margin * 2), ($Size - $margin * 2)
  $ringPen = New-Object System.Drawing.Pen ([System.Drawing.Color]::FromArgb(90, 180, 232, 216)), ([Math]::Max(1.0, $Size * 0.025))
  $graphics.DrawEllipse($ringPen, $inner)

  $points = @(
    @([double]0.50, [double]0.19),
    @([double]0.77, [double]0.34),
    @([double]0.77, [double]0.66),
    @([double]0.50, [double]0.81),
    @([double]0.23, [double]0.66),
    @([double]0.23, [double]0.34)
  )

  $linePen = New-Object System.Drawing.Pen ([System.Drawing.Color]::FromArgb(155, 103, 232, 209)), ([Math]::Max(1.2, $Size * 0.035))
  $accentPen = New-Object System.Drawing.Pen ([System.Drawing.Color]::FromArgb(205, 255, 181, 82)), ([Math]::Max(1.1, $Size * 0.026))

  for ($i = 0; $i -lt $points.Count; $i++) {
    $a = $points[$i]
    $b = $points[($i + 1) % $points.Count]
    $graphics.DrawLine($linePen, [float]($a[0] * $Size), [float]($a[1] * $Size), [float]($b[0] * $Size), [float]($b[1] * $Size))
  }
  $graphics.DrawLine($accentPen, [float]($points[0][0] * $Size), [float]($points[0][1] * $Size), [float]($points[3][0] * $Size), [float]($points[3][1] * $Size))
  $graphics.DrawLine($accentPen, [float]($points[4][0] * $Size), [float]($points[4][1] * $Size), [float]($points[1][0] * $Size), [float]($points[1][1] * $Size))

  $centerBrush = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 238, 247, 244))
  $nodeBrush = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 79, 224, 202))
  $hotBrush = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 255, 184, 72))
  $shadowBrush = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(80, 0, 0, 0))

  foreach ($p in $points) {
    $radius = [Math]::Max(2.0, $Size * 0.075)
    $x = [float]($p[0] * $Size - $radius)
    $y = [float]($p[1] * $Size - $radius)
    $graphics.FillEllipse($shadowBrush, $x + ($Size * 0.01), $y + ($Size * 0.012), [float]($radius * 2), [float]($radius * 2))
    $graphics.FillEllipse($nodeBrush, $x, $y, [float]($radius * 2), [float]($radius * 2))
  }

  $centerRadius = [Math]::Max(3.0, $Size * 0.13)
  $cx = [float]($Size * 0.5 - $centerRadius)
  $cy = [float]($Size * 0.5 - $centerRadius)
  $graphics.FillEllipse($shadowBrush, $cx + ($Size * 0.012), $cy + ($Size * 0.015), [float]($centerRadius * 2), [float]($centerRadius * 2))
  $graphics.FillEllipse($centerBrush, $cx, $cy, [float]($centerRadius * 2), [float]($centerRadius * 2))

  $slashPen = New-Object System.Drawing.Pen ([System.Drawing.Color]::FromArgb(255, 31, 55, 64)), ([Math]::Max(1.4, $Size * 0.045))
  $slashPen.StartCap = [System.Drawing.Drawing2D.LineCap]::Round
  $slashPen.EndCap = [System.Drawing.Drawing2D.LineCap]::Round
  $graphics.DrawLine($slashPen, [float]($Size * 0.45), [float]($Size * 0.57), [float]($Size * 0.54), [float]($Size * 0.43))
  $graphics.DrawLine($slashPen, [float]($Size * 0.52), [float]($Size * 0.57), [float]($Size * 0.60), [float]($Size * 0.43))
  $graphics.DrawLine($slashPen, [float]($Size * 0.49), [float]($Size * 0.53), [float]($Size * 0.56), [float]($Size * 0.53))

  $graphics.Dispose()
  $bg.Dispose()
  $ringPen.Dispose()
  $linePen.Dispose()
  $accentPen.Dispose()
  $centerBrush.Dispose()
  $nodeBrush.Dispose()
  $hotBrush.Dispose()
  $shadowBrush.Dispose()
  $slashPen.Dispose()
  return $bitmap
}

function Save-Png {
  param([System.Drawing.Bitmap]$Bitmap, [string]$Path)
  $Bitmap.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
}

function Get-PngBytes {
  param([System.Drawing.Bitmap]$Bitmap)
  $stream = New-Object System.IO.MemoryStream
  $Bitmap.Save($stream, [System.Drawing.Imaging.ImageFormat]::Png)
  return $stream.ToArray()
}

$sizes = @(16, 32, 64, 128, 256)
$pngFrames = @()
foreach ($size in $sizes) {
  $bitmap = New-AppIconBitmap -Size $size
  if ($size -eq 32) { Save-Png -Bitmap $bitmap -Path (Join-Path $iconDir '32x32.png') }
  if ($size -eq 128) { Save-Png -Bitmap $bitmap -Path (Join-Path $iconDir '128x128.png') }
  if ($size -eq 256) {
    Save-Png -Bitmap $bitmap -Path (Join-Path $iconDir '128x128@2x.png')
    Save-Png -Bitmap $bitmap -Path (Join-Path $iconDir 'icon.png')
  }
  $pngFrames += ,@{ Size = $size; Bytes = (Get-PngBytes -Bitmap $bitmap) }
  $bitmap.Dispose()
}

$icoPath = Join-Path $iconDir 'icon.ico'
$stream = New-Object System.IO.MemoryStream
$writer = New-Object System.IO.BinaryWriter $stream
$writer.Write([UInt16]0)
$writer.Write([UInt16]1)
$writer.Write([UInt16]$pngFrames.Count)
$offset = 6 + (16 * $pngFrames.Count)
foreach ($frame in $pngFrames) {
  $sizeByte = if ($frame.Size -eq 256) { 0 } else { [byte]$frame.Size }
  $writer.Write([byte]$sizeByte)
  $writer.Write([byte]$sizeByte)
  $writer.Write([byte]0)
  $writer.Write([byte]0)
  $writer.Write([UInt16]1)
  $writer.Write([UInt16]32)
  $writer.Write([UInt32]$frame.Bytes.Length)
  $writer.Write([UInt32]$offset)
  $offset += $frame.Bytes.Length
}
foreach ($frame in $pngFrames) {
  $writer.Write([byte[]]$frame.Bytes)
}
$writer.Flush()
[System.IO.File]::WriteAllBytes($icoPath, $stream.ToArray())
$writer.Dispose()
$stream.Dispose()

Write-Host "Generated desktop icons in $iconDir"
