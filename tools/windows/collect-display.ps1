# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# Display/panel control state on Windows: driver registry, OS display configuration, panel calibration UEFI variables,
# WMI brightness, video power settings, ETW providers. Read-only.
# Usage: powershell -ExecutionPolicy Bypass -File tools\windows\run-elevated.ps1 tools\windows\collect-display.ps1
#        (without elevation the GraphicsDrivers\Configuration keys and UEFI variables are skipped)
# Output: private/windows/display/ (configuration keys carry instance paths, UEFI calibration is per-unit measurement data).
param([string]$Out)
$root = Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path))
if (-not $Out) { $Out = Join-Path $root 'private\windows\display' }
New-Item -ItemType Directory -Force $Out | Out-Null
$admin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)

function Format-Value($v) {
    if ($v -is [byte[]]) { return 'hex:' + (($v | ForEach-Object { '{0:x2}' -f $_ }) -join '') }
    if ($v -is [int] -or $v -is [long]) { return '{0} (0x{0:X})' -f $v }
    if ($v -is [array]) { return ($v -join ' | ') }
    return "$v"
}
function Dump-Key($path, [switch]$Recurse) {
    $keys = @(Get-Item $path -ErrorAction SilentlyContinue)
    if ($Recurse) { $keys += Get-ChildItem $path -Recurse -ErrorAction SilentlyContinue }
    foreach ($k in $keys) {
        "[$($k.Name)]"
        foreach ($n in $k.GetValueNames()) { '  {0} = {1}' -f $n, (Format-Value $k.GetValue($n)) }
    }
}

# 1. Driver registry: Intel display adapter and monitor (panel) class keys
$cls = 'HKLM:\SYSTEM\CurrentControlSet\Control\Class'
& {
    Get-ChildItem "$cls\{4d36e968-e325-11ce-bfc1-08002be10318}" -ErrorAction SilentlyContinue | ForEach-Object { Dump-Key $_.PSPath -Recurse }
    Get-ChildItem "$cls\{4d36e96e-e325-11ce-bfc1-08002be10318}" -ErrorAction SilentlyContinue | ForEach-Object { Dump-Key $_.PSPath }
} | Out-File -Encoding utf8 (Join-Path $Out 'class-keys.txt')

# 2. OS display configuration (refresh rate, scaling, advanced color) and monitor data store
$gd = 'HKLM:\SYSTEM\CurrentControlSet\Control\GraphicsDrivers'
& {
    foreach ($s in 'Configuration', 'Connectivity', 'MonitorDataStore', 'ScaleFactors', 'InternalMonEdid', 'AdditionalModeLists', 'DCI') {
        Dump-Key "$gd\$s" -Recurse
    }
    Dump-Key 'HKCU:\Software\Microsoft\DirectX' -Recurse
    Dump-Key 'HKCU:\Software\Microsoft\Windows NT\CurrentVersion\ICM' -Recurse
    Dump-Key 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\ICM' -Recurse
} | Out-File -Encoding utf8 (Join-Path $Out 'graphicsdrivers.txt')

# 3. Panel calibration UEFI variables read by SurfaceOemPanel.dll (GetFirmwareEnvironmentVariableW)
if ($admin) {
    Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class Fw {
  [DllImport("kernel32.dll", SetLastError=true, CharSet=CharSet.Unicode)]
  public static extern uint GetFirmwareEnvironmentVariableExW(string name, string guid, byte[] buf, uint size, out uint attr);
  [DllImport("advapi32.dll", SetLastError=true)] static extern bool OpenProcessToken(IntPtr h, uint acc, out IntPtr tok);
  [DllImport("advapi32.dll", SetLastError=true, CharSet=CharSet.Unicode)] static extern bool LookupPrivilegeValue(string sys, string name, out long luid);
  [StructLayout(LayoutKind.Sequential, Pack=4)] struct TP { public uint Count; public long Luid; public uint Attr; }
  [DllImport("advapi32.dll", SetLastError=true)] static extern bool AdjustTokenPrivileges(IntPtr tok, bool dis, ref TP n, uint len, IntPtr p, IntPtr r);
  [DllImport("kernel32.dll")] static extern IntPtr GetCurrentProcess();
  public static bool Enable() { IntPtr t; if (!OpenProcessToken(GetCurrentProcess(), 0x28, out t)) return false;
    TP tp = new TP(); tp.Count = 1; tp.Attr = 2; if (!LookupPrivilegeValue(null, "SeSystemEnvironmentPrivilege", out tp.Luid)) return false;
    return AdjustTokenPrivileges(t, false, ref tp, 0, IntPtr.Zero, IntPtr.Zero); }
}
'@
    [Fw]::Enable() | Out-Null
    $vars = @(
        @('{0C3B397F-4162-4ED2-9AAA-5BAF400E3AAF}', 'Type_1'),
        @('{0C3B397F-4162-4ED2-9AAA-5BAF400E3AAF}', 'Type_2'),
        @('{0C3B397F-4162-4ED2-9AAA-5BAF400E3AAF}', 'Type_6'),
        @('{CC32A53F-C206-4FF2-836E-F1980799EC47}', 'MsPlatformDisplayInfoVariable')
    )
    $sha = [Security.Cryptography.SHA256]::Create()
    & {
        foreach ($v in $vars) {
            $buf = New-Object byte[] 1048576; $attr = 0
            $n = [Fw]::GetFirmwareEnvironmentVariableExW($v[1], $v[0], $buf, $buf.Length, [ref]$attr)
            $err = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
            if ($n -eq 0) { '{0}-{1}: not readable (Win32 error {2})' -f $v[1], $v[0], $err; continue }
            $data = $buf[0..($n - 1)]
            $file = Join-Path $Out ('uefi-{0}-{1}.bin' -f $v[1], $v[0].Trim('{}').ToLower())
            [IO.File]::WriteAllBytes($file, [byte[]]$data)
            '{0}-{1}: {2} B, attributes 0x{3:X}, sha256 {4}' -f $v[1], $v[0], $n, $attr, (($sha.ComputeHash([byte[]]$data) | ForEach-Object { '{0:x2}' -f $_ }) -join '')
        }
    } | Tee-Object -FilePath (Join-Path $Out 'uefi-vars.txt')
} else { 'not elevated: UEFI variables skipped' }

# 4. WMI brightness and monitor classes
& {
    foreach ($c in 'WmiMonitorBrightness', 'WmiMonitorBasicDisplayParams', 'WmiMonitorColorCharacteristics', 'WmiMonitorListedSupportedSourceModes') {
        "== $c"
        try { Get-CimInstance -Namespace root\wmi -ClassName $c -ErrorAction Stop | Format-List * | Out-String -Width 300 } catch { "  $($_.Exception.Message)" }
    }
    '== root\wmi classes matching monitor/brightness/display'
    Get-CimClass -Namespace root\wmi | Where-Object { $_.CimClassName -match 'Monitor|Bright|Display|Backlight|Panel' } | Select-Object -ExpandProperty CimClassName
} | Out-File -Encoding utf8 (Join-Path $Out 'wmi.txt')

# 5. Video power settings (adaptive brightness, dimming) for the active scheme
powercfg /qh SCHEME_CURRENT SUB_VIDEO | Out-File -Encoding utf8 (Join-Path $Out 'powercfg-video.txt')

# 6. ETW providers related to display, panel, brightness
logman query providers | Select-String -Pattern 'Display|Dxg|Graphics|Brightness|Panel|Monitor|Surface|Intel|igfx|Color|Dwm|Backlight' |
    ForEach-Object { $_.Line.Trim() } | Out-File -Encoding utf8 (Join-Path $Out 'etw-providers.txt')

Get-ChildItem $Out | Select-Object Name, Length | Format-Table -AutoSize | Out-String
