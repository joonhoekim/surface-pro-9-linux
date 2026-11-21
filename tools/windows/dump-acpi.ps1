# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# Dump every ACPI table through GetSystemFirmwareTable (the registry only holds DSDT/SSDTs) and decompile with iasl.
# MSDM (Windows product key) is skipped. Output: hw/windows/acpi/<sig>[N].dat and dsl/<sig>[N].dsl
# iasl: built from github.com/acpica/acpica in MSYS2 (winget MSYS2.MSYS2; pacman -S gcc make bison flex; make iasl).
param([string]$Out, [string]$Iasl = 'C:\msys64\opt\src\acpica\generate\unix\bin\iasl')
$root = Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path))
if (-not $Out) { $Out = Join-Path $root 'hw\windows\acpi' }
New-Item -ItemType Directory -Force $Out, (Join-Path $Out 'dsl') | Out-Null

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class Fw {
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern uint EnumSystemFirmwareTables(uint provider, byte[] buf, uint size);
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern uint GetSystemFirmwareTable(uint provider, uint id, byte[] buf, uint size);
}
'@
$ACPI = [uint32]0x41435049
$n = [Fw]::EnumSystemFirmwareTables($ACPI, $null, 0)
$ids = New-Object byte[] $n
[void][Fw]::EnumSystemFirmwareTables($ACPI, $ids, $n)

Get-ChildItem $Out -File -Filter *.dat | Remove-Item
$seen = @{}
for ($i = 0; $i -lt $n; $i += 4) {
    $id = [BitConverter]::ToUInt32($ids, $i)
    $sig = [Text.Encoding]::ASCII.GetString($ids, $i, 4)
    if ($sig -in 'MSDM', 'SSDT') { continue }
    $len = [Fw]::GetSystemFirmwareTable($ACPI, $id, $null, 0)
    $buf = New-Object byte[] $len
    [void][Fw]::GetSystemFirmwareTable($ACPI, $id, $buf, $len)
    $k = $seen[$sig]; $seen[$sig] = 1 + [int]$k
    $name = if ($k) { "$($sig.ToLower())$k" } else { $sig.ToLower() }
    [IO.File]::WriteAllBytes((Join-Path $Out "$name.dat"), $buf)
}
# The enumeration lists SSDT once per table but every call returns the first one, and DSDT is not listed:
# take DSDT and the SSDTs from the registry, named by OEM table ID.
Get-ChildItem HKLM:\HARDWARE\ACPI | Where-Object { $_.PSChildName -notin 'MSDM' } | ForEach-Object {
    Get-ChildItem $_.PSPath -Recurse | ForEach-Object {
        foreach ($v in $_.GetValueNames()) {
            $data = $_.GetValue($v)
            if ($data -is [byte[]] -and $data.Length -ge 36) {
                $s = [Text.Encoding]::ASCII.GetString($data, 0, 4)
                $tid = [Text.Encoding]::ASCII.GetString($data, 16, 8).Trim([char]0, ' ')
                if ($s -eq 'SSDT') { [IO.File]::WriteAllBytes((Join-Path $Out "ssdt-$($tid.ToLower()).dat"), $data) }
                if ($s -eq 'DSDT') { [IO.File]::WriteAllBytes((Join-Path $Out 'dsdt.dat'), $data) }
            }
        }
    }
}

$env:PATH = "C:\msys64\usr\bin;$env:PATH"
$exe = Join-Path $env:TEMP 'iasl.exe'
Copy-Item $Iasl $exe -Force
Push-Location (Join-Path $Out 'dsl')
Get-ChildItem -File | Remove-Item
foreach ($f in Get-ChildItem $Out -File -Filter *.dat) {
    Copy-Item $f.FullName .
    & $exe -d $f.Name *> $null
    Remove-Item $f.Name
}
Pop-Location
Get-ChildItem $Out -File | Select Name, Length | Format-Table -AutoSize
