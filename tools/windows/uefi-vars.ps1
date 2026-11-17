# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# List UEFI variables (name, vendor GUID, attributes, size) through NtEnumerateSystemEnvironmentValuesEx.
# Read-only. Needs admin (SeSystemEnvironmentPrivilege): run through run-elevated.ps1.
#
#   run-elevated.ps1 tools\windows\uefi-vars.ps1 [-Name REGEX] [-Values] [-SaveTo FILE]
#
# -Name filters by variable name or GUID. -Values adds a hex dump of each listed variable's data (first -MaxBytes bytes, default 256).
# Values can hold device-unique data (serials, keys, MACs): with -Values write to private/ via -SaveTo, not to hw/.
param([string]$Name = '.', [switch]$Values, [int]$MaxBytes = 256, [string]$SaveTo)

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class UefiEnum {
    [DllImport("ntdll.dll")] static extern int NtEnumerateSystemEnvironmentValuesEx(uint InfoClass, IntPtr Buffer, ref uint Length);
    [DllImport("ntdll.dll")] static extern int RtlAdjustPrivilege(int Privilege, bool Enable, bool Client, out bool Was);
    public static void Enable() { bool was; RtlAdjustPrivilege(22, true, false, out was); }  // SE_SYSTEM_ENVIRONMENT_PRIVILEGE
    // InfoClass 2 = SystemEnvironmentValueInformation: VARIABLE_NAME_AND_VALUE entries
    public static byte[] Raw(out int status) {
        uint len = 0;
        status = NtEnumerateSystemEnvironmentValuesEx(2, IntPtr.Zero, ref len);
        if (len == 0) len = 1 << 20;
        IntPtr buf = Marshal.AllocHGlobal((int)len);
        try {
            status = NtEnumerateSystemEnvironmentValuesEx(2, buf, ref len);
            byte[] b = new byte[len];
            Marshal.Copy(buf, b, 0, (int)len);
            return b;
        } finally { Marshal.FreeHGlobal(buf); }
    }
}
'@

[UefiEnum]::Enable()
$st = 0
$raw = [UefiEnum]::Raw([ref]$st)
if ($st -ne 0) { "NtEnumerateSystemEnvironmentValuesEx failed: 0x{0:X8} (needs admin)" -f $st; exit 1 }

# VARIABLE_NAME_AND_VALUE: ULONG NextEntryOffset; ULONG ValueOffset; ULONG ValueLength; ULONG Attributes;
#                          GUID VendorGuid; WCHAR Name[]; ... value at ValueOffset
$rows = @()
$off = 0
while ($off -lt $raw.Length) {
    $next = [BitConverter]::ToUInt32($raw, $off)
    $voff = [BitConverter]::ToUInt32($raw, $off + 4)
    $vlen = [BitConverter]::ToUInt32($raw, $off + 8)
    $attr = [BitConverter]::ToUInt32($raw, $off + 12)
    $guid = [Guid]::new([byte[]]$raw[($off + 16)..($off + 31)])
    $nameEnd = $off + 32
    while ($nameEnd + 1 -lt $raw.Length -and ($raw[$nameEnd] -ne 0 -or $raw[$nameEnd + 1] -ne 0)) { $nameEnd += 2 }
    $nm = [Text.Encoding]::Unicode.GetString($raw, $off + 32, $nameEnd - $off - 32)
    $val = if ($vlen -gt 0) { [byte[]]$raw[($off + $voff)..($off + $voff + $vlen - 1)] } else { @() }
    $rows += [pscustomobject]@{ Guid = $guid; Name = $nm; Attr = $attr; Size = $vlen; Value = $val }
    if ($next -eq 0) { break }
    $off += $next
}

$sel = $rows | Where-Object { $_.Name -match $Name -or "$($_.Guid)" -match $Name } | Sort-Object Guid, Name
$lines = foreach ($r in $sel) {
    # Attributes: 1 NV, 2 BS, 4 RT, 0x20 time-based auth
    "{0} {1,-40} attr=0x{2:x2} size={3}" -f $r.Guid, $r.Name, $r.Attr, $r.Size
    if ($Values -and $r.Size -gt 0) {
        $v = $r.Value[0..([Math]::Min($r.Size, $MaxBytes) - 1)]
        for ($i = 0; $i -lt $v.Count; $i += 16) {
            $chunk = $v[$i..([Math]::Min($i + 15, $v.Count - 1))]
            "    {0:x4}: {1}" -f $i, (($chunk | ForEach-Object { '{0:x2}' -f $_ }) -join ' ')
        }
    }
}
"# {0} variables, {1} listed" -f $rows.Count, @($sel).Count
if ($SaveTo) { $lines | Out-File -Encoding utf8 $SaveTo; "written to $SaveTo" } else { $lines }
