$ErrorActionPreference = 'Stop'

$root = Resolve-Path (Join-Path $PSScriptRoot '..')
$manifestPath = Join-Path $root 'backend-rs/Cargo.toml'
$distDir = Join-Path $root 'backend-rs/dist'
$releaseExe = Join-Path $root 'backend-rs/target/release/ag-swarmer-backend.exe'
$sidecarExe = Join-Path $distDir 'ag-swarmer-backend-x86_64-pc-windows-msvc.exe'

cargo build --manifest-path $manifestPath --package ag-swarmer-backend --release
if ($LASTEXITCODE -ne 0) {
  throw "Rust backend release build failed with exit code $LASTEXITCODE."
}

if (!(Test-Path -LiteralPath $releaseExe)) {
  throw "Rust backend release executable was not found: $releaseExe"
}

New-Item -ItemType Directory -Force -Path $distDir | Out-Null
Copy-Item -LiteralPath $releaseExe -Destination $sidecarExe -Force

Write-Host "Built Rust backend sidecar: $sidecarExe"
