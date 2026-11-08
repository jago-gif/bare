# Crear ZIP portable de Bare (Bare.exe + README)
# Uso: powershell -ExecutionPolicy Bypass -File scripts\make-zip.ps1
param([string]$Out = "")
$ErrorActionPreference = "Stop"
$root = Resolve-Path (Join-Path $PSScriptRoot "..")
$exe = Join-Path $root "target\release\bare.exe"
if (-not (Test-Path $exe)) { throw "falta cargo build --release" }
if (-not $Out) { $Out = Join-Path $root ("Bare-portable-v{0}.zip" -f "1.0") }

$stage = Join-Path $env:TEMP "bare-portable"
if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
New-Item -ItemType Directory -Path $stage | Out-Null

Copy-Item $exe (Join-Path $stage "Bare.exe")
Copy-Item (Join-Path $root "README.md") (Join-Path $stage "README.md")
# WebView2Loader.dll es estatico/estandar del sistema en windows moderno; no se copia.

if (Test-Path $Out) { Remove-Item $Out -Force }
Compress-Archive -Path "$stage\*" -DestinationPath $Out -CompressionLevel Optimal
Remove-Item $stage -Recurse -Force
$z = Get-Item $Out
Write-Host ("ZIP portable: {0} ({1:N2} MB)" -f $z.FullName, ($z.Length/1MB))
