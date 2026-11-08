# Fase 8: prueba de estabilidad automatizada de Bare.
# Escenarios:
#  valida_serv   : DNS inexistente + localhost:1 (conexion rechazada) + cert autfirmado
#  valida_pdf    : carga PDF con handler interno deshabilitado (msPdfOOUI)
#  valida_stress : 16 peticiones pesadas (8 x 2), hibernacion al perder foco, cierre limpio
# Cada escenario exige: proceso vivo al final, sin panics en el log, y cierre HWND-based.
# Uso: powershell -ExecutionPolicy Bypass -File scripts\test-fase8.ps1

param(
    [int]$SettleSec = 15,
    [int]$HibernateWaitSec = 25
)

$ErrorActionPreference = "Continue"
$exe = (Resolve-Path (Join-Path $PSScriptRoot "..\target\release\bare.exe")).Path
$log = Join-Path $env:LOCALAPPDATA "Bare\logs\bare.log"
$pre = @(Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" | Select-Object -ExpandProperty ProcessId)

function Log-Watermark { if (Test-Path $log) { (Get-Content $log | Measure-Object -Line).Lines } else { 0 } }
function Panics-Since([int]$mark) {
    if (-not (Test-Path $log)) { return @() }
    Get-Content $log | Select-Object -Skip $mark | Where-Object { $_ -match "panic|panicked|ERROR CRITICO" }
}

Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class W {
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern bool ShowWindowAsync(IntPtr h, int c);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
}
"@

function Get-PrivOne([int]$id, $pre) {
    # RAM privada de un proceso + descendencia msedgewebview2 (cierre transitivo)
    $sum = 0.0
    $pr = Get-Process -Id $id -ErrorAction SilentlyContinue
    if ($pr) { $sum += $pr.PrivateMemorySize64 / 1MB }
    $seed = New-Object System.Collections.Generic.HashSet[int]
    foreach ($p in (Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'")) {
        if ($pre -contains $p.ProcessId) { continue }
        if ($p.ParentProcessId -eq $id) { [void]$seed.Add($p.ProcessId) }
        elseif ($p.CommandLine -match "--webview-exe-name=bare") { [void]$seed.Add($p.ProcessId) }
    }
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
    foreach ($sid in $seed) {
        $p2 = Get-Process -Id $sid -ErrorAction SilentlyContinue
        if ($p2) { $sum += $p2.PrivateMemorySize64 / 1MB }
    }
    [pscustomobject]@{ Priv = $sum; WebProcs = $seed.Count }
}

$fails = @()
function Verdict($name, $ok, $detail) {
    $r = if ($ok) { "PASS" } else { "FAIL" }
    Write-Host ("[{0}] {1} -- {2}" -f $r, $name, $detail)
    if (-not $ok) { $fails += $name }
}

function Start-Case($urls) {
    $mark = Log-Watermark
    $p = if ($urls.Count -gt 0) { Start-Process -FilePath $exe -ArgumentList $urls -PassThru } `
         else { Start-Process -FilePath $exe -PassThru }
    Start-Sleep -Seconds 3
    $hwnd = (Get-Process -Id $p.Id -ErrorAction SilentlyContinue).MainWindowHandle
    [pscustomobject]@{ Proc = $p; Mark = $mark; Hwnd = $hwnd }
}

function Stop-Case($case) {
    # cierre por HWND capturado en el arranque (no en el estado actual de la ventana)
    if ($case.Hwnd -ne [IntPtr]::Zero) {
        [void][W]::PostMessage($case.Hwnd, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero)
    }
    Start-Sleep -Seconds 5
    $left = @(Get-Process -Name bare -ErrorAction SilentlyContinue).Count
    return $left
}

Write-Host "== test-fase8: $exe"
Write-Host ("   log: {0}" -f $log)

# --- Escenario 1: fallos de red y certificado ---
Write-Host "`n-- Escenario valida_serv: DNS inexistente + puerto cerrado + cert invalido"
$case = Start-Case @("https://dominio-inexistente-qw8jz2.invalid", "https://localhost:1", "https://self-signed.badssl.com", "http://10.255.255.1/")
Start-Sleep -Seconds $SettleSec
$still = Get-Process -Id $case.Proc.Id -ErrorAction SilentlyContinue
$fam = Get-PrivOne $case.Proc.Id $pre
Verdict "vive tras $SettleSec s con 4 fallos de red" ($null -ne $still) ("priv {0:N1} MB | webprocs {1}" -f $fam.Priv, $fam.WebProcs)
$panic = Panics-Since $case.Mark
Verdict "sin panics (fase serv)" ($panic.Count -eq 0) ($panic -join " | ")
$left = Stop-Case $case
Verdict "cierre limpio tras WM_CLOSE (0 procesos bare)" ($left -eq 0) "restantes: $left"

# --- Escenario 2: PDF ---
Write-Host "`n-- Escenario valida_pdf"
$case = Start-Case @("https://www.w3.org/WAI/ER/tests/xhtml/testfiles/resources/pdf/dummy.pdf")
Start-Sleep -Seconds $SettleSec
$still = Get-Process -Id $case.Proc.Id -ErrorAction SilentlyContinue
Verdict "vive tras PDF (handler interno deshabilitado)" ($null -ne $still) "pid $($case.Proc.Id)"
$panic = Panics-Since $case.Mark
Verdict "sin panics (fase pdf)" ($panic.Count -eq 0) ($panic -join " | ")
$left = Stop-Case $case
Verdict "cierre limpio tras PDF" ($left -eq 0) "restantes: $left"

# --- Escenario 3: stress 16 peticiones + hibernacion + cierre ---
Write-Host "`n-- Escenario valida_stress: 16 peticiones, hibernacion, cierre"
$sites = @("https://www.clarin.com","https://www.infobae.com","https://www.lanacion.com.ar","https://www.milenio.com","https://www.eluniverso.com","https://www.eltiempo.com","https://www.20minutos.es","https://elpais.com")
$sites16 = $sites + $sites
$case = Start-Case $sites16
$sw = $case.Proc
Start-Sleep -Seconds $SettleSec
$s1 = Get-PrivOne $sw.Id $pre
Write-Host ("   tras carga: priv {0:N1} MB | {1} procs web" -f $s1.Priv, $s1.WebProcs)
# esperar al temporizador de hibernacion (45 s desde la ultima activacion)
Start-Sleep -Seconds $HibernateWaitSec
$s2 = Get-PrivOne $sw.Id $pre
$hib = @(Get-Content $log | Select-Object -Skip $case.Mark | Where-Object { $_ -match "hibernando pesta" }).Count
Write-Host ("   tras +HibernateWaitSec: priv {0:N1} MB | hibernaciones en log: {1}" -f $s2.Priv, $hib)
Verdict "hibernador abrio >=14 pestanas a about:blank" ($hib -ge 14) "lineas hibernando: $hib"
$still = Get-Process -Id $sw.Id -ErrorAction SilentlyContinue
Verdict "proceso vivo tras stress + hibernacion" ($null -ne $still) "pid $($sw.Id)"
$panic = Panics-Since $case.Mark
Verdict "sin panics (fase stress)" ($panic.Count -eq 0) ($panic -join " | ")
$left = Stop-Case $case
Verdict "cierre limpio tras stress" ($left -eq 0) "restantes: $left"

# resumen
Write-Host ""
if ($fails.Count -eq 0) {
    Write-Host "== RESULTADO: TODOS LOS ESCENARIOS PASS"
    exit 0
} else {
    Write-Host ("== RESULTADO: {0} escenario(s) FALLaron: {1}" -f $fails.Count, ($fails -join ", "))
    exit 1
}
