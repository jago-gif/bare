param(
  [string]$ExePath = (Join-Path $PSScriptRoot "..\target\debug\bare.exe"),
  [int]$Seconds = 20,
  [int]$IntervalSec = 2,
  [string]$CsvPath = "",
  [string]$Etiqueta = "medicion"
)

function Get-TreeRss([int]$RootPid) {
  $pids = New-Object System.Collections.Generic.HashSet[int]
  $pids.Add($RootPid) | Out-Null
  $pending = New-Object System.Collections.Generic.List[int]
  $pending.Add($RootPid)
  while ($pending.Count -gt 0) {
    $cur = $pending[0]; $pending.RemoveAt(0)
    Get-CimInstance Win32_Process -Filter "ParentProcessId=$cur" -ErrorAction SilentlyContinue | ForEach-Object {
      if ($pids.Add([int]$_.ProcessId)) { $pending.Add([int]$_.ProcessId) }
    }
  }
  $bytes = 0L
  foreach ($p in $pids) {
    $gp = Get-Process -Id $p -ErrorAction SilentlyContinue
    if ($gp) { $bytes += $gp.WorkingSet64 }
  }
  [math]::Round($bytes / 1MB, 1)
}

$proc = Start-Process -FilePath $ExePath -PassThru
Start-Sleep -Milliseconds 1500
$rows = @()
$deadline = (Get-Date).AddSeconds($Seconds)
while ((Get-Date) -lt $deadline) {
  if ($proc.HasExited) { Write-Host "El proceso salio prematuramente (exit $($proc.ExitCode))"; break }
  $rss = Get-TreeRss $proc.Id
  $ts = (Get-Date).ToString("HH:mm:ss")
  $row = [pscustomobject]@{ t=$ts; etiqueta=$Etiqueta; rss_mb=$rss }
  $rows += $row
  "{0}  [{1}]  RSS total (Rust + WebView2): {2} MB" -f $ts, $Etiqueta, $rss
  Start-Sleep -Seconds $IntervalSec
}
if (-not $proc.HasExited) { Stop-Process -Id $proc.Id }
if ($CsvPath) { $rows | Export-Csv -NoTypeInformation -Encoding UTF8 $CsvPath; "CSV: $CsvPath" }
$vals = $rows | ForEach-Object { $_.rss_mb }
if ($vals) { "RESUMEN [{0}]: media {1} MB | mediana {2} MB | max {3} MB | muestras {4}" -f $Etiqueta, ([math]::Round(($vals | Measure-Object -Average).Average,1)), ([math]::Round(($vals | Sort-Object)[[math]::Floor(($vals.Count-1)/2)] ,1)), ($vals | Measure-Object -Maximum).Maximum, $vals.Count }
