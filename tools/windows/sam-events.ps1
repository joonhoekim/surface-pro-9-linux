# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# SAM(SSH) event registrations and Surface driver warnings from the Windows event logs.
# No admin needed. Output goes to stdout.
#
# The Surface Serial Hub driver logs every SAM event it enables
# ("register events succeeded, TargetID, TargetCategory, TargetCategoryInstance, CommandID").
# That list is the ground truth of which SAM functions Windows actually uses on this device.

$ErrorActionPreference = 'SilentlyContinue'

"## SAM event registrations (Microsoft-Surface-SurfaceSerialHubDriver/Operational)"
Get-WinEvent -LogName 'Microsoft-Surface-SurfaceSerialHubDriver/Operational' |
    Where-Object { $_.Message -match 'register events succeeded' -and $_.Message -notmatch 'unregister' } |
    ForEach-Object {
        if ($_.Message -match 'TargetID = (\d+), TargetCategory = (\w+), TargetCategoryInstance = (\d+), CommandID = (\d+)') {
            [pscustomobject]@{ Tid = [int]$Matches[1]; Tc = $Matches[2]; Iid = [int]$Matches[3]; Cid = [int]$Matches[4] }
        }
    } |
    Group-Object Tid, Tc, Iid, Cid |
    ForEach-Object { "{0,-24} x{1}" -f $_.Name, $_.Count }

"`n## Other messages (deduplicated)"
$logs = 'Microsoft-Surface-SurfaceSerialHubDriver/Operational',
        'Microsoft-Surface-SurfaceHotPlug/Operational',
        'Microsoft-Surface-SurfaceSmfClient/Operational',
        'Microsoft-Surface-SurfaceSmfCoreDriver/Operational',
        'Microsoft-Surface-SurfaceHidMiniDriver/Operational',
        'Microsoft-Surface-SurfaceBattery/Operational',
        'Microsoft-Surface-SurfaceAcpiPlatformExtensionDriver/Operational'
foreach ($l in $logs) {
    Get-WinEvent -LogName $l |
        Where-Object { $_.Message -notmatch 'register events succeeded' } |
        ForEach-Object { "[{0}] id={1} {2}" -f ($l -replace '^Microsoft-Surface-|/Operational$', ''), $_.Id, ($_.Message -replace "`r?`n", ' ') } |
        Group-Object |
        ForEach-Object { "x{0,-3} {1}" -f $_.Count, $_.Name }
}

"`n## CPU SMF client power-limit updates (Microsoft-Surface-SurfaceIhvCpuSmfClient/Operational, last 500)"
Get-WinEvent -LogName 'Microsoft-Surface-SurfaceIhvCpuSmfClient/Operational' -MaxEvents 500 |
    ForEach-Object {
        if ($_.Message -match 'Channel Index=(\d+), ChannelInstance=(\d+), Value=(\d+)') {
            [pscustomobject]@{ Ch = [int]$Matches[1]; Inst = [int]$Matches[2]; Value = [int]$Matches[3] }
        }
    } |
    Group-Object Ch, Inst |
    ForEach-Object {
        $v = $_.Group.Value | Measure-Object -Minimum -Maximum
        "channel {0,-8} n={1,-4} min={2} max={3}" -f $_.Name, $_.Count, $v.Minimum, $v.Maximum
    }
