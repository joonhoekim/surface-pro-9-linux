# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# Read the Surface SMF policy blob (UEFI variable SMF_DATA_BLOB, vendor {A3D05FA9-DB5E-490C-956E-A57384A4B70B}) read-only
# and decode it with smf_blob.py. Needs admin (SeSystemEnvironmentPrivilege): run through tools\windows\run-elevated.ps1.
# Output: private/windows/perf/smf-data-blob.bin (raw) and smf-data-blob.txt (decompressed CSV node graph).
$ErrorActionPreference = 'Stop'
$repo = Resolve-Path (Join-Path $PSScriptRoot '..\..\..')
$dir = Join-Path $repo 'private\windows\perf'; New-Item -ItemType Directory -Force $dir | Out-Null
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class SmfVar {
  [DllImport("kernel32.dll", SetLastError=true, CharSet=CharSet.Unicode)]
  public static extern uint GetFirmwareEnvironmentVariableExW(string n, string g, byte[] b, uint s, out uint attr);
  [DllImport("advapi32.dll", SetLastError=true)] static extern bool OpenProcessToken(IntPtr h, uint a, out IntPtr t);
  [DllImport("advapi32.dll", SetLastError=true, CharSet=CharSet.Unicode)] static extern bool LookupPrivilegeValue(string s, string n, out long l);
  [StructLayout(LayoutKind.Sequential, Pack=4)] struct TP { public uint c; public long l; public uint a; }
  [DllImport("advapi32.dll", SetLastError=true)] static extern bool AdjustTokenPrivileges(IntPtr t, bool d, ref TP n, uint l, IntPtr p, IntPtr r);
  [DllImport("kernel32.dll")] static extern IntPtr GetCurrentProcess();
  public static void Enable() { IntPtr t; OpenProcessToken(GetCurrentProcess(), 0x28, out t); long l;
    LookupPrivilegeValue(null, "SeSystemEnvironmentPrivilege", out l); TP p = new TP(); p.c = 1; p.l = l; p.a = 2;
    AdjustTokenPrivileges(t, false, ref p, 0, IntPtr.Zero, IntPtr.Zero); }
}
"@
[SmfVar]::Enable()
$buf = New-Object byte[] 1048576; $attr = 0
$n = [SmfVar]::GetFirmwareEnvironmentVariableExW('SMF_DATA_BLOB', '{A3D05FA9-DB5E-490C-956E-A57384A4B70B}', $buf, $buf.Length, [ref]$attr)
if ($n -eq 0) { throw "GetFirmwareEnvironmentVariableEx failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())" }
$bin = Join-Path $dir 'smf-data-blob.bin'
[IO.File]::WriteAllBytes($bin, $buf[0..($n - 1)])
"SMF_DATA_BLOB: $n bytes, attributes 0x$('{0:x}' -f $attr)"
& python -I (Join-Path $PSScriptRoot 'smf_blob.py') $bin (Join-Path $dir 'smf-data-blob.txt')
