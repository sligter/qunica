$ErrorActionPreference = 'Stop'

$root = Resolve-Path (Join-Path $PSScriptRoot '..')
$source = Join-Path $root 'assets/qunica-logo.png'
$iconDir = Join-Path $root 'frontend/src-tauri/icons'
$tempRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
$tempDir = Join-Path $tempRoot "qunica-icons-$([guid]::NewGuid())"

New-Item -ItemType Directory -Path $tempDir | Out-Null
try {
  & pnpm --dir (Join-Path $root 'frontend') exec tauri icon $source --output $tempDir
  if ($LASTEXITCODE -ne 0) { throw "Tauri icon generation failed with exit code $LASTEXITCODE" }

  @('32x32.png', '128x128.png', '128x128@2x.png', 'icon.png', 'icon.ico', 'icon.icns') |
    ForEach-Object { Copy-Item (Join-Path $tempDir $_) (Join-Path $iconDir $_) -Force }
}
finally {
  if ([System.IO.Path]::GetFullPath($tempDir).StartsWith($tempRoot)) {
    Remove-Item -LiteralPath $tempDir -Recurse -Force -ErrorAction SilentlyContinue
  }
}
