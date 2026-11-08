# Medida de RAM del navegador Bare (frio y bajo carga).
# Uso:
#   .\scripts\medir-ram.ps1
#   .\scripts\medir-ram.ps1 -SiteList "https://clarin.com,https://infobae.com"
#   .\scripts\medir-ram.ps1 -Urls ... -Samples 5 -SettleSec 30 -NoKill
# Los msedgewebview2 preexistentes al arranque se excluyen.

param(
    # lista de sitios separados por coma (evita problemas de binding de arrays en -File)
    [string]$SiteList = "",
    [int]$Samples = 3,
    [int]$SettleSec = 20,
    [int]$IntervalSec = 4,
    [switch]$NoKill
)

$Urls = @($SiteList -split "\s*,\s*" | Where-Object { $_ })

$ErrorActionPreference = "Continue"
$exe = Join-Path $PSScriptRoot "..\target\release\bare.exe"
if (-not (Test-Path $exe)) { $exe = Join-Path $PSScriptRoot "..\target\debug\bare.exe" }
if (-not (Test-Path $exe)) { Write-Error "bare.exe no compilado (cargo build --release)"; exit 1 }
$exe = (Resolve-Path $exe).Path

# snapshot de msedgewebview2 preexistentes (otros apps con WebView2)
$pre = @(Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" |
    Select-Object -ExpandProperty ProcessId)

function Get-Family([int]$mainPid) {
    # seed: webviews con padre = main o marcados con webview-exe-name=bare
    $seed = New-Object System.Collections.Generic.HashSet[int]
    foreach ($p in (Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'")) {
        if ($pre -contains $p.ProcessId) { continue }
        if ($p.ParentProcessId -eq $mainPid) { [void]$seed.Add($p.ProcessId) }
        elseif ($p.CommandLine -match "--webview-exe-name=bare") { [void]$seed.Add($p.ProcessId) }
    }
    # cierre transitivo: hijos del browser-process de esta instancia
    if ($seed.Count -gt 0) {
        for (;;) {
            $added = $false
            foreach ($p in (Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'")) {
                if ($pre -contains $p.ProcessId) { continue }
                if (-not $seed.Contains($p.ProcessId) -and $seed.Contains($p.ParentProcessId)) {
                    [void]$seed.Add($p.ProcessId); $added = $true
                }
            }
            if (-not $added) { break }
        }
    }

    $sum = 0.0; $ws = 0.0; $n = 0
    $ids = @($mainPid) + @($seed)
    foreach ($id in $ids) {
        $pr = Get-Process -Id $id -ErrorAction SilentlyContinue
        if ($pr) { $sum += $pr.PrivateMemorySize64 / 1MB; $ws += $pr.WorkingSet64 / 1MB; $n++ }
    }
    [pscustomobject]@{ Procs = $n; Private = $sum; WorkingSet = $ws }
}

Write-Host "== Bare: $exe"
Write-Host "== Lanzando con $($Urls.Count) pestana(s); espera de estabilizacion: $SettleSec s..."
if ($Urls.Count -gt 0) {
    $main = Start-Process -FilePath $exe -ArgumentList $Urls -PassThru
} else {
    $main = Start-Process -FilePath $exe -PassThru
}
Start-Sleep -Seconds $SettleSec

$root = $main.Id
if (-not (Get-Process -Id $root -ErrorAction SilentlyContinue)) {
    $alt = Get-Process -Name bare -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($alt -and $alt.Id -ne $main.Id) { Write-Host "  (PID inicial $main.Id murio; usando el bare vivo $($alt.Id) -no nuestro)"; }
    else { Write-Host "  (PID inicial $main.Id murio; sin procesos bare)"; }
}

$runs = @()
for ($i = 1; $i -le $Samples; $i++) {
    $f = Get-Family $root
    $runs += $f
    Write-Host ("  muestra {0}/{1}: {2} procesos | priv {3:N1} MB | ws {4:N1} MB" -f $i, $Samples, $f.Procs, $f.Private, $f.WorkingSet)
    if ($i -lt $Samples) { Start-Sleep -Seconds $IntervalSec }
}

# desglose: tipo de proceso (de cmdline de msedgewebview2) y RAM privada
Write-Host ""
Write-Host "== Desglose por proceso (ultima muestra)"
foreach ($p in (Get-Process -Name msedgewebview2 -ErrorAction SilentlyContinue)) {
    $cim = Get-CimInstance Win32_Process -Filter "ProcessId=$($p.Id)"
    if (-not $cim) { continue }
    if ($pre -contains $p.Id) { continue }
    $cmd = $cim.CommandLine
    $tipo = if ($cmd -match "--type=([a-z-]+)") { $Matches[1] } elseif ($cim.ParentProcessId -eq $root) { "browser" } else { "otro" }
    $rama = if ($cmd -match "--utility-sub-type=([a-zA-Z0-9_.]+)") { " ($($Matches[1]))" } else { "" }
    Write-Host ("   {0,-12}{1,-24} priv {2,8:N1} MB | ws {3,8:N1} MB" -f $tipo, $rama, ($p.PrivateMemorySize64/1MB), ($p.WorkingSet64/1MB))
}
$bareMain = Get-Process -Id $root -ErrorAction SilentlyContinue
if ($bareMain) { Write-Host ("   {0,-12}{1,-24} priv {2,8:N1} MB | ws {3,8:N1} MB" -f "main", "(bare.exe)", ($bareMain.PrivateMemorySize64/1MB), ($bareMain.WorkingSet64/1MB)) }

$tot = $runs | Measure-Object Private -Average -Maximum
$wst = $runs | Measure-Object WorkingSet -Average -Maximum
Write-Host ""
Write-Host "== RESULTADO ($($Urls.Count) pestana(s), estabilizado)"
Write-Host ("   RAM privada: media {0:N1} MB | pico {1:N1} MB" -f $tot.Average, $tot.Maximum)
Write-Host ("   WorkingSet:  media {0:N1} MB | pico {1:N1} MB" -f $wst.Average, $wst.Maximum)
Write-Host ("   Linea: 0 pestana examen previsto < 200 MB | 8 sitios de noticias < 350 MB")

if (-not $NoKill) {
    Get-Process -Name bare -ErrorAction SilentlyContinue | Stop-Process -Force
    Write-Host "== procesos bare cerrados"
}
