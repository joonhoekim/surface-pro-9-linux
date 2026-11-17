# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# Record which HID control requests Windows sends to the input devices (THC touch controller, Type Cover/KIP,
# SAM HID, Surface Button VHF) across idle, display off/on and Modern Standby with a timed wake, and optional
# manual steps (cover detach/attach, keyboard backlight key, cover folded over the screen).
#
# What is captured (all raw output goes to private/, the device paths in summaries have Bluetooth addresses masked):
#   - ETW: HIDCLASS WPP (47c779cd-...; rundown maps device/collection pointers, per-request events carry the
#     IOCTL code and report id), Microsoft-Windows-Input-HIDCLASS, IntelTHCBase (DMA read/write type and length),
#     Surface HID mini driver / serial hub / hot plug. Payload bytes of SET requests are NOT in these events.
#   - Feature snapshots: read-only HidD_GetFeature of every feature report of the THC/KIP/VHF collections at each
#     phase boundary (pen-bonding, pen-id and firmware-update collections are skipped). Diffs show what changed.
#   - markers.csv (FILETIME UTC per phase), process list per phase (pid -> name), Kernel-Power standby events.
# The script never sends SET_FEATURE/OUTPUT itself. It changes one setting only with -Standby: "allow wake timers"
# (SUB_SLEEP/RTCWAKE) on the active scheme, restored in finally.
#
# Usage (elevated; relaunches itself through run-elevated.ps1, or a visible elevated console with -Manual):
#   powershell -ExecutionPolicy Bypass -File tools\windows\input-trace.ps1                 # idle only
#   powershell -ExecutionPolicy Bypass -File tools\windows\input-trace.ps1 -Standby        # + display off (= Modern Standby
#       entry on this device) and SetSuspendState, each woken by a resume timer
#   powershell -ExecutionPolicy Bypass -File tools\windows\input-trace.ps1 -Standby -Manual
#   -IdleSeconds N (60), -DisplayOffSeconds N (60), -StandbySeconds N (120), -NoDecode
# Decode only: python -I -X utf8 tools\windows\input-trace.py <run dir>
param(
    [string]$OutDir,
    [int]$IdleSeconds = 60,
    [int]$DisplayOffSeconds = 60,
    [int]$StandbySeconds = 120,
    [switch]$Standby,
    [switch]$Manual,
    [switch]$NoDecode
)
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path

$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole(
    [Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    $pass = @()
    foreach ($k in $PSBoundParameters.Keys) {
        $v = $PSBoundParameters[$k]
        if ($v -is [switch]) { if ($v) { $pass += "-$k" } } else { $pass += "-$k"; $pass += "$v" }
    }
    if ($Manual) {
        # Manual steps need a visible console for the prompts.
        $argStr = ($pass | ForEach-Object { if ($_ -match '^-\w+$') { $_ } else { "'" + ($_ -replace "'", "''") + "'" } }) -join ' '
        $cmd = "Set-Location '$repo'; & '$PSCommandPath' $argStr; Read-Host 'Enter to close'"
        Start-Process powershell -Verb RunAs -Wait -ArgumentList '-NoProfile', '-ExecutionPolicy', 'Bypass', '-Command', $cmd
        exit 0
    }
    Push-Location $repo
    & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot 'run-elevated.ps1') $PSCommandPath @pass
    $rc = $LASTEXITCODE; Pop-Location; exit $rc
}

if ($OutDir) { $Out = $OutDir } else { $Out = Join-Path $repo ("private\windows\input-trace\" + (Get-Date -Format 'yyyyMMdd-HHmmss')) }
New-Item -ItemType Directory -Force $Out, (Join-Path $Out 'features'), (Join-Path $Out 'procs') | Out-Null
$Out = (Resolve-Path $Out).Path
$log = Join-Path $Out 'run.log'
function Log([string]$m) { $line = "{0:HH:mm:ss.fff} {1}" -f (Get-Date), $m; $line; Add-Content -Encoding utf8 $log $line }

Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class InTrace {
  [DllImport("hid.dll")] static extern void HidD_GetHidGuid(out Guid g);
  [DllImport("hid.dll")] static extern bool HidD_GetPreparsedData(IntPtr h, out IntPtr p);
  [DllImport("hid.dll")] static extern bool HidD_FreePreparsedData(IntPtr p);
  [DllImport("hid.dll")] static extern int HidP_GetCaps(IntPtr p, byte[] c);
  [DllImport("hid.dll")] static extern int HidP_GetValueCaps(int t, byte[] c, ref ushort n, IntPtr p);
  [DllImport("hid.dll")] static extern int HidP_GetButtonCaps(int t, byte[] c, ref ushort n, IntPtr p);
  [DllImport("hid.dll", SetLastError=true)] static extern bool HidD_GetFeature(IntPtr h, byte[] b, int len);
  [DllImport("setupapi.dll", CharSet=CharSet.Unicode)] static extern IntPtr SetupDiGetClassDevsW(ref Guid g, IntPtr e, IntPtr w, int f);
  [DllImport("setupapi.dll")] static extern bool SetupDiEnumDeviceInterfaces(IntPtr s, IntPtr d, ref Guid g, int i, byte[] ifd);
  [DllImport("setupapi.dll", CharSet=CharSet.Unicode)] static extern bool SetupDiGetDeviceInterfaceDetailW(IntPtr s, byte[] ifd, IntPtr det, int sz, out int req, IntPtr di);
  [DllImport("setupapi.dll")] static extern bool SetupDiDestroyDeviceInfoList(IntPtr s);
  [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern IntPtr CreateFileW(string n, uint a, uint s, IntPtr sa, uint c, uint f, IntPtr t);
  [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);

  [DllImport("user32.dll")] static extern IntPtr SendMessageTimeoutW(IntPtr h, uint m, IntPtr w, IntPtr l, uint f, uint t, out IntPtr r);
  [DllImport("user32.dll")] static extern uint SendInput(uint n, INPUT[] i, int cb);
  [DllImport("kernel32.dll")] public static extern uint SetThreadExecutionState(uint f);
  [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern IntPtr CreateWaitableTimerW(IntPtr a, bool manual, string n);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool SetWaitableTimer(IntPtr h, ref long due, int period, IntPtr f, IntPtr a, bool resume);
  [DllImport("kernel32.dll")] static extern uint WaitForSingleObject(IntPtr h, uint ms);
  [DllImport("powrprof.dll", SetLastError=true)] static extern bool SetSuspendState(bool hib, bool force, bool wakeDisabled);

  [StructLayout(LayoutKind.Sequential)] struct MOUSEINPUT { public int dx, dy; public uint data, flags, time; public IntPtr extra; }
  [StructLayout(LayoutKind.Sequential)] struct INPUT { public uint type; public MOUSEINPUT mi; }  // 40 bytes on x64 = sizeof(INPUT)

  public static List<string> Paths() {
    Guid g; HidD_GetHidGuid(out g); var r = new List<string>();
    IntPtr s = SetupDiGetClassDevsW(ref g, IntPtr.Zero, IntPtr.Zero, 0x12);
    for (int i = 0; ; i++) { byte[] ifd = new byte[32]; BitConverter.GetBytes(32).CopyTo(ifd, 0);
      if (!SetupDiEnumDeviceInterfaces(s, IntPtr.Zero, ref g, i, ifd)) break;
      int req; SetupDiGetDeviceInterfaceDetailW(s, ifd, IntPtr.Zero, 0, out req, IntPtr.Zero);
      IntPtr det = Marshal.AllocHGlobal(req); Marshal.WriteInt32(det, 8);
      if (SetupDiGetDeviceInterfaceDetailW(s, ifd, det, req, out req, IntPtr.Zero)) r.Add(Marshal.PtrToStringUni(det + 4));
      Marshal.FreeHGlobal(det); }
    SetupDiDestroyDeviceInfoList(s);
    return r; }

  // Read-only: every feature report id of the collection, full length, hex.
  // Skipped top-level collections: pen bonding (FFF4), CFU firmware update (FF0B:0101), pen id (FF0F:0051), FFA1.
  // A report that failed once (most often error 121, a ~8 s timeout: write-only PTP features) is not asked again.
  static HashSet<string> failed = new HashSet<string>();
  public static string Features(string path) {
    var sb = new StringBuilder();
    IntPtr h = CreateFileW(path, 0, 3, IntPtr.Zero, 3, 0, IntPtr.Zero);
    if (h == new IntPtr(-1)) return "  open failed " + Marshal.GetLastWin32Error() + "\n";
    try {
      IntPtr p; if (!HidD_GetPreparsedData(h, out p)) return "";
      byte[] c = new byte[64]; HidP_GetCaps(p, c); int flen = BitConverter.ToUInt16(c, 8);
      uint tlc = ((uint)BitConverter.ToUInt16(c, 2) << 16) | BitConverter.ToUInt16(c, 0);
      if ((tlc >> 16) == 0xFFF4 || tlc == 0xFF0B0101 || tlc == 0xFF0F0051 || (tlc >> 16) == 0xFFA1) { HidD_FreePreparsedData(p); return ""; }
      var ids = new SortedSet<int>();
      ushort nb = BitConverter.ToUInt16(c, 58), nv = BitConverter.ToUInt16(c, 60);
      if (nb > 0) { byte[] b = new byte[72*nb]; HidP_GetButtonCaps(2, b, ref nb, p); for (int i = 0; i < nb; i++) ids.Add(b[72*i+2]); }
      if (nv > 0) { byte[] v = new byte[72*nv]; HidP_GetValueCaps(2, v, ref nv, p); for (int i = 0; i < nv; i++) ids.Add(v[72*i+2]); }
      HidD_FreePreparsedData(p);
      foreach (int id in ids) {
        string key = path + "|" + id;
        if (failed.Contains(key)) continue;
        byte[] buf = new byte[flen]; buf[0] = (byte)id;
        bool ok = HidD_GetFeature(h, buf, buf.Length);
        if (!ok) { failed.Add(key); continue; }
        int last = buf.Length; while (last > 1 && buf[last-1] == 0) last--;
        sb.AppendFormat("  feat {0} {1}\n", id, BitConverter.ToString(buf, 0, last).Replace("-", ""));
      }
    } finally { CloseHandle(h); }
    return sb.ToString(); }

  public static void Monitor(int state) {  // SC_MONITORPOWER: 2 off, -1 on
    IntPtr r; SendMessageTimeoutW(new IntPtr(0xFFFF), 0x0112, new IntPtr(0xF170), new IntPtr(state), 0x0002, 2000, out r); }
  public static void Nudge() {  // 1-px relative mouse move and back: turns the display on
    var i = new INPUT[2]; i[0].type = 0; i[0].mi.dx = 1; i[0].mi.flags = 1; i[1].type = 0; i[1].mi.dx = -1; i[1].mi.flags = 1;
    SendInput(2, i, Marshal.SizeOf(typeof(INPUT))); }
  public static IntPtr ArmWake(int seconds) {
    IntPtr t = CreateWaitableTimerW(IntPtr.Zero, true, null);
    long due = -10000000L * seconds;
    if (!SetWaitableTimer(t, ref due, 0, IntPtr.Zero, IntPtr.Zero, true)) throw new Exception("SetWaitableTimer " + Marshal.GetLastWin32Error());
    return t; }
  public static bool Wait(IntPtr t, int ms) { return WaitForSingleObject(t, (uint)ms) == 0; }
  public static string Sleep() { bool ok = SetSuspendState(false, false, false); return ok ? "ok" : "err" + Marshal.GetLastWin32Error(); }
}
'@

$featFilter = 'dev_51d0|target_kip|target_sam|hid_device_system_vhf'
# Pen-bonding, pen-id and firmware-update collections are skipped by their usage page inside Features().
$hidPaths = [InTrace]::Paths() | Where-Object { $_ -match $featFilter } | Sort-Object
function Snapshot([string]$name) {
    $sb = New-Object Text.StringBuilder
    foreach ($p in $hidPaths) {
        $f = [InTrace]::Features($p)
        if ($f) { [void]$sb.AppendLine(($p -replace '(?i)(?<=[_&])[0-9a-f]{12}(?=[_&#])', '<addr>')); [void]$sb.Append($f) }
    }
    [IO.File]::WriteAllText((Join-Path $Out "features\$name.txt"), $sb.ToString())
    Get-Process | Select-Object Id, ProcessName | ConvertTo-Csv -NoTypeInformation | Set-Content -Encoding utf8 (Join-Path $Out "procs\$name.csv")
}
$markers = Join-Path $Out 'markers.csv'
Set-Content -Encoding utf8 $markers 'filetime,phase,event'
function Mark([string]$phase, [string]$ev) {
    Add-Content -Encoding utf8 $markers ("{0},{1},{2}" -f [DateTime]::UtcNow.ToFileTimeUtc(), $phase, $ev)
    Log "$phase $ev"
}
function Phase([string]$name, [scriptblock]$body) {
    Mark $name 'start'; Snapshot "$name-start"
    & $body
    Snapshot "$name-end"; Mark $name 'end'
}

$session = 'sp9inputtrace'
$etl = Join-Path $Out 'input.etl'
$providers = @(
    '{47c779cd-4efd-49d7-9b10-9f16e5c25d06} 0xffffffff 0x5'           # HIDCLASS WPP (busiotools BluetoothStack.wprp)
    '{6465DA78-E7A0-4F39-B084-8F53C7C30DC6} 0xffffffffffffffff 0x5'   # Microsoft-Windows-Input-HIDCLASS
    '{937AD94E-CA8D-4B8E-8143-3FCE4ACCB8CB} 0xffffffff 0x5'           # IntelTHCBase
    '{2FEA7205-B0B1-41CA-8609-5A1D16F3132F} 0xffffffffffffffff 0x5'   # Microsoft-Surface-SurfaceHidMiniDriver
    '{8DDB06D0-A54B-4014-B119-5A09FB550D18} 0xffffffffffffffff 0x5'   # Microsoft-Surface-SurfaceSerialHubDriver
    '{69CD60E3-430F-4DA4-B1B3-E3BDAF945875} 0xffffffffffffffff 0x5'   # Microsoft-Surface-SurfaceHotPlug
    '{E13FF11E-E989-4838-A9FA-38A4D13914CF} 0xffffffffffffffff 0x5'   # Microsoft-Windows-GPIOButtons
)
$pf = Join-Path $Out 'providers.txt'
Set-Content -Encoding ascii $pf $providers

$wake = $null; $restore = $null
$t0 = Get-Date
try {
    logman stop $session -ets 2>$null | Out-Null
    logman start $session -ets -o $etl -pf $pf -bs 256 -nb 16 64 -mode sequential -max 512 | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "logman start failed ($LASTEXITCODE)" }
    Log "ETW session $session -> $etl"

    Phase 'idle1' { Start-Sleep -Seconds $IdleSeconds }

    if ($Standby) {
        # On this device turning the display off IS Modern Standby entry: SC_MONITORPOWER off enters it at once
        # (Kernel-Power 506) even with ES_SYSTEM_REQUIRED, and this process stays frozen until something wakes the
        # system. Every phase that turns the display off therefore arms a resume timer first.
        $sub = '238c9fa8-0aad-41ed-83f4-97be242c8f20'; $rtc = 'bd3b718a-0680-4d9d-8ab2-e1d2b4ac806d'
        $q = (powercfg /q scheme_current $sub $rtc) -join "`n"
        $hex = [regex]::Matches($q, '0x[0-9a-fA-F]{8}') | ForEach-Object { $_.Value }
        if ($hex.Count -lt 2) { throw "cannot read RTCWAKE ($q)" }
        $restore = @{ ac = [Convert]::ToInt32($hex[-2], 16); dc = [Convert]::ToInt32($hex[-1], 16) }
        Log ("wake timers before: AC {0} DC {1}" -f $restore.ac, $restore.dc)
        powercfg /setacvalueindex scheme_current $sub $rtc 1 | Out-Null
        powercfg /setdcvalueindex scheme_current $sub $rtc 1 | Out-Null
        powercfg /setactive scheme_current | Out-Null
        function WakeAndOn($timer, [int]$seconds, [string]$phase) {
            $woke = [InTrace]::Wait($timer, ($seconds + 120) * 1000)
            Mark $phase ("timer-" + $(if ($woke) { 'fired' } else { 'timeout' }))
            [InTrace]::Monitor(-1); [InTrace]::Nudge()
            Start-Sleep -Seconds 10
            [InTrace]::Nudge()
        }
        Phase 'displayoff' {
            $t = [InTrace]::ArmWake($DisplayOffSeconds)
            Mark 'displayoff' 'monitor-off'
            [InTrace]::Monitor(2)
            WakeAndOn $t $DisplayOffSeconds 'displayoff'
        }
        Phase 'idle2' { Start-Sleep -Seconds $IdleSeconds }
        Phase 'standby' {
            $t = [InTrace]::ArmWake($StandbySeconds)
            Mark 'standby' 'enter'
            $r = [InTrace]::Sleep()
            Log "SetSuspendState -> $r"
            if ($r -ne 'ok') { [InTrace]::Monitor(2); Log 'fallback: display off' }
            WakeAndOn $t $StandbySeconds 'standby'
        }
        Phase 'idle3' { Start-Sleep -Seconds $IdleSeconds }
    }

    if ($Manual) {
        $steps = @(
            @('coverdetach', 'Detach the Type Cover, wait 10 s, then press Enter'),
            @('coverattach', 'Attach the Type Cover again, wait 10 s, then press Enter'),
            @('backlight', 'Press the keyboard backlight key slowly, once per level, through a full cycle, then press Enter'),
            @('coverfold', 'Fold the cover over the screen, wait 15 s, open it again, then press Enter')
        )
        foreach ($s in $steps) { Phase $s[0] { Read-Host $s[1] | Out-Null } }
    }
} catch {
    Log "error: $($_.Exception.Message) at $($_.InvocationInfo.PositionMessage)"
} finally {
    if ($restore) {
        $sub = '238c9fa8-0aad-41ed-83f4-97be242c8f20'; $rtc = 'bd3b718a-0680-4d9d-8ab2-e1d2b4ac806d'
        powercfg /setacvalueindex scheme_current $sub $rtc $restore.ac | Out-Null
        powercfg /setdcvalueindex scheme_current $sub $rtc $restore.dc | Out-Null
        powercfg /setactive scheme_current | Out-Null
        Log ("wake timers restored: AC {0} DC {1}" -f $restore.ac, $restore.dc)
    }
    [void][InTrace]::SetThreadExecutionState([uint32]2147483648)
    logman stop $session -ets | Out-Null
    Log "ETW stopped"
}

# Kernel-Power standby entry/exit and display events during the run.
Get-WinEvent -FilterHashtable @{ LogName = 'System'; ProviderName = 'Microsoft-Windows-Kernel-Power'; StartTime = $t0 } -ErrorAction SilentlyContinue |
    Sort-Object TimeCreated | ForEach-Object { "{0:o},{1},{2}" -f $_.TimeCreated.ToUniversalTime(), $_.Id, (($_.Message -split "`r?`n")[0] -replace ',', ';') } |
    Set-Content -Encoding utf8 (Join-Path $Out 'kernel-power.csv')

if (-not $NoDecode) {
    $ErrorActionPreference = 'Continue'   # etldump writes its count to stderr
    $py = Join-Path $env:LOCALAPPDATA 'sp9-baseline\venv\Scripts\python.exe'
    if (-not (Test-Path $py)) { $py = 'python' }
    & $py -I -X utf8 (Join-Path $PSScriptRoot 'input-trace.py') $Out
}
Log "done: $Out"
