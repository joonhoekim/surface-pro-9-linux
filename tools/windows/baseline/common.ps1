# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# Shared paths and helpers, dot-sourced by the kit's scripts.
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..')).Path
$data = Join-Path $env:LOCALAPPDATA 'sp9-baseline'
$Kit = @{
    Repo    = $repo
    Dir     = $PSScriptRoot
    Out     = Join-Path $repo 'out\baseline'
    Probe   = Join-Path $repo 'out\baseline\sp9probe.exe'
    Data    = $data
    Python  = Join-Path $data 'venv\Scripts\python.exe'
    Wprp    = Join-Path $data 'BluetoothStack.wprp'
    Private = Join-Path $repo 'private\windows\baseline'
    Public  = Join-Path $repo 'hw\windows\baseline'
    Elev    = Join-Path $repo 'tools\windows\run-elevated.ps1'
}

function Get-Ffmpeg {
    $c = Get-Command ffmpeg -ErrorAction SilentlyContinue
    if ($c) { return $c.Source }
    $p = Get-ChildItem (Join-Path $env:LOCALAPPDATA 'Microsoft\WinGet\Packages') -Filter ffmpeg.exe -Recurse -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -match 'Gyan\.FFmpeg' } | Select-Object -First 1
    if ($p) { return $p.FullName }
    return $null
}

# Remove machine-unique strings before anything goes to hw/: Bluetooth/MAC addresses, GUID-like instance tails, host name, serials.
function Protect-Text([string]$s) {
    if (-not $s) { return $s }
    $s = $s -replace '(?i)\b([0-9a-f]{2}[:-]){5}[0-9a-f]{2}\b', 'XX:XX:XX:XX:XX:XX'
    $s = $s -replace '(?i)(?<=[_&#\\])[0-9a-f]{12}(?=[_&#\\]|$)', '<addr>'
    $s = $s -replace '(?i)\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b', '<guid>'
    $s = $s -replace '(?i)\b\d&[0-9a-f]{6,8}&\d&[0-9a-f]{4}\b', '<inst>'
    if ($env:COMPUTERNAME) { $s = $s -replace [regex]::Escape($env:COMPUTERNAME), '<host>' }
    $s = $s -replace '(?i)((serial|bssid|ssid)\s*[:=]\s*)\S.*', '$1<masked>'
    return $s
}

function Get-UnixTime { [double]::Parse(([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() / 1000.0).ToString('F3'), [Globalization.CultureInfo]::InvariantCulture) }

function Get-WindowsBuild {
    $cv = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion'
    "Windows {0}.{1}" -f $cv.CurrentBuild, $cv.UBR
}
