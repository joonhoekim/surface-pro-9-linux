# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# Periodic read-only sampler for the baseline kit. Writes <Dir>\<kind>.csv (t = Unix seconds) until <Dir>\stop exists.
#   battery     root\wmi BatteryStatus (mW, mWh, mV) + Win32_Battery %         every IntervalMs (default 2000)
#   perf        % Processor Performance/Utility, frequency, RAPL power (mW), thermal zones (K)   about 1 s (Get-Counter)
#   sensors     accelerometer, gyro, inclinometer, ambient light, simple orientation (WinRT)       every IntervalMs
#   brightness  WmiMonitorBrightness current level                                                  every IntervalMs
#   presence    status of Surface HID/BLE pen/type cover device nodes (logged on change)           every IntervalMs
#   power-mode  power scheme and the Windows power-mode overlay (logged on change)                 every IntervalMs
# Usage: sampler.ps1 -Kind perf -Dir DIR [-IntervalMs 1000] [-Seconds N]
param([Parameter(Mandatory)][string]$Kind, [Parameter(Mandatory)][string]$Dir, [int]$IntervalMs = 0, [double]$Seconds = 0)
$ErrorActionPreference = 'SilentlyContinue'
. (Join-Path $PSScriptRoot 'common.ps1')
New-Item -ItemType Directory -Force $Dir | Out-Null
$stop = Join-Path $Dir 'stop'
$csv = Join-Path $Dir "$Kind.csv"
$inv = [Globalization.CultureInfo]::InvariantCulture
$t0 = Get-UnixTime
function Done { (Test-Path $stop) -or ($Seconds -gt 0 -and (Get-UnixTime) - $t0 -gt $Seconds) }
function Row([object[]]$f) { $line = ($f | ForEach-Object { if ($_ -is [double] -or $_ -is [single]) { $_.ToString('0.####', $inv) } else { "$_" } }) -join ','; Add-Content -Encoding utf8 $csv $line }
function T { (Get-UnixTime).ToString('F3', $inv) }

switch ($Kind) {
    'battery' {
        if (-not $IntervalMs) { $IntervalMs = 2000 }
        Row @('t', 'power_online', 'charging', 'discharging', 'charge_rate_mw', 'discharge_rate_mw', 'remaining_mwh', 'voltage_mv', 'percent')
        while (-not (Done)) {
            $b = Get-CimInstance -Namespace root\wmi -ClassName BatteryStatus | Select-Object -First 1
            $p = (Get-CimInstance Win32_Battery | Select-Object -First 1).EstimatedChargeRemaining
            Row @((T), [int]$b.PowerOnline, [int]$b.Charging, [int]$b.Discharging, $b.ChargeRate, $b.DischargeRate, $b.RemainingCapacity, $b.Voltage, $p)
            Start-Sleep -Milliseconds $IntervalMs
        }
    }
    'perf' {
        $paths = '\Processor Information(_Total)\% Processor Performance', '\Processor Information(_Total)\% Processor Utility',
        '\Processor Information(_Total)\Processor Frequency', '\Processor Information(_Total)\% Processor Time',
        '\Energy Meter(*)\Power', '\Thermal Zone Information(*)\High Precision Temperature'
        $header = $null
        while (-not (Done)) {
            $s = (Get-Counter -Counter $paths -SampleInterval 1 -MaxSamples 1).CounterSamples
            if (-not $s) { Start-Sleep 1; continue }
            $names = $s | ForEach-Object { ($_.Path -replace '^\\\\[^\\]+\\', '') -replace ',', ';' }
            if (-not $header) { $header = $names; Row (@('t') + $names) }
            Row (@((T)) + ($s | ForEach-Object { [double]$_.CookedValue }))
        }
    }
    'sensors' {
        if (-not $IntervalMs) { $IntervalMs = 200 }
        $null = [Windows.Devices.Sensors.Accelerometer, Windows.Devices.Sensors, ContentType = WindowsRuntime]
        $acc = [Windows.Devices.Sensors.Accelerometer]::GetDefault()
        $gyr = [Windows.Devices.Sensors.Gyrometer]::GetDefault()
        $inc = [Windows.Devices.Sensors.Inclinometer]::GetDefault()
        $als = [Windows.Devices.Sensors.LightSensor]::GetDefault()
        $ori = [Windows.Devices.Sensors.SimpleOrientationSensor]::GetDefault()
        Row @('t', 'acc_x', 'acc_y', 'acc_z', 'gyro_x', 'gyro_y', 'gyro_z', 'pitch', 'roll', 'yaw', 'lux', 'orientation')
        while (-not (Done)) {
            $a = if ($acc) { $acc.GetCurrentReading() }; $g = if ($gyr) { $gyr.GetCurrentReading() }
            $i = if ($inc) { $inc.GetCurrentReading() }; $l = if ($als) { $als.GetCurrentReading() }
            Row @((T), $a.AccelerationX, $a.AccelerationY, $a.AccelerationZ, $g.AngularVelocityX, $g.AngularVelocityY, $g.AngularVelocityZ,
                $i.PitchDegrees, $i.RollDegrees, $i.YawDegrees, $l.IlluminanceInLux, $(if ($ori) { $ori.GetCurrentOrientation() }))
            Start-Sleep -Milliseconds $IntervalMs
        }
    }
    'brightness' {
        if (-not $IntervalMs) { $IntervalMs = 250 }
        Row @('t', 'brightness'); $last = $null
        while (-not (Done)) {
            $v = (Get-CimInstance -Namespace root\wmi -ClassName WmiMonitorBrightness | Select-Object -First 1).CurrentBrightness
            if ($v -ne $last) { Row @((T), $v); $last = $v }
            Start-Sleep -Milliseconds $IntervalMs
        }
    }
    'presence' {
        if (-not $IntervalMs) { $IntervalMs = 500 }
        # Pen BLE HID children, the pen GATT HID node, type cover / KIP HID (045E:09xx), Surface Slim Pen 2 BT node
        $q = "SELECT PNPDeviceID,Name,Status,ConfigManagerErrorCode,Present FROM Win32_PnPEntity WHERE PNPDeviceID LIKE '%045E%0C0F%' OR PNPDeviceID LIKE 'HID\\VID_045E&PID_09%' OR Name LIKE '%Slim Pen%' OR Name LIKE '%Type Cover%'"
        Row @('t', 'event', 'name', 'status', 'id'); $prev = @{}
        while (-not (Done)) {
            $now = @{}
            foreach ($d in Get-CimInstance -Query $q) { $now[$d.PNPDeviceID] = "$($d.Name -replace ',', ';')|$($d.ConfigManagerErrorCode)" }
            foreach ($k in $now.Keys) { if ($prev[$k] -ne $now[$k]) { $n, $c = $now[$k] -split '\|'; Row @((T), $(if ($prev.ContainsKey($k)) { 'change' } else { 'present' }), $n, $c, $k) } }
            foreach ($k in $prev.Keys) { if (-not $now.ContainsKey($k)) { $n, $c = $prev[$k] -split '\|'; Row @((T), 'gone', $n, $c, $k) } }
            $prev = $now
            Start-Sleep -Milliseconds $IntervalMs
        }
    }
    'power-mode' {
        if (-not $IntervalMs) { $IntervalMs = 1000 }
        Row @('t', 'scheme', 'overlay_ac', 'overlay_dc'); $last = $null
        $k = 'HKLM:\SYSTEM\CurrentControlSet\Control\Power\User\PowerSchemes'
        while (-not (Done)) {
            $p = Get-ItemProperty $k
            $s = ((powercfg /getactivescheme) -replace '.*\(([^)]*)\).*', '$1') -replace ',', ';'
            $v = "$s,$($p.ActiveOverlayAcPowerScheme),$($p.ActiveOverlayDcPowerScheme)"
            if ($v -ne $last) { Add-Content -Encoding utf8 $csv "$(T),$v"; $last = $v }
            Start-Sleep -Milliseconds $IntervalMs
        }
    }
    default { "unknown kind $Kind"; exit 1 }
}
