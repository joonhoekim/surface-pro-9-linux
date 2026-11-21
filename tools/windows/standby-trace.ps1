# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# Modern Standby (S0ix) entry/exit trace: start ETW (Kernel-Power, Kernel-Pep, Kernel-Acpi, PDC, Intel PEP, Surface
# drivers), enter Modern Standby by turning the display off, wait for the exit, stop ETW and summarize the sequence
# with standby-summary.py.
#
# Usage (elevated):
#   powershell -ExecutionPolicy Bypass -File tools\windows\run-elevated.ps1 tools\windows\standby-trace.ps1 -Minutes 10
#   ... standby-trace.ps1 -DryRun                     # checks + 5 s trace + summary, no standby, no power setting change
#   ... standby-trace.ps1 -Summarize <dir>            # re-summarize an earlier run
#   ... standby-trace.ps1 -TryTimerWake -Minutes 5    # experimental: try to leave standby by itself (see below)
#
# Waking (default, -Manual): a person presses the power button once after -Minutes; the script waits for Kernel-Power
# 507 up to -Minutes + 30 min. The script's own output is frozen during standby (and hidden under run-elevated.ps1),
# so the instruction line is printed before the display goes off and kept in run.txt.
# -TryTimerWake: standby-wake.ps1 arms a resume timer in a service and tries a power request, an input nudge and the
# power button path. On this device none of them has ended Modern Standby (docs/research/control/standby.md, section 9); if 507 does
# not follow, the run falls back to waiting for the person. "Allow wake timers" (RTCWAKE) is set to Enable for AC and
# DC only in this mode, and restored in finally.
# Entering (-ByLid): a person closes the Type Cover over the screen; the script only waits up to 5 min for 506
# (lid path) and the person opens the cover after -Minutes to wake.
# Entering (default): SC_MONITORPOWER 2 posted to the shell window; on this device that enters Modern Standby at once (506
# reason SC_MONITORPOWER). If 506 does not follow within 90 s, the cycle is recorded as screen-off only.
#
# ETW is a logman session, not a WPR profile: a WPR profile with these provider GUIDs enabled only PDC, SleepStudy and
# UserModePowerService (0 Kernel-Power/Acpi/Pep/Surface events in a 10 min standby run). Kernel-Pep, Kernel-Acpi, PDC
# and Intel PEP get a capture-state request after start and before stop (PepDeviceRundown = applied constraints).
#
# Output: private/windows/standby/<run>/ (trace.etl, trace.tsv, sleepstudy.xml, run.txt, summary.md). The raw files
# carry the machine name and device instance paths; copy only model-level facts into docs.
param(
    [int]$Minutes = 10,
    [int]$Cycles = 1,
    [switch]$Manual,
    [switch]$TryTimerWake,
    [switch]$ByLid,
    [int]$AwakeSeconds = 60,
    [int]$LeadSeconds = 15,
    [switch]$DryRun,
    [string]$Summarize,
    [string]$Out
)
$ErrorActionPreference = 'Continue'   # native tools write to stderr; failures are checked explicitly
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$root = Split-Path -Parent (Split-Path -Parent $here)
$session = 'sp9standby'
if ($Manual -and $TryTimerWake) { throw '-Manual and -TryTimerWake exclude each other' }

function Invoke-Summary([string]$dir) {
    $py = (Get-Command python -ErrorAction SilentlyContinue).Source
    if (-not $py) { "python not found: run  python -I -X utf8 tools\windows\standby-summary.py `"$dir`"  later"; return }
    & $py -I -X utf8 (Join-Path $here 'standby-summary.py') $dir
}

if ($Summarize) { Invoke-Summary (Resolve-Path $Summarize).Path; exit 0 }

# name, guid, keywords, level, capture state
$providers = @(
    # Kernel-Power without Diagnostic(0x4), Performance(0x8), Idle(0x10), TimerResolution(0x4000): those are ~4000
    # events/s while awake. Kept: Scenario, Simple, Thermal, PowerSetting, RuntimeFx (PoFx), SleepStudy,
    # DiagnosticLight (506/507/566/172), SleepDiagnostic, WakeDiagnostics.
    @('Microsoft-Windows-Kernel-Power',           '331C3B3A-2005-44C2-AC5E-77220C37D6B4', '0x113A3', 5, $false),
    @('Microsoft-Windows-Kernel-Pep',             '5412704E-B2E1-4624-8FFD-55777B8F7373', '0xFFFFFFFFFFFFFFFF', 5, $true),
    @('Microsoft-Windows-Kernel-Acpi',            'C514638F-7723-485B-BCFC-96565D735D4A', '0xFFFFFFFFFFFFFFFF', 5, $true),
    @('Microsoft-Windows-PDC',                    'A6BF0DEB-3659-40AD-9F81-E25AF62CE3C7', '0xFFFFFFFFFFFFFFFF', 5, $true),
    @('Microsoft-Windows-SleepStudy',             'D37687E7-8BF0-4D11-B589-A7ABE080756A', '0xFFFFFFFFFFFFFFFF', 5, $false),
    @('Microsoft-Windows-UserModePowerService',   'CE8DEE0B-D539-4000-B0F8-77BED049C590', '0xFFFFFFFFFFFFFFFF', 5, $false),
    @('IntelPepTraceLoggingProvider',             '9B82887E-0D74-4A2E-85FF-521F93C9D4A0', '0xFFFFFFFFFFFFFFFF', 5, $true),
    @('Microsoft-Surface-SurfaceAcpiPlatformExtensionDriver', 'ABA759C2-2F7B-41C2-8CFC-71C370E17276', '0xFFFFFFFFFFFFFFFF', 5, $false),
    @('Microsoft-Surface-SurfaceSerialHubDriver', '8DDB06D0-A54B-4014-B119-5A09FB550D18', '0xFFFFFFFFFFFFFFFF', 5, $false),
    @('Microsoft-Surface-SurfaceSmfCoreDriver',   '0EC12C6E-794A-4B5E-8E27-6D3DC98BBD38', '0xFFFFFFFFFFFFFFFF', 5, $false),
    @('Microsoft-Surface-SurfaceSmfClient',       'A8D5FCB2-FF94-4166-920D-3338902E77E0', '0xFFFFFFFFFFFFFFFF', 5, $false),
    @('Microsoft-Surface-SmfCpuClientDriver',     '32760E2C-4AC2-40EB-9972-1C69C12ED757', '0xFFFFFFFFFFFFFFFF', 5, $false),
    @('Microsoft-Surface-SurfacePowerState',      '44F35D0A-A3FA-4248-A7CB-22FE08B3288D', '0xFFFFFFFFFFFFFFFF', 5, $false),
    @('Microsoft-Surface-SurfacePowerTrackerCore','23AC50B3-680E-7AD3-A2B3-D5DBEF945870', '0xFFFFFFFFFFFFFFFF', 5, $false),
    @('Microsoft-Surface-SurfaceHotPlug',         '69CD60E3-430F-4DA4-B1B3-E3BDAF945875', '0xFFFFFFFFFFFFFFFF', 5, $false),
    @('Microsoft-Surface-SurfaceHidMiniDriver',   '2FEA7205-B0B1-41CA-8609-5A1D16F3132F', '0xFFFFFFFFFFFFFFFF', 5, $false),
    @('Microsoft-Surface-SurfaceBattery',         'DCF09792-3C77-4D1C-8B99-A8A089841155', '0xFFFFFFFFFFFFFFFF', 5, $false),
    @('Intel-iaLPSS2-UART2',                      'B87CAA6F-37A7-4F09-8DC4-D15572C5904E', '0xFFFFFFFFFFFFFFFF', 4, $false),
    @('IntelTHCBase',                             '937AD94E-CA8D-4B8E-8143-3FCE4ACCB8CB', '0xFFFFFFFFFFFFFFFF', 4, $false)
)

$etwSrc = @'
using System;
using System.Runtime.InteropServices;
public static class Sp9Etw {
    [DllImport("advapi32.dll", CharSet = CharSet.Unicode)] static extern uint ControlTraceW(ulong h, string name, IntPtr props, uint code);
    [DllImport("advapi32.dll")] static extern uint EnableTraceEx2(ulong h, ref Guid provider, uint control, byte level, ulong any, ulong all, uint timeout, IntPtr p);
    // EVENT_CONTROL_CODE_CAPTURE_STATE for one provider of a running session; returns a Win32 error code
    public static uint CaptureState(string session, string guid, ulong keywords) {
        IntPtr p = Marshal.AllocHGlobal(2048);
        try {
            for (int i = 0; i < 2048; i++) Marshal.WriteByte(p, i, 0);
            Marshal.WriteInt32(p, 0, 2048);           // Wnode.BufferSize
            Marshal.WriteInt32(p, 44, 0x00020000);    // Wnode.Flags = WNODE_FLAG_TRACED_GUID
            Marshal.WriteInt32(p, 112, 120 + 1024);   // LogFileNameOffset
            Marshal.WriteInt32(p, 116, 120);          // LoggerNameOffset
            uint rc = ControlTraceW(0, session, p, 0 /* EVENT_TRACE_CONTROL_QUERY */);
            if (rc != 0) return rc;
            ulong h = (ulong)Marshal.ReadInt64(p, 8); // Wnode.HistoricalContext = session handle
            Guid g = new Guid(guid);
            return EnableTraceEx2(h, ref g, 2 /* EVENT_CONTROL_CODE_CAPTURE_STATE */, 5, keywords, 0, 0, IntPtr.Zero);
        } finally { Marshal.FreeHGlobal(p); }
    }
}
'@
function Add-Etw { if (-not ('Sp9Etw' -as [type])) { Add-Type -TypeDefinition $etwSrc } }

function Request-CaptureState([string]$what) {
    foreach ($p in $providers | Where-Object { $_[4] }) {
        $rc = [Sp9Etw]::CaptureState($session, $p[1], [Convert]::ToUInt64($p[2].Substring(2), 16))
        Note "capture state ($what) $($p[0]): rc=$rc"
    }
}

function Get-PowerIndex([string]$sub, [string]$alias) {
    $q = cmd /c "powercfg /qh SCHEME_CURRENT $sub $alias 2>nul"
    $ac = ($q | Select-String 'Current AC Power Setting Index: 0x([0-9a-fA-F]+)').Matches | Select-Object -First 1
    $dc = ($q | Select-String 'Current DC Power Setting Index: 0x([0-9a-fA-F]+)').Matches | Select-Object -First 1
    [pscustomobject]@{ AC = if ($ac) { [Convert]::ToInt32($ac.Groups[1].Value, 16) } else { $null }
                       DC = if ($dc) { [Convert]::ToInt32($dc.Groups[1].Value, 16) } else { $null } }
}

function Get-OnAC { try { [bool](Get-CimInstance -Namespace root\wmi -ClassName BatteryStatus | Select-Object -First 1).PowerOnline } catch { $true } }

# ---- run ---------------------------------------------------------------------------------------------------------
$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) { throw 'run elevated: tools\windows\run-elevated.ps1 tools\windows\standby-trace.ps1 ...' }
if (-not $Out) { $Out = Join-Path $root ('private\windows\standby\' + (Get-Date -Format 'yyyyMMdd-HHmmss') + $(if ($DryRun) { '-dry' } else { '' })) }
New-Item -ItemType Directory -Force $Out | Out-Null
$runTxt = Join-Path $Out 'run.txt'
function Note([string]$s) { $s; Add-Content -Encoding utf8 -Path $runTxt -Value $s }

. (Join-Path $here 'standby-wake.ps1')   # Enter-StandbyDisplayOff, Wait-StandbyExit, Start/Stop-StandbyWake, [Sp9Wake]
Add-WakeHelper
Add-Etw

$wake = if ($TryTimerWake) { 'timer (experimental)' } else { 'manual (power button)' }
$rtc = Get-PowerIndex SUB_SLEEP RTCWAKE
Note "wake=$wake minutes=$Minutes cycles=$Cycles dryrun=$([bool]$DryRun)"
Note "on_ac=$(Get-OnAC) allow_wake_timers ac=$($rtc.AC) dc=$($rtc.DC)"
Note ("powercfg /a: " + ((powercfg /a | Select-Object -First 3) -join ' / '))

$pf = Join-Path $Out 'providers.txt'
Set-Content -Encoding ascii $pf ($providers | ForEach-Object { "{$($_[1])} $($_[2]) 0x$('{0:x}' -f $_[3])" })
$etl = Join-Path $Out 'trace.etl'
logman query $session -ets *> $null
if ($LASTEXITCODE -eq 0) { throw "ETW session $session is already running; stop it with: logman stop $session -ets" }

$traceStarted = $false; $rtcChanged = $false; $w = $null
try {
    logman start $session -ets -o $etl -pf $pf -bs 1024 -nb 16 128 -mode sequential -max 2048 | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "logman start failed ($LASTEXITCODE)" }
    $traceStarted = $true
    Note "trace started: $etl"
    Request-CaptureState 'start'

    if ($DryRun) {
        Note "dry run: wake helper compiled, last standby event = $((Get-StandbyEvents (Get-Date).AddDays(-7) | Select-Object -Last 1).Id); tracing 5 s"
        Start-Sleep -Seconds 5
    } else {
        if ($TryTimerWake -and ($rtc.AC -ne 1 -or $rtc.DC -ne 1)) {
            powercfg /setacvalueindex SCHEME_CURRENT SUB_SLEEP RTCWAKE 1
            powercfg /setdcvalueindex SCHEME_CURRENT SUB_SLEEP RTCWAKE 1
            powercfg /setactive SCHEME_CURRENT
            $rtcChanged = $true
        }
        for ($c = 1; $c -le $Cycles; $c++) {
            $at = (Get-Date).AddSeconds($LeadSeconds)
            $press = $at.AddMinutes($Minutes)
            if ($ByLid) {
                Note "cycle ${c}: CLOSE THE COVER over the screen after $($at.ToString('HH:mm:ss')); OPEN IT AT $($at.AddMinutes($Minutes).ToString('HH:mm:ss')) (after $Minutes min)."
            } elseif ($TryTimerWake) {
                $w = Start-StandbyWake -Seconds ($LeadSeconds + 60 * $Minutes)
                Note "cycle ${c}: display off at $($at.ToString('HH:mm:ss')); timer wake armed for $($press.ToString('HH:mm:ss')). If the screen is still off 5 min after that, press the power button once."
            } else {
                Note "cycle ${c}: display off at $($at.ToString('HH:mm:ss')). PRESS THE POWER BUTTON ONCE AT $($press.ToString('HH:mm:ss')) (after $Minutes min)."
            }
            Start-Sleep -Seconds $LeadSeconds   # quiet baseline in the trace, time to read the line above
            $t0 = Get-Date
            if ($ByLid) {
                Note "cycle ${c}: waiting for the cover to close from $($t0.ToString('o'))"
                $first = Wait-StandbyExit -Since $t0 -TimeoutSeconds 300
            } else {
                Note "cycle ${c}: entering standby at $($t0.ToString('o')) via SC_MONITORPOWER (shell window)"
                Enter-StandbyDisplayOff | Out-Null
                $first = Wait-StandbyExit -Since $t0 -TimeoutSeconds 90
            }
            if (-not $first.Entered) {
                Note "cycle ${c}: no 506 within the entry window; turning the display back on"
                [void][Sp9Wake]::Monitor(-1)
                $r = $first
            } elseif ($first.Exited) {
                $r = $first
            } else {
                $budget = 60 * $Minutes + $(if ($TryTimerWake) { 600 } else { 1800 })
                $r = Wait-StandbyExit -Since $t0 -TimeoutSeconds $budget
                if ($TryTimerWake -and -not $r.Exited) {
                    Note "cycle ${c}: timer wake did not end standby; waiting up to 30 min for the power button"
                    $r = Wait-StandbyExit -Since $t0 -TimeoutSeconds 1800
                }
            }
            Note ("cycle ${c}: 506=" + $(if ($r.Entered) { $r.Entered.TimeCreated.ToString('o') + ' ' + ($r.Entered.Message -replace '\s+', ' ') } else { 'none' }) +
                  " | 507=" + $(if ($r.Exited) { $r.Exited.TimeCreated.ToString('o') + ' ' + ($r.Exited.Message -replace '\s+', ' ') } else { 'none' }))
            if ($w) {
                Stop-StandbyWake $w; $w = $null
                if (Test-Path (Join-Path $env:ProgramData 'sp9-standby-wake.log')) {
                    Get-Content (Join-Path $env:ProgramData 'sp9-standby-wake.log') | Select-Object -Last 8 | ForEach-Object { Note "  wake log: $_" }
                }
            }
            if ($r.Entered -and -not $r.Exited) { Note "cycle ${c}: still in standby at $((Get-Date).ToString('o')); stopping"; break }
            if ($c -lt $Cycles) { Start-Sleep -Seconds $AwakeSeconds }
        }
        Start-Sleep -Seconds 10
    }
} finally {
    if ($w) { Stop-StandbyWake $w }
    if ($rtcChanged) {
        powercfg /setacvalueindex SCHEME_CURRENT SUB_SLEEP RTCWAKE $rtc.AC
        powercfg /setdcvalueindex SCHEME_CURRENT SUB_SLEEP RTCWAKE $rtc.DC
        powercfg /setactive SCHEME_CURRENT
        $now = Get-PowerIndex SUB_SLEEP RTCWAKE
        Note "allow wake timers restored ac=$($now.AC) dc=$($now.DC)"
    }
    if ($traceStarted) {
        Request-CaptureState 'stop'
        Start-Sleep -Seconds 2
        logman stop $session -ets | Out-Null
        if ($LASTEXITCODE -ne 0) { Note "logman stop failed (exit $LASTEXITCODE)" }
    }
}
if (-not $DryRun) { powercfg /sleepstudy /xml /duration 1 /output (Join-Path $Out 'sleepstudy.xml') | Out-Null }
if (Test-Path $etl) { Invoke-Summary $Out }
"output: $Out"
