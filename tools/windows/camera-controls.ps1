# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# List, read-only, the controls Windows exposes for each camera (WinRT MediaCapture).
#   - standard controls (VideoDeviceController: Exposure, Focus, Brightness ...): supported or not, range, default
#   - advanced controls (ExposureControl, FocusControl, IsoSpeedControl, FlashControl, TorchControl, InfraredTorchControl ...)
#   - the result of a GET query for ids 0..63 of KSPROPERTYSETID_ExtendedCameraControl (header Capability/Flags and payload hex)
# Writes no value. Opening a source group powers that camera sensor briefly and the privacy LED may light. No streaming.
# Usage: powershell -ExecutionPolicy Bypass -File tools\windows\camera-controls.ps1 [-Group <name regex>] [-NoExtended] [-MaxId 63]
param([string]$Group = '.', [switch]$NoExtended, [int]$MaxId = 63)
$ErrorActionPreference = 'Continue'
Add-Type -AssemblyName System.Runtime.WindowsRuntime
$null = [Windows.Media.Capture.MediaCapture, Windows.Media.Capture, ContentType = WindowsRuntime]
$null = [Windows.Media.Capture.Frames.MediaFrameSourceGroup, Windows.Media.Capture, ContentType = WindowsRuntime]
$null = [Windows.Media.Devices.VideoDeviceController, Windows.Media.Devices, ContentType = WindowsRuntime]
$ext = [System.WindowsRuntimeSystemExtensions].GetMethods()
$asTaskOp = $ext | Where-Object { $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1' } | Select-Object -First 1
$asTaskAct = $ext | Where-Object { $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncAction' } | Select-Object -First 1
function Wait-Op($op, [Type]$t) { $task = $asTaskOp.MakeGenericMethod($t).Invoke($null, @($op)); [void]$task.Wait(15000); $task.Result }
function Wait-Act($op) {
    $task = $asTaskAct.Invoke($null, @($op))
    if (-not $task.Wait(20000)) { throw "timed out ($($task.Status))" }
    if ($task.Exception) { throw $task.Exception.InnerException }
}

# ksmedia.h KSPROPERTY_CAMERACONTROL_EXTENDED_PROPERTY
$extNames = @{ 0 = 'PHOTOMODE'; 1 = 'PHOTOFRAMERATE'; 2 = 'PHOTOMAXFRAMERATE'; 3 = 'PHOTOTRIGGERTIME'; 4 = 'WARMSTART'
    5 = 'MAXVIDFPS_PHOTORES'; 6 = 'PHOTOTHUMBNAIL'; 7 = 'SCENEMODE'; 8 = 'TORCHMODE'; 9 = 'FLASHMODE'; 10 = 'OPTIMIZATIONHINT'
    11 = 'WHITEBALANCEMODE'; 12 = 'EXPOSUREMODE'; 13 = 'FOCUSMODE'; 14 = 'ISO'; 15 = 'FIELDOFVIEW'; 16 = 'EVCOMPENSATION'
    17 = 'CAMERAANGLEOFFSET'; 18 = 'METADATA'; 19 = 'FOCUSPRIORITY'; 20 = 'FOCUSSTATE'; 21 = 'ROI_CONFIGCAPS'; 22 = 'ROI_ISPCONTROL'
    23 = 'PHOTOCONFIRMATION'; 24 = 'ZOOM'; 25 = 'MCC'; 26 = 'ISO_ADVANCED'; 27 = 'VIDEOSTABILIZATION'; 28 = 'VFR'
    29 = 'FACEDETECTION'; 30 = 'VIDEOHDR'; 31 = 'HISTOGRAM'; 32 = 'OIS'; 33 = 'ADVANCEDPHOTO'; 34 = 'PROFILE'
    35 = 'FACEAUTH_MODE'; 36 = 'SECURE_MODE'; 37 = 'VIDEOTEMPORALDENOISING'; 38 = 'IRTORCHMODE'; 39 = 'RELATIVEPANELOPTIMIZATION'
    40 = 'EYEGAZECORRECTION'; 41 = 'BACKGROUNDSEGMENTATION'; 42 = 'DIGITALWINDOW_CONFIGCAPS'; 43 = 'DIGITALWINDOW'
    44 = 'END'; 45 = 'FRAMERATE_THROTTLE'; 46 = 'FIELDOFVIEW2_CONFIGCAPS'; 47 = 'FIELDOFVIEW2' }
$extSet = [guid]'1CB79112-C0D2-4213-9CA6-CD4FDB927972'

function Show-Std($vdc) {
    foreach ($n in 'Brightness', 'Contrast', 'Hue', 'WhiteBalance', 'BacklightCompensation', 'Pan', 'Tilt', 'Zoom', 'Roll', 'Exposure', 'Focus') {
        $c = $vdc.$n
        if ($null -eq $c) { continue }
        $cap = $c.Capabilities
        if (-not $cap.Supported) { "    std $n : -"; continue }
        $auto = $null; [void]$c.TryGetAuto([ref]$auto)
        $val = $null; [void]$c.TryGetValue([ref]$val)
        "    std {0} : min={1} max={2} step={3} default={4} auto-cap={5} auto={6} value={7}" -f $n, $cap.Min, $cap.Max, $cap.Step, $cap.Default, $cap.AutoModeSupported, $auto, $val
    }
}

function Show-Adv($vdc) {
    $e = $vdc.ExposureControl
    if ($e.Supported) { "    adv Exposure : min={0} max={1} step={2} auto={3} value={4}" -f $e.Min, $e.Max, $e.Step, $e.Auto, $e.Value } else { '    adv Exposure : -' }
    $f = $vdc.FocusControl
    if ($f.Supported) { "    adv Focus : min={0} max={1} step={2} value={3} modes={4} presets={5}" -f $f.Min, $f.Max, $f.Step, $f.Value, ($f.SupportedFocusModes -join ','), ($f.SupportedPresets -join ',') } else { '    adv Focus : -' }
    $i = $vdc.IsoSpeedControl
    if ($i.Supported) { "    adv IsoSpeed : min={0} max={1} step={2} auto={3} value={4}" -f $i.Min, $i.Max, $i.Step, $i.Auto, $i.Value } else { '    adv IsoSpeed : -' }
    $w = $vdc.WhiteBalanceControl
    if ($w.Supported) { "    adv WhiteBalance : min={0} max={1} step={2} preset={3} value={4}" -f $w.Min, $w.Max, $w.Step, $w.Preset, $w.Value } else { '    adv WhiteBalance : -' }
    $c = $vdc.ExposureCompensationControl
    if ($c.Supported) { "    adv ExposureCompensation : min={0} max={1} step={2} value={3}" -f $c.Min, $c.Max, $c.Step, $c.Value } else { '    adv ExposureCompensation : -' }
    $fl = $vdc.FlashControl
    "    adv Flash : supported={0} powerSupported={1} redEye={2}" -f $fl.Supported, $fl.PowerSupported, $fl.RedEyeReductionSupported
    $t = $vdc.TorchControl
    "    adv Torch : supported={0} powerSupported={1}" -f $t.Supported, $t.PowerSupported
    try {
        $ir = $vdc.InfraredTorchControl
        if ($ir.IsSupported) {
            "    adv InfraredTorch : modes={0} current={1} power min={2} max={3} step={4} default={5} value={6}" -f ($ir.SupportedModes -join ','), $ir.CurrentMode, $ir.MinPower, $ir.MaxPower, $ir.PowerStep, $ir.DefaultPower, $ir.Power
        } else { '    adv InfraredTorch : -' }
    } catch { "    adv InfraredTorch : n/a ($($_.Exception.Message))" }
    try { $p = $vdc.PanelBasedOptimizationControl; "    adv PanelBasedOptimization : supported={0}" -f $p.IsSupported } catch {}
    try { $h = $vdc.HdrVideoControl; "    adv HdrVideo : supported={0} modes={1}" -f $h.Supported, ($h.SupportedModes -join ',') } catch {}
    try { $v = $vdc.VideoTemporalDenoisingControl; "    adv TemporalDenoising : supported={0} modes={1}" -f $v.Supported, ($v.SupportedModes -join ',') } catch {}
    try { $z = $vdc.ZoomControl; if ($z.Supported) { "    adv Zoom : min={0} max={1} step={2}" -f $z.Min, $z.Max, $z.Step } } catch {}
    "    camera profile/primary use: {0}" -f $vdc.PrimaryUse
}

function Show-Ext($vdc) {
    for ($id = 0; $id -le $MaxId; $id++) {
        $k = New-Object byte[] 24
        [Array]::Copy($extSet.ToByteArray(), 0, $k, 0, 16)
        [Array]::Copy([BitConverter]::GetBytes([uint32]$id), 0, $k, 16, 4)
        [Array]::Copy([BitConverter]::GetBytes([uint32]1), 0, $k, 20, 4)   # KSPROPERTY_TYPE_GET
        try { $r = $vdc.GetDevicePropertyByExtendedId($k, [Nullable[uint32]]4096) } catch { continue }
        if ($r.Status -ne 'Success') { continue }
        $b = [byte[]]$r.Value
        $name = if ($extNames.ContainsKey($id)) { $extNames[$id] } else { "id$id" }
        if ($b.Length -ge 32) {
            $ver = [BitConverter]::ToUInt32($b, 0); $pin = [BitConverter]::ToUInt32($b, 4); $size = [BitConverter]::ToUInt32($b, 8)
            $res = [BitConverter]::ToUInt32($b, 12); $flags = [BitConverter]::ToUInt64($b, 16); $cap = [BitConverter]::ToUInt64($b, 24)
            $tail = if ($b.Length -gt 32) { ([BitConverter]::ToString($b, 32)).Replace('-', ' ') } else { '' }
            "    ext {0,2} {1} : ver={2} pin={3:x} size={4} result={5:x} flags=0x{6:x} capability=0x{7:x} {8}" -f $id, $name, $ver, $pin, $size, $res, $flags, $cap, $tail
        } else {
            "    ext {0,2} {1} : {2}" -f $id, $name, ([BitConverter]::ToString($b)).Replace('-', ' ')
        }
    }
}

$groups = Wait-Op ([Windows.Media.Capture.Frames.MediaFrameSourceGroup]::FindAllAsync()) ([System.Collections.Generic.IReadOnlyList[Windows.Media.Capture.Frames.MediaFrameSourceGroup]])
foreach ($g in $groups) {
    if ($g.DisplayName -notmatch $Group) { continue }
    "group `"$($g.DisplayName)`""
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
            "  source kind=$($fs.Info.SourceKind) stream=$($fs.Info.MediaStreamType) device=`"$($fs.Info.DeviceInformation.Name)`""
            $vdc = $fs.Controller.VideoDeviceController
            if ($null -eq $vdc) { '    (no VideoDeviceController)'; continue }
            Show-Std $vdc
            Show-Adv $vdc
            if (-not $NoExtended) { Show-Ext $vdc }
        }
    } catch { "  open failed: $($_.Exception.Message)" } finally { $mc.Dispose() }
}
