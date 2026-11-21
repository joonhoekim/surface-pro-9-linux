# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# Windows performance counters for perf-modes.ps1, about once a second, to <Dir>\counters.csv (t = Unix seconds) until <Dir>\stop.
# Effective clock (% Processor Performance x nominal), utility, RAPL energy meters (mW), the 12 ACPI thermal zones
# (TPOL + MSHW0188 TZ01-TZ0B = SAM sensors RTS1-8, VTS1-3; K) with their passive limit, and the PPM performance-limit counters.
# Battery state (root\wmi BatteryStatus) is appended on each row.
param([Parameter(Mandatory)][string]$Dir)
$ErrorActionPreference = 'SilentlyContinue'
$inv = [Globalization.CultureInfo]::InvariantCulture
$csv = Join-Path $Dir 'counters.csv'; $stop = Join-Path $Dir 'stop'
$paths = '\Processor Information(*)\% Processor Performance', '\Processor Information(_Total)\% Processor Utility',
    '\Processor Information(_Total)\Processor Frequency', '\Processor Information(_Total)\% Performance Limit',
    '\Processor Information(_Total)\Performance Limit Flags', '\Energy Meter(*)\Power',
    '\Thermal Zone Information(*)\High Precision Temperature', '\Thermal Zone Information(*)\% Passive Limit'
$header = $null
while (-not (Test-Path $stop)) {
    $s = (Get-Counter -Counter $paths -SampleInterval 1 -MaxSamples 1).CounterSamples
    if (-not $s) { Start-Sleep 1; continue }
    $b = Get-CimInstance -Namespace root\wmi -ClassName BatteryStatus | Select-Object -First 1
    $t = ([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() / 1000.0).ToString('F3', $inv)
    $names = $s | ForEach-Object { ($_.Path -replace '^\\\\[^\\]+\\', '') -replace ',', ';' }
    if (-not $header) {
        $header = $names
        Add-Content -Encoding utf8 $csv ((@('t') + $names + @('power_online', 'charge_rate_mw', 'discharge_rate_mw', 'remaining_mwh')) -join ',')
    }
    $vals = $s | ForEach-Object { ([double]$_.CookedValue).ToString('0.###', $inv) }
    Add-Content -Encoding utf8 $csv ((@($t) + $vals + @([int]$b.PowerOnline, $b.ChargeRate, $b.DischargeRate, $b.RemainingCapacity)) -join ',')
}
