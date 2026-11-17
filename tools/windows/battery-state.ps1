# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# Battery and charger state as Windows sees it: WMI root\wmi battery classes (the battery class driver's view of
# SurfaceBattery.sys), Win32_Battery, AC/charge flags. Read-only, no admin needed. Serial number and unique ID are masked.
#
#   powershell -ExecutionPolicy Bypass -File tools\windows\battery-state.ps1 [-Watch SECONDS] [-Report]
#
# -Watch N prints one status line every N seconds until Ctrl+C (charge flags, rate, capacity), for watching
#   what happens at a charge limit or on plug/unplug.
# -Report also writes `powercfg /batteryreport /xml` to private/windows/power/ (it holds the battery serial).
param([int]$Watch = 0, [switch]$Report)

$ErrorActionPreference = 'SilentlyContinue'

function Mask($s) { if ($s) { ($s -replace '[0-9A-Za-z]', 'x') } }

function StatusLine {
    $st = Get-CimInstance -Namespace root\wmi -ClassName BatteryStatus | Select-Object -First 1
    $fc = (Get-CimInstance -Namespace root\wmi -ClassName BatteryFullChargedCapacity | Select-Object -First 1).FullChargedCapacity
    $pct = if ($fc) { [math]::Round(100 * $st.RemainingCapacity / $fc, 1) } else { $null }
    $net = $st.ChargeRate - $st.DischargeRate
    "{0:HH:mm:ss} online={1} charging={2} discharging={3} critical={4} rem={5}mWh full={6}mWh ({7}%) rate={8}mW chg={9} dis={10} V={11}mV" -f `
        (Get-Date), [int]$st.PowerOnline, [int]$st.Charging, [int]$st.Discharging, [int]$st.Critical,
        $st.RemainingCapacity, $fc, $pct, $net, $st.ChargeRate, $st.DischargeRate, $st.Voltage
}

if ($Watch -gt 0) { while ($true) { StatusLine; Start-Sleep -Seconds $Watch } }

"## BatteryStaticData (root\wmi; 'Generic failure' on SurfaceBattery means the query is refused as a whole)"
Get-CimInstance -Namespace root\wmi -ClassName BatteryStaticData | ForEach-Object {
    "DeviceName={0} Manufacturer={1} Chemistry=0x{2:X8} Technology={3} Capabilities=0x{4:X8}" -f $_.DeviceName, $_.ManufactureName, $_.Chemistry, $_.Technology, $_.Capabilities
    "DesignedCapacity={0}mWh DefaultAlert1={1} DefaultAlert2={2} CriticalBias={3} Granularity={4}/{5}/{6}/{7}" -f $_.DesignedCapacity, $_.DefaultAlert1, $_.DefaultAlert2, $_.CriticalBias, $_.Granularity1, $_.Granularity2, $_.Granularity3, $_.Granularity4
    "SerialNumber={0} UniqueID={1} ManufactureDate={2}" -f (Mask $_.SerialNumber), (Mask $_.UniqueID), $_.ManufactureDate
}
"`n## BatteryFullChargedCapacity / BatteryCycleCount / BatteryTemperature / BatteryRuntime"
"FullChargedCapacity={0}mWh" -f (Get-CimInstance -Namespace root\wmi -ClassName BatteryFullChargedCapacity).FullChargedCapacity
"CycleCount={0}" -f (Get-CimInstance -Namespace root\wmi -ClassName BatteryCycleCount).CycleCount
$t = Get-CimInstance -Namespace root\wmi -ClassName BatteryTemperature
"Temperature={0} (tenths of K; empty = driver does not report)" -f $t.Temperature
"EstimatedRuntime={0}s" -f (Get-CimInstance -Namespace root\wmi -ClassName BatteryRuntime).EstimatedRuntime

"`n## BatteryStatus"
StatusLine

"`n## Win32_Battery"
Get-CimInstance Win32_Battery | ForEach-Object {
    "Name={0} Status={1} BatteryStatus={2} EstimatedChargeRemaining={3}% Chemistry={4} DesignVoltage={5}mV" -f $_.Name, $_.Status, $_.BatteryStatus, $_.EstimatedChargeRemaining, $_.Chemistry, $_.DesignVoltage
}

"`n## Power source (SystemPowerStatus)"
Add-Type -TypeDefinition @'
using System.Runtime.InteropServices;
public static class Sps {
    [StructLayout(LayoutKind.Sequential)] public struct S { public byte AC, Flag, Pct, Saver; public int Life, Full; }
    [DllImport("kernel32.dll")] public static extern bool GetSystemPowerStatus(out S s);
}
'@
$s = New-Object Sps+S
[void][Sps]::GetSystemPowerStatus([ref]$s)
# BatteryFlag: 1 high, 2 low, 4 critical, 8 charging, 128 no battery. SystemStatusFlag (here Saver): 1 = battery saver on
"ACLineStatus={0} BatteryFlag=0x{1:X2} Percent={2} BatterySaver={3}" -f $s.AC, $s.Flag, $s.Pct, $s.Saver

if ($Report) {
    $root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
    $dir = Join-Path $root 'private\windows\power'
    New-Item -ItemType Directory -Force $dir | Out-Null
    $out = Join-Path $dir 'battery-report.xml'
    powercfg /batteryreport /xml /output $out | Out-Null
    "`nbattery report written to $out"
}
