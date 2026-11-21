# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# Windows power-mode characterization: for each power-mode overlay, idle -> all-core load -> cool-down, sampled every second.
# Needs admin (MSR reads through PawnIO, ETW session). Run: powershell -File tools\windows\run-elevated.ps1 tools\windows\perf-modes.ps1
# Requires: winget LibreHardwareMonitor.LibreHardwareMonitor and namazso.PawnIO (MSR access, read only), Python 3 for the summary.
#
# The only system change is the power-mode overlay (Settings > Power > Power mode). The overlay in effect at start is restored
# in finally{} (normal end, error, Ctrl-C). Power plans, PL values and everything else are only read.
#
# Raw data (device-specific): private/windows/perf/<stamp>-<ac|battery>/
#   markers.csv      phase boundaries        msr.csv        PL1/PL2, package power/temp, per-CPU ratio (perfprobe sample)
#   counters.csv     Windows counters, SAM thermal zones, battery     smf.csv   SMF CPU "control update" events in the run
#   msr-<mode>.txt   MSR snapshot after the mode switch               surface.etl / etw.csv   Surface driver ETW (unless -NoEtw)
#   nodes.csv        SMF node values (SurfacePowerTrackerCore ETW, written by summarize.py)
# Summary (no unique ids): hw/windows/perf/<ac|battery>-<mode>.md and hw/windows/perf/README.md (tools/windows/perf/summarize.py).
param(
    [string[]]$Modes = @('efficiency', 'balanced', 'better', 'best'),
    [int]$IdleSec = 60, [int]$LoadSec = 120, [int]$CoolSec = 60, [int]$SettleSec = 15,
    [switch]$DefaultQos,   # leave EcoQoS to Windows for the load process (default: opt out, like a foreground app)
    [switch]$NoEtw
)
$ErrorActionPreference = 'Stop'
$repo = Resolve-Path (Join-Path $PSScriptRoot '..\..')
$overlays = [ordered]@{
    efficiency = '961cc777-2547-4f9d-8174-7d86181b8a7a'   # Best power efficiency
    balanced   = '00000000-0000-0000-0000-000000000000'   # Balanced (no overlay)
    better     = '3af9b8d9-7c97-431d-ad78-34a8bfea439f'   # Better performance (Surface's AC default)
    best       = 'ded574b5-45a0-4f42-8737-46345c09c238'   # Best performance
}
foreach ($m in $Modes) { if (-not $overlays.Contains($m)) { throw "unknown mode $m (use $($overlays.Keys -join ', '))" } }
if (-not ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole('Administrators')) { throw 'run elevated (tools\windows\run-elevated.ps1)' }

# perfprobe.exe (.NET Framework csc, C# 5)
$probe = Join-Path $repo 'out\perf\perfprobe.exe'
$src = Join-Path $PSScriptRoot 'perf\perfprobe.cs'
if (-not (Test-Path $probe) -or (Get-Item $src).LastWriteTime -gt (Get-Item $probe).LastWriteTime) {
    New-Item -ItemType Directory -Force (Split-Path $probe) | Out-Null
    & (Join-Path $env:WINDIR 'Microsoft.NET\Framework64\v4.0.30319\csc.exe') /nologo /optimize+ /target:exe /platform:x64 /codepage:65001 /out:$probe $src
    if ($LASTEXITCODE) { throw "csc failed ($LASTEXITCODE)" }
}

$inv = [Globalization.CultureInfo]::InvariantCulture
function Now { ([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() / 1000.0).ToString('F3', $inv) }
$online = [bool](Get-CimInstance -Namespace root\wmi -ClassName BatteryStatus | Select-Object -First 1).PowerOnline
$power = if ($online) { 'ac' } else { 'battery' }
$raw = Join-Path $repo ("private\windows\perf\{0}-{1}" -f (Get-Date -Format 'yyyyMMdd-HHmmss'), $power)
New-Item -ItemType Directory -Force $raw | Out-Null
$markers = Join-Path $raw 'markers.csv'
Set-Content -Encoding utf8 $markers 't,mode,phase'
function Mark($mode, $phase) { Add-Content -Encoding utf8 $markers "$(Now),$mode,$phase"; Write-Host "$(Get-Date -Format HH:mm:ss) $mode $phase" }

$orig = ((& $probe overlay get) -match '^effective ') -replace '^effective ', ''
if (-not $orig) { throw 'could not read the current overlay' }
Set-Content -Encoding utf8 (Join-Path $raw 'meta.txt') @("power $power", "overlay_at_start $orig", "modes $($Modes -join ' ')",
    "idle $IdleSec load $LoadSec cool $CoolSec settle $SettleSec", "load_qos $(if ($DefaultQos) { 'default' } else { 'high (EcoQoS off)' })",
    "threads $env:NUMBER_OF_PROCESSORS", "windows $((Get-CimInstance Win32_OperatingSystem).Version)")
$t0 = Now
$jobs = @(); $etw = $false
try {
    if (-not $NoEtw) {
        logman stop sp9-perf -ets 2>$null | Out-Null
        logman start sp9-perf -ets -o (Join-Path $raw 'surface.etl') -pf (Join-Path $PSScriptRoot 'perf\etw-providers.txt') -bs 64 -nb 16 64 | Out-Null
        $etw = ($LASTEXITCODE -eq 0)
    }
    $jobs += Start-Process $probe -ArgumentList 'sample', "`"$raw`"", 1000 -PassThru -WindowStyle Hidden
    $jobs += Start-Process powershell -ArgumentList '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "`"$(Join-Path $PSScriptRoot 'perf\counters.ps1')`"", '-Dir', "`"$raw`"" -PassThru -WindowStyle Hidden
    Start-Sleep 3
    Mark 'start' $orig
    foreach ($m in $Modes) {
        Mark $m 'switch'
        & $probe overlay set $overlays[$m] | Out-Null
        Start-Sleep $SettleSec
        & $probe msr | Set-Content -Encoding utf8 (Join-Path $raw "msr-$m.txt")
        Mark $m 'idle'; Start-Sleep $IdleSec
        Mark $m 'load'
        $a = @('stress', $LoadSec); if ($DefaultQos) { $a += '--default-qos' }
        & $probe @a | Out-Null
        Mark $m 'cool'; Start-Sleep $CoolSec
    }
    Mark 'end' ''
}
finally {
    & $probe overlay set $orig | Out-Null
    Add-Content -Encoding utf8 $markers "$(Now),restore,$orig"
    New-Item -ItemType File -Force (Join-Path $raw 'stop') | Out-Null
    foreach ($j in $jobs) { if (-not $j.WaitForExit(15000)) { $j.Kill() } }
    if ($etw) { logman stop sp9-perf -ets | Out-Null }
    Write-Host "overlay restored: $((& $probe overlay get) -join ' ')"
}

# SMF CPU control updates and power-source changes inside the run window
$start = [DateTimeOffset]::FromUnixTimeMilliseconds([long]([double]$t0 * 1000)).LocalDateTime.AddSeconds(-5)
$rows = @('t,channel,instance,value')
# The client logs only changes; start a day earlier so the summary knows the values in force when the run began.
Get-WinEvent -FilterHashtable @{ LogName = 'Microsoft-Surface-SurfaceIhvCpuSmfClient/Operational'; StartTime = $start.AddDays(-1) } -ErrorAction SilentlyContinue |
    Sort-Object TimeCreated, RecordId | ForEach-Object {
        if ($_.Message -match 'Channel Index=(\d+), ChannelInstance=(\d+), Value=(\d+)') {
            $rows += '{0},{1},{2},{3}' -f ([DateTimeOffset]$_.TimeCreated).ToUnixTimeMilliseconds().ToString(), $Matches[1], $Matches[2], $Matches[3]
        }
    }
Set-Content -Encoding utf8 (Join-Path $raw 'smf.csv') $rows
Get-WinEvent -FilterHashtable @{ LogName = 'System'; ProviderName = 'Microsoft-Windows-Kernel-Power'; Id = 105; StartTime = $start } -ErrorAction SilentlyContinue |
    ForEach-Object { '{0},power_source_change' -f ([DateTimeOffset]$_.TimeCreated).ToUnixTimeMilliseconds() } | Set-Content -Encoding utf8 (Join-Path $raw 'power-source.csv')
if ($etw) { tracerpt (Join-Path $raw 'surface.etl') -of CSV -o (Join-Path $raw 'etw.csv') -y 2>$null | Out-Null }

$py = (Get-Command python -ErrorAction SilentlyContinue).Source
if ($py) { & $py -I (Join-Path $PSScriptRoot 'perf\summarize.py') $raw (Join-Path $repo 'hw\windows\perf') }
else { Write-Host "python not found; summarize later: python tools\windows\perf\summarize.py `"$raw`" hw\windows\perf" }
Write-Host "raw: $raw"
