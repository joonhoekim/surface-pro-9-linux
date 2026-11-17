# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# Capture what changes in software when the Type Cover's physical posture changes (e.g. the pen-tray flap folded against
# the bottom bezel, covering the Slim Pen). Read-only: GET only toward the cover, no settings changed.
# Usage (elevated): tools\windows\run-elevated.ps1 tools\windows\posture-trace.ps1 -Seconds 60
#   1. snapshot A: typecover-probe automatic part (features, devices) + slate/posture registry + PnP list
#   2. ETW for -Seconds (Surface PowerTrackerCore posture node, SMF, Kernel-ACPI, Kernel-PnP); the person changes the posture
#      once during that window and leaves it
#   3. snapshot B, then diffs and a decoded event list in private/windows/posture/<run>/
param([int]$Seconds = 60, [string]$Label = 'change')
$ErrorActionPreference = 'Continue'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$repo = Split-Path -Parent (Split-Path -Parent $here)
$run = Join-Path $repo ("private\windows\posture\" + (Get-Date -Format 'yyyyMMdd-HHmmss') + "-$Label")
New-Item -ItemType Directory -Force $run | Out-Null

function Snapshot([string]$name) {
    $d = Join-Path $run $name
    New-Item -ItemType Directory -Force $d | Out-Null
    & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $here 'typecover-probe.ps1') -OutDir $d *> (Join-Path $d 'probe.log')
    Get-PnpDevice -PresentOnly | Sort-Object InstanceId | ForEach-Object { "$($_.Status)`t$($_.Class)`t$($_.FriendlyName)`t$($_.InstanceId -replace '\\[^\\]*$', '\...')" } |
        Out-File -Encoding utf8 (Join-Path $d 'pnp.txt')
    $pc = Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\PriorityControl' -ErrorAction SilentlyContinue
    $im = Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\ImmersiveShell' -ErrorAction SilentlyContinue
    @("ConvertibleSlateMode=$($pc.ConvertibleSlateMode)", "TabletMode=$($im.TabletMode)",
      "SM_CONVERTIBLESLATEMODE=$([Sp9Posture]::Metric(0x2003))", "SM_SYSTEMDOCKED=$([Sp9Posture]::Metric(0x2004))") |
        Out-File -Encoding utf8 (Join-Path $d 'slate.txt')
}
Add-Type -TypeDefinition 'using System.Runtime.InteropServices; public static class Sp9Posture { [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i); public static int Metric(int i) { return GetSystemMetrics(i); } }'

"snapshot A"; Snapshot 'A'
$session = 'sp9posture'
$providers = @(
    @('Microsoft-Surface-SurfacePowerTrackerCore', '{23AC50B3-680E-7AD3-A2B3-D5DBEF945870}'),
    @('Microsoft-Surface-SurfaceSmfClient', '{A8D5FCB2-FF94-4166-920D-3338902E77E0}'),
    @('Microsoft-Surface-SurfaceHotPlug', '{69CD60E3-430F-4DA4-B1B3-E3BDAF945875}'),
    @('Microsoft-Surface-SurfaceHidMiniDriver', '{2FEA7205-B0B1-41CA-8609-5A1D16F3132F}'),
    @('Microsoft-Windows-Kernel-Acpi', '{C514638F-7723-485B-BCFC-96565D735D4A}'),
    @('Microsoft-Windows-Kernel-PnP', '{9C205A39-1250-487D-ABD7-E831C6290539}'))
$pf = Join-Path $run 'providers.txt'
$providers | ForEach-Object { "$($_[1]) 0xFFFFFFFFFFFFFFFF 5" } | Out-File -Encoding ascii $pf
logman stop $session -ets 2>$null | Out-Null
logman start $session -ets -o (Join-Path $run 'trace.etl') -pf $pf -bs 256 -nb 16 64 -mode sequential -max 256 | Out-Null
$t0 = Get-Date
"tracing $Seconds s from $($t0.ToString('HH:mm:ss')): change the posture once now"
Start-Sleep -Seconds $Seconds
logman stop $session -ets | Out-Null
"snapshot B"; Snapshot 'B'

& { foreach ($f in 'features.txt', 'devices.txt', 'pnp.txt', 'slate.txt') {
    $a = Join-Path $run "A\$f"; $b = Join-Path $run "B\$f"
    if ((Test-Path $a) -and (Test-Path $b)) {
        $diff = Compare-Object (Get-Content $a) (Get-Content $b)
        "## $f : $(@($diff).Count) changed lines"
        $diff | ForEach-Object { "  $($_.SideIndicator) $($_.InputObject)" }
    }
} } | Tee-Object -FilePath (Join-Path $run 'diff.txt')

$exe = Join-Path $repo 'out\standby\etwdecode.exe'
if (Test-Path $exe) {
    & $exe (Join-Path $run 'trace.etl') (Join-Path $run 'trace.tsv') 2>&1 | Out-Null
    if (Test-Path (Join-Path $run 'trace.tsv')) {
        "## events by provider/event"
        Import-Csv (Join-Path $run 'trace.tsv') -Delimiter "`t" -Header t, provider, id, task, name, props -ErrorAction SilentlyContinue |
            Group-Object provider, name | Sort-Object Count -Descending | Select-Object -First 25 Count, Name | Format-Table -AutoSize | Out-String
    }
}
"output: $run"
