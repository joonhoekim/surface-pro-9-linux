# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# Type Cover (KIP hub MSHW0153, SHP devices 1/2/3/5) probe. Read-only toward the cover: no SET_FEATURE, SET_OUTPUT or
# writes, ever. Builds typecover-probe.cs with the .NET Framework csc into out/typecover/.
#
# Automatic part (default; relaunches itself elevated through run-elevated.ps1). Model-level output to hw/windows/typecover/:
#   layout.txt          every KIP collection: caps, link collections, bit position of every field
#   features.txt        GET of every feature report (pen bonding FFF4 skipped) + GET_INPUT of the Fn-state and backlight reports,
#                       decoded as link/PAGE:USAGE=value
#   devices.txt         KIP PnP devices: class, service, filters, INF (SHP instance suffix masked)
#   ptp-registry.txt    Precision Touchpad settings (HKLM defaults written by SurfaceService, HKCU user settings)
#
# Interactive part (-Interactive; visible elevated console + a window with step prompts, Enter between steps):
#   Fn alone, top row with Fn lock off/on/held, backlight key through all levels, backlight idle timeout, ambient light,
#   two-finger scroll held at the edge / middle / side, three- and four-finger swipes, taps and clicks, Caps Lock,
#   special keys. Logged per step to private/windows/typecover/<run>/: Raw Input (keyboard: key names only, character
#   keys masked; HID collections: decoded reports), ReadFile on shareable KIP collections, WH_MOUSE_LL wheel/button
#   events, list scroll position, feature/GET_INPUT snapshots at every step boundary, polls of backlight Feature 214 and
#   Fn state, HIDCLASS/Surface ETW (which process sent which SET/GET to which collection). Then input-trace.py and
#   typecover-summary.py write summary.txt and summary.md.
#   The display is kept on (ES_DISPLAY_REQUIRED) for the whole session; nothing enters standby.
#
#   powershell -ExecutionPolicy Bypass -File tools\windows\typecover-probe.ps1                 # automatic part
#   powershell -ExecutionPolicy Bypass -File tools\windows\typecover-probe.ps1 -Interactive    # guided part (user)
#   -Auto N      session without prompts, every step N seconds (self test of the logging pieces)
#   -NoEtw       session without the ETW trace
#   -Summarize <run dir>   decode again
param(
    [switch]$Interactive,
    [int]$Auto = 0,
    [switch]$NoEtw,
    [string]$OutDir,
    [string]$Summarize
)
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$py = Join-Path $env:LOCALAPPDATA 'sp9-baseline\venv\Scripts\python.exe'
if (-not (Test-Path $py)) { $py = 'python' }

function Decode([string]$run) {
    $ErrorActionPreference = 'Continue'
    if (Test-Path (Join-Path $run 'input.etl')) { & $py -I -X utf8 (Join-Path $PSScriptRoot 'input-trace.py') $run | Out-Null }
    & $py -I -X utf8 (Join-Path $PSScriptRoot 'typecover-summary.py') $run
}
if ($Summarize) { Decode (Resolve-Path $Summarize).Path; exit 0 }

$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole(
    [Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    $pass = @()
    foreach ($k in $PSBoundParameters.Keys) {
        $v = $PSBoundParameters[$k]
        if ($v -is [switch]) { if ($v) { $pass += "-$k" } } else { $pass += "-$k"; $pass += "$v" }
    }
    if ($Interactive -and $Auto -eq 0) {
        $argStr = ($pass | ForEach-Object { if ($_ -match '^-\w+$') { $_ } else { "'" + ($_ -replace "'", "''") + "'" } }) -join ' '
        $cmd = "Set-Location '$repo'; & '$PSCommandPath' $argStr; Read-Host 'Press Enter to close this window'"
        Start-Process powershell -Verb RunAs -Wait -ArgumentList '-NoProfile', '-ExecutionPolicy', 'Bypass', '-Command', $cmd
        exit 0
    }
    Push-Location $repo
    & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot 'run-elevated.ps1') $PSCommandPath @pass
    $rc = $LASTEXITCODE; Pop-Location; exit $rc
}

# Build (again when the source is newer).
$src = Join-Path $PSScriptRoot 'typecover-probe.cs'
$exe = Join-Path $repo 'out\typecover\typecover-probe.exe'
if (-not (Test-Path $exe) -or (Get-Item $src).LastWriteTime -gt (Get-Item $exe).LastWriteTime) {
    New-Item -ItemType Directory -Force (Split-Path $exe) | Out-Null
    $csc = Join-Path $env:WINDIR 'Microsoft.NET\Framework64\v4.0.30319\csc.exe'
    $o = & $csc /nologo /optimize+ /target:exe /platform:x64 /codepage:65001 /r:System.Windows.Forms.dll /r:System.Drawing.dll "/out:$exe" $src
    if ($LASTEXITCODE) { $o; throw "csc failed ($LASTEXITCODE)" }
}

$maskShp = { param($s) $s -replace '(?i)(SHP_DEVICE_[0-9a-f]{8}_)[0-9a-f]{8}', '$1<cover>' }

if (-not $Interactive -and $Auto -eq 0) {
    $hw = if ($OutDir) { $OutDir } else { Join-Path $repo 'hw\windows\typecover' }
    New-Item -ItemType Directory -Force $hw | Out-Null
    $t0 = Get-Date
    & $exe layout (Join-Path $hw 'layout.txt')
    & $exe features (Join-Path $hw 'features.txt')
    "features: {0:N0} s (write-only reports time out after ~8 s each)" -f ((Get-Date) - $t0).TotalSeconds

    # PnP view of the cover: who binds each collection.
    $lines = foreach ($d in Get-PnpDevice -PresentOnly | Where-Object { $_.InstanceId -match 'Target_KIP|MSHW0153' } | Sort-Object InstanceId) {
        $p = @{}
        Get-PnpDeviceProperty -InstanceId $d.InstanceId -KeyName DEVPKEY_Device_Service, DEVPKEY_Device_UpperFilters, DEVPKEY_Device_LowerFilters, DEVPKEY_Device_DriverInfPath, DEVPKEY_Device_DriverVersion -ErrorAction SilentlyContinue |
            ForEach-Object { $p[$_.KeyName -replace 'DEVPKEY_Device_', ''] = ($_.Data -join ',') }
        "{0}`t{1}`t{2}`tservice={3} upper={4} lower={5} inf={6} ver={7}" -f (& $maskShp $d.InstanceId), $d.Class, $d.FriendlyName, $p.Service, $p.UpperFilters, $p.LowerFilters, $p.DriverInfPath, $p.DriverVersion
    }
    $lines | Set-Content -Encoding utf8 (Join-Path $hw 'devices.txt')

    $reg = @()
    foreach ($k in 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\PrecisionTouchPad', 'HKCU:\Software\Microsoft\Windows\CurrentVersion\PrecisionTouchPad',
                   'HKCU:\Software\Microsoft\Windows\CurrentVersion\PrecisionTouchPad\Status', 'HKLM:\SYSTEM\CurrentControlSet\Services\SurfaceService') {
        $reg += "[$k]"
        $item = Get-ItemProperty $k -ErrorAction SilentlyContinue
        if ($item) { $item.PSObject.Properties | Where-Object { $_.Name -notmatch '^PS' -and $_.Name -notmatch 'ImagePath|FailureActions|Description|DisplayName|Owners' } | ForEach-Object { "{0} = {1}" -f $_.Name, ($_.Value -join ',') } | ForEach-Object { $reg += $_ } }
        $reg += ''
    }
    $reg | Set-Content -Encoding utf8 (Join-Path $hw 'ptp-registry.txt')
    "done: $hw"
    exit 0
}

# Interactive (or -Auto) session.
$run = if ($OutDir) { $OutDir } else { Join-Path $repo ("private\windows\typecover\" + (Get-Date -Format 'yyyyMMdd-HHmmss')) }
New-Item -ItemType Directory -Force $run | Out-Null
$run = (Resolve-Path $run).Path
$session = 'sp9typecover'
$providers = @(
    '{47c779cd-4efd-49d7-9b10-9f16e5c25d06} 0xffffffff 0x5'           # HIDCLASS WPP (rundown + per-request IOCTL/report id)
    '{6465DA78-E7A0-4F39-B084-8F53C7C30DC6} 0xffffffffffffffff 0x5'   # Microsoft-Windows-Input-HIDCLASS
    '{2FEA7205-B0B1-41CA-8609-5A1D16F3132F} 0xffffffffffffffff 0x5'   # Microsoft-Surface-SurfaceHidMiniDriver
    '{8DDB06D0-A54B-4014-B119-5A09FB550D18} 0xffffffffffffffff 0x5'   # Microsoft-Surface-SurfaceSerialHubDriver
    '{69CD60E3-430F-4DA4-B1B3-E3BDAF945875} 0xffffffffffffffff 0x5'   # Microsoft-Surface-SurfaceHotPlug
)
if (-not $NoEtw) {
    $pf = Join-Path $run 'providers.txt'
    Set-Content -Encoding ascii $pf $providers
    logman stop $session -ets 2>$null | Out-Null
    logman start $session -ets -o (Join-Path $run 'input.etl') -pf $pf -bs 256 -nb 16 64 -mode sequential -max 512 | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "logman start failed ($LASTEXITCODE)" }
}
try {
    if ($Auto -eq 0) {
        "The Type Cover test window opens. Follow its instructions and press Enter after each step."
        "Do not press Play/Pause. The screen stays on."
    }
    $a = @('session', $run); if ($Auto -gt 0) { $a += '--auto', "$Auto" }
    $p = Start-Process -FilePath $exe -ArgumentList $a -PassThru -Wait
    "session exit $($p.ExitCode)"
} finally {
    if (-not $NoEtw) { logman stop $session -ets | Out-Null }
}
Decode $run
"done: $run"
