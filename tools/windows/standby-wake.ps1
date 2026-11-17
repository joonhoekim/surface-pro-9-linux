# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# Timed exit from Modern Standby for unattended runs (used by standby-trace.ps1 and input-trace.ps1).
#
# Desktop processes and processes started by Task Scheduler are frozen while the system is in Modern Standby, so
# their timers never act. The wake therefore runs in a Windows service (standby-wake.cs, built to
# out/standby/sp9wake.exe): it arms a resume timer (SetWaitableTimerEx, fResume) and, when it fires, tries
#   request  PowerSetRequest(DisplayRequired + SystemRequired) and SetThreadExecutionState
#   nudge    an on-demand task as the logged-on user: SendInput (1-px mouse move) + SC_MONITORPOWER -1
#   button   NtPowerInformation(PhysicalPowerButtonPress)
# until Kernel-Power 507 (standby exit) appears. The log line "wake: exit=<method>" records which one worked.
#
# Library use (dot-source, elevated):
#   . tools\windows\standby-wake.ps1
#   $w = Start-StandbyWake -Seconds 600 -Log <file>     # build, install and start the service; register the nudge task
#   Enter-StandbyDisplayOff                              # SC_MONITORPOWER 2 to the shell window (enters standby here)
#   Wait-StandbyExit -Since $t0 -TimeoutSeconds 900      # @{Entered=<506 event>; Exited=<507 event>}
#   Stop-StandbyWake $w                                  # remove the service and the task
# "Allow wake timers" must be Enable for the current source; the callers set it and restore it in finally.
param(
    [switch]$Nudge,
    [int]$Seconds = 600,
    [string]$Log = (Join-Path $env:ProgramData 'sp9-standby-wake.log'),
    [string]$NudgeTask = 'sp9-standby-nudge'
)

$script:WakeHelperSrc = @'
using System;
using System.Runtime.InteropServices;
public static class Sp9Wake {
    [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)]
    struct REASON_CONTEXT { public uint Version, Flags; public IntPtr SimpleReasonString; }
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern IntPtr CreateWaitableTimerExW(IntPtr a, string n, uint flags, uint access);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool SetWaitableTimerEx(IntPtr h, ref long due, int period, IntPtr f, IntPtr a, ref REASON_CONTEXT ctx, uint tolerable);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool SetWaitableTimer(IntPtr h, ref long due, int period, IntPtr f, IntPtr a, bool resume);
    [DllImport("kernel32.dll")] static extern uint WaitForSingleObject(IntPtr h, uint ms);
    [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr PowerCreateRequest(ref REASON_CONTEXT ctx);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool PowerSetRequest(IntPtr h, int type);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool PowerClearRequest(IntPtr h, int type);
    [DllImport("kernel32.dll")] static extern uint SetThreadExecutionState(uint f);
    [DllImport("ntdll.dll")] static extern int NtPowerInformation(int level, ref byte inBuf, int inLen, IntPtr outBuf, int outLen);
    [DllImport("ntdll.dll")] static extern int RtlAdjustPrivilege(int priv, bool enable, bool thread, out bool was);
    [DllImport("user32.dll")] static extern uint SendInput(uint n, INPUT[] i, int cb);
    [DllImport("user32.dll")] static extern IntPtr GetShellWindow();
    [DllImport("user32.dll")] static extern bool PostMessageW(IntPtr h, uint m, IntPtr w, IntPtr l);
    [StructLayout(LayoutKind.Sequential)] struct MOUSEINPUT { public int dx, dy; public uint data, flags, time; public IntPtr extra; }
    [StructLayout(LayoutKind.Sequential)] struct INPUT { public uint type; public MOUSEINPUT mi; }

    static REASON_CONTEXT Reason(string s) {
        REASON_CONTEXT c; c.Version = 0; c.Flags = 1; /* POWER_REQUEST_CONTEXT_SIMPLE_STRING */ c.SimpleReasonString = Marshal.StringToHGlobalUni(s); return c;
    }
    public static IntPtr ArmTimer(int seconds) {
        IntPtr t = CreateWaitableTimerExW(IntPtr.Zero, null, 1 /* MANUAL_RESET */, 0x1F0003);
        long due = -10000000L * seconds;
        REASON_CONTEXT c = Reason("sp9 standby trace timed wake");
        if (!SetWaitableTimerEx(t, ref due, 0, IntPtr.Zero, IntPtr.Zero, ref c, 0))
            if (!SetWaitableTimer(t, ref due, 0, IntPtr.Zero, IntPtr.Zero, true)) throw new Exception("SetWaitableTimer " + Marshal.GetLastWin32Error());
        return t;
    }
    public static bool Wait(IntPtr t, int ms) { return WaitForSingleObject(t, (uint)ms) == 0; }
    static IntPtr req = IntPtr.Zero;
    public static string Request() {
        REASON_CONTEXT c = Reason("sp9 standby trace timed wake");
        if (req == IntPtr.Zero) req = PowerCreateRequest(ref c);
        bool a = PowerSetRequest(req, 0 /* DisplayRequired */), b = PowerSetRequest(req, 1 /* SystemRequired */);
        uint s = SetThreadExecutionState(0x80000000 | 0x2 | 0x1);
        return "display=" + a + " system=" + b + " stes=0x" + s.ToString("X");
    }
    public static void ClearRequest() {
        if (req != IntPtr.Zero) { PowerClearRequest(req, 0); PowerClearRequest(req, 1); }
        SetThreadExecutionState(0x80000000);
    }
    public static string Button() {
        bool was; RtlAdjustPrivilege(19 /* SeShutdownPrivilege */, true, false, out was);
        byte pressed = 1;
        return "0x" + NtPowerInformation(90 /* PhysicalPowerButtonPress */, ref pressed, 1, IntPtr.Zero, 0).ToString("X8");
    }
    public static uint Nudge() {
        INPUT[] i = new INPUT[2];
        i[0].type = 0; i[0].mi.dx = 1; i[0].mi.flags = 1; i[1].type = 0; i[1].mi.dx = -1; i[1].mi.flags = 1;
        return SendInput(2, i, Marshal.SizeOf(typeof(INPUT)));
    }
    // SC_MONITORPOWER to the shell window only. A broadcast can sit in a frozen window's queue and turn the display
    // off again after the wake.
    public static bool Monitor(int state) { return PostMessageW(GetShellWindow(), 0x0112, (IntPtr)0xF170, (IntPtr)state); }
}
'@
function Add-WakeHelper { if (-not ('Sp9Wake' -as [type])) { Add-Type -TypeDefinition $script:WakeHelperSrc } }

function Write-WakeLog([string]$m) { "{0:o} {1}" -f (Get-Date), $m | Add-Content -Encoding utf8 $Log }

function Get-StandbyEvents([datetime]$Since) {
    @(Get-WinEvent -FilterHashtable @{ LogName = 'System'; ProviderName = 'Microsoft-Windows-Kernel-Power'; Id = 506, 507; StartTime = $Since } -ErrorAction SilentlyContinue | Sort-Object TimeCreated)
}
function Test-InStandby([datetime]$Since) {
    $e = Get-StandbyEvents $Since
    $e.Count -gt 0 -and $e[-1].Id -eq 506
}

# ---- nudge (logged-on user, interactive) -------------------------------------------------------------------------
if ($Nudge) {
    Add-WakeHelper
    $n = [Sp9Wake]::Nudge(); [void][Sp9Wake]::Monitor(-1)
    Start-Sleep -Seconds 2
    $n2 = [Sp9Wake]::Nudge()
    Write-WakeLog "nudge: SendInput=$n,$n2 monitor-on posted"
    exit 0
}

# ---- library -----------------------------------------------------------------------------------------------------
$script:WakeSelf = $MyInvocation.MyCommand.Path

function Start-StandbyWake([int]$Seconds, [string]$Log = (Join-Path $env:ProgramData 'sp9-standby-wake.log')) {
    $repo = Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $script:WakeSelf))
    $exe = Join-Path $repo 'out\standby\sp9wake.exe'
    $src = Join-Path (Split-Path -Parent $script:WakeSelf) 'standby-wake.cs'
    if (-not (Test-Path $exe) -or (Get-Item $src).LastWriteTime -gt (Get-Item $exe).LastWriteTime) {
        New-Item -ItemType Directory -Force (Split-Path -Parent $exe) | Out-Null
        & "$env:WINDIR\Microsoft.NET\Framework64\v4.0.30319\csc.exe" /nologo /optimize+ /platform:x64 /r:System.ServiceProcess.dll "/out:$exe" $src | Out-Null
        if (-not (Test-Path $exe)) { throw "build failed: $exe" }
    }
    $ps = Join-Path $env:WINDIR 'System32\WindowsPowerShell\v1.0\powershell.exe'
    $set = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -MultipleInstances IgnoreNew
    $user = (Get-CimInstance Win32_ComputerSystem).UserName
    if (-not $user) { throw 'no interactive user logged on' }
    $nArg = "-NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File `"$script:WakeSelf`" -Nudge -Log `"$Log`""
    Register-ScheduledTask -TaskName 'sp9-standby-nudge' -Force -ErrorAction Stop -Settings $set `
        -Action (New-ScheduledTaskAction -Execute $ps -Argument $nArg) `
        -Principal (New-ScheduledTaskPrincipal -UserId $user -LogonType Interactive -RunLevel Highest) | Out-Null
    Stop-StandbyWake $null
    sc.exe create sp9wake binPath= "`"$exe`" $Seconds `"$Log`" sp9-standby-nudge" type= own start= demand | Out-Null
    sc.exe start sp9wake | Out-Null
    $deadline = (Get-Date).AddSeconds(30)
    while ((Get-Date) -lt $deadline) {
        if ((Test-Path $Log) -and (Select-String -Path $Log -Pattern "service: armed $Seconds s ok=True" -Quiet)) { break }
        Start-Sleep -Milliseconds 500
    }
    if (-not (Select-String -Path $Log -Pattern "service: armed $Seconds s ok=True" -Quiet -ErrorAction SilentlyContinue)) { throw "wake service did not arm (see $Log)" }
    @{ Log = $Log; Armed = (Get-Date) }
}

function Stop-StandbyWake($w) {
    sc.exe stop sp9wake 2>$null | Out-Null
    sc.exe delete sp9wake 2>$null | Out-Null
    if ($w) { Unregister-ScheduledTask -TaskName 'sp9-standby-nudge' -Confirm:$false -ErrorAction SilentlyContinue }
}

function Enter-StandbyDisplayOff { Add-WakeHelper; [Sp9Wake]::Monitor(2) }

function Wait-StandbyExit([datetime]$Since, [int]$TimeoutSeconds) {
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    $entered = $null; $exited = $null
    while ((Get-Date) -lt $deadline) {
        Start-Sleep -Seconds 3
        $ev = Get-StandbyEvents $Since
        if (-not $entered) { $entered = $ev | Where-Object Id -eq 506 | Select-Object -First 1 }
        if ($entered) { $exited = $ev | Where-Object { $_.Id -eq 507 -and $_.TimeCreated -ge $entered.TimeCreated } | Select-Object -First 1 }
        if ($exited) { break }
    }
    @{ Entered = $entered; Exited = $exited }
}
