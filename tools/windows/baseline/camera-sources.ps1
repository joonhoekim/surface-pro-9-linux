# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# List Media Foundation frame source groups (color, infrared, depth) and the formats each source offers, through WinRT.
# This sees the IR camera, which DirectShow/ffmpeg does not. MediaCapture is opened shared read-only (no format change);
# opening a source group briefly turns on that camera's privacy LED.
# Usage: powershell -ExecutionPolicy Bypass -File camera-sources.ps1 [-NoOpen]
param([switch]$NoOpen)
Add-Type -AssemblyName System.Runtime.WindowsRuntime
$null = [Windows.Media.Capture.MediaCapture, Windows.Media.Capture, ContentType = WindowsRuntime]
$null = [Windows.Media.Capture.Frames.MediaFrameSourceGroup, Windows.Media.Capture, ContentType = WindowsRuntime]
$ext = [System.WindowsRuntimeSystemExtensions].GetMethods()
$asTaskOp = $ext | Where-Object { $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1' } | Select-Object -First 1
$asTaskAct = $ext | Where-Object { $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncAction' } | Select-Object -First 1
function Wait-Op($op, [Type]$t) { $task = $asTaskOp.MakeGenericMethod($t).Invoke($null, @($op)); [void]$task.Wait(15000); $task.Result }
function Wait-Act($op) {
    $task = $asTaskAct.Invoke($null, @($op))
    if (-not $task.Wait(20000)) { throw "timed out ($($task.Status))" }
    if ($task.Exception) { throw $task.Exception.InnerException }
}

$groups = Wait-Op ([Windows.Media.Capture.Frames.MediaFrameSourceGroup]::FindAllAsync()) ([System.Collections.Generic.IReadOnlyList[Windows.Media.Capture.Frames.MediaFrameSourceGroup]])
foreach ($g in $groups) {
    "group `"$($g.DisplayName)`""
    foreach ($si in $g.SourceInfos) { "  source kind=$($si.SourceKind) stream=$($si.MediaStreamType) device=`"$($si.DeviceInformation.Name)`"" }
    if ($NoOpen) { continue }
    $mc = New-Object Windows.Media.Capture.MediaCapture
    $s = New-Object Windows.Media.Capture.MediaCaptureInitializationSettings
    $s.SourceGroup = $g
    $s.SharingMode = [Windows.Media.Capture.MediaCaptureSharingMode]::SharedReadOnly
    $s.MemoryPreference = [Windows.Media.Capture.MediaCaptureMemoryPreference]::Cpu
    $s.StreamingCaptureMode = [Windows.Media.Capture.StreamingCaptureMode]::Video
    try {
        Wait-Act ($mc.InitializeAsync($s))
        foreach ($kv in $mc.FrameSources) {
            $fs = $kv.Value
            $cur = $fs.CurrentFormat
            "  open kind=$($fs.Info.SourceKind) stream=$($fs.Info.MediaStreamType) current=$($cur.Subtype) $($cur.VideoFormat.Width)x$($cur.VideoFormat.Height)@$([math]::Round($cur.FrameRate.Numerator / [math]::Max(1,$cur.FrameRate.Denominator),2))"
            # one line per resolution: subtypes and frame rates
            $fs.SupportedFormats | Group-Object { $_.VideoFormat.Width * 100000 + $_.VideoFormat.Height } | Sort-Object { [int]$_.Name } -Descending | ForEach-Object {
                $w = $_.Group[0].VideoFormat
                $sub = ($_.Group | Group-Object Subtype | ForEach-Object {
                        $_.Name + "@" + (($_.Group | ForEach-Object { [math]::Round($_.FrameRate.Numerator / [math]::Max(1, $_.FrameRate.Denominator), 2) } | Sort-Object -Unique) -join ",")
                    }) -join " "
                "    {0}x{1}: {2}" -f $w.Width, $w.Height, $sub
            }
        }
    } catch { "  open failed: $($_.Exception.Message)" } finally { $mc.Dispose() }
}
