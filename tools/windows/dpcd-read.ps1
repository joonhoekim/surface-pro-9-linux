# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# Read DPCD registers of the internal panel through the Intel Graphics Control Library (ControlLib.dll, ctlAUXAccess).
# Native AUX reads only; nothing is written. Prints hex rows per range for every display output that answers.
# Usage: powershell -ExecutionPolicy Bypass -File tools\windows\run-elevated.ps1 tools\windows\dpcd-read.ps1 [-Ranges 0x0:16,0x700:16]
param([string[]]$Ranges)
$default = @(
    '0x000:16',  # receiver caps (0x007 bit6 MSA_TIMING_PAR_IGNORED)
    '0x020:16',  # 0x02e ALPM caps
    '0x070:4',   # PSR caps
    '0x0b0:4',   # Panel Replay caps
    '0x100:16',  # link config set (0x107 bit7 MSA_TIMING_PAR_IGNORE_EN, 0x10a eDP config set)
    '0x116:2',   # ALPM config
    '0x170:4',   # PSR config set
    '0x1b0:4',   # Panel Replay config
    '0x2008:4',  # PSR status
    '0x2200:16', # extended receiver caps
    '0x2210:8',  # DPRX feature enumeration (0x2214 adaptive-sync caps)
    '0x340:16',  # Intel eDP HDR TCON caps
    '0x350:16',  # Intel eDP HDR control
    '0x400:12',  # sink OUI, device id, hw/fw rev
    '0x500:12',  # branch OUI
    '0x700:16',  # eDP caps (0x701-0x703 backlight caps)
    '0x720:16',  # eDP backlight/display control state
    '0x730:8'
)
$Ranges = @($Ranges | Where-Object { $_ -match '^0x[0-9a-fA-F]+:\d+$' })  # run-elevated.ps1 passes '' when no args are given
if (-not $Ranges) { $Ranges = $default }
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class Igcl {
  [StructLayout(LayoutKind.Sequential)] public struct InitArgs { public uint Size; public byte Version; public uint AppVersion; public uint Flags; public uint SupportedVersion; public uint U1; public ushort U2; public ushort U3; public uint U4; public uint U5; }
  [StructLayout(LayoutKind.Sequential)] public struct AuxArgs { public uint Size; public byte Version; public uint OpType; public uint Flags; public uint Address; public ulong RAD; public uint PortID; public uint DataSize;
    [MarshalAs(UnmanagedType.ByValArray, SizeConst=132)] public byte[] Data; }
  [DllImport("ControlLib.dll")] public static extern int ctlInit(ref InitArgs a, out IntPtr api);
  [DllImport("ControlLib.dll")] public static extern int ctlClose(IntPtr api);
  [DllImport("ControlLib.dll")] public static extern int ctlEnumerateDevices(IntPtr api, ref uint n, [Out] IntPtr[] dev);
  [DllImport("ControlLib.dll")] public static extern int ctlEnumerateDisplayOutputs(IntPtr dev, ref uint n, [Out] IntPtr[] outs);
  [DllImport("ControlLib.dll")] public static extern int ctlAUXAccess(IntPtr output, ref AuxArgs a);
  // Native AUX read; returns hex bytes or null on error
  public static string Read(IntPtr output, uint addr, uint len) {
    AuxArgs a = new AuxArgs(); a.Size = (uint)Marshal.SizeOf(typeof(AuxArgs)); a.OpType = 1; a.Flags = 1;
    a.Address = addr; a.DataSize = len; a.Data = new byte[132];
    if (ctlAUXAccess(output, ref a) != 0) return null;
    return BitConverter.ToString(a.Data, 0, (int)len).Replace('-', ' ').ToLower();
  }
}
'@
$init = New-Object Igcl+InitArgs
$init.Size = [Runtime.InteropServices.Marshal]::SizeOf([type][Igcl+InitArgs]); $init.AppVersion = (1 -shl 16) -bor 1
$api = [IntPtr]::Zero
$r = [Igcl]::ctlInit([ref]$init, [ref]$api); if ($r -ne 0) { 'ctlInit failed 0x{0:X8}' -f $r; exit 1 }
'IGCL runtime version 0x{0:X}' -f $init.SupportedVersion
$n = [uint32]0; [Igcl]::ctlEnumerateDevices($api, [ref]$n, $null) | Out-Null
$devs = New-Object IntPtr[] $n; [Igcl]::ctlEnumerateDevices($api, [ref]$n, $devs) | Out-Null
foreach ($d in $devs) {
    $m = [uint32]0; [Igcl]::ctlEnumerateDisplayOutputs($d, [ref]$m, $null) | Out-Null
    $outs = New-Object IntPtr[] $m; [Igcl]::ctlEnumerateDisplayOutputs($d, [ref]$m, $outs) | Out-Null
    for ($i = 0; $i -lt $m; $i++) {
        if ($null -eq [Igcl]::Read($outs[$i], 0, 1)) { continue }  # output without an AUX channel or not connected
        "== output $i"
        foreach ($rg in $Ranges) {
            $parts = $rg.Split(':'); $addr = [uint32][int]$parts[0]; $len = [uint32]$parts[1]
            $bytes = [Igcl]::Read($outs[$i], $addr, $len)
            if ($null -eq $bytes) {  # some sinks/drivers reject multi-byte reads: fall back to single bytes
                $list = @()
                for ($k = 0; $k -lt $len; $k++) { $one = [Igcl]::Read($outs[$i], $addr + $k, 1); if ($null -eq $one) { $list += '--' } else { $list += $one } }
                $bytes = $list -join ' '
            }
            '{0:x5}: {1}' -f $addr, $bytes
        }
    }
}
[Igcl]::ctlClose($api) | Out-Null
