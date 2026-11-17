# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# Ask the Surface Integration Driver (MSHW0343, device interface {54f596c1-3b0d-40f2-97a4-fcbcfd253d3b}) for the
# battery data the Surface app reads, through the same IOCTL (0x225C00) and packet format as the app's BatteryDm.exe.
# Only the read commands are allowed here; the driver turns each into one SAM request (see docs/research/control/battery-charging.md).
# Needs admin (the IOCTL is admin-only): run through run-elevated.ps1.
#
#   run-elevated.ps1 tools\windows\surface-battery-query.ps1 [-Raw]
#
# Packet: [len, 0x03, 0x00 (command group), command, 0x00 (instance), payload...]. -Raw adds a hex dump of every reply.
# The serialization serial number in "general custom" is masked.
param([switch]$Raw)

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;
public static class SidBat {
    [DllImport("setupapi.dll", SetLastError=true)] static extern IntPtr SetupDiGetClassDevsW(ref Guid g, IntPtr e, IntPtr w, int f);
    [DllImport("setupapi.dll", SetLastError=true)] static extern bool SetupDiEnumDeviceInterfaces(IntPtr s, IntPtr d, ref Guid g, int i, byte[] ifd);
    [DllImport("setupapi.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern bool SetupDiGetDeviceInterfaceDetailW(IntPtr s, byte[] ifd, IntPtr det, int sz, out int req, IntPtr di);
    [DllImport("setupapi.dll")] static extern bool SetupDiDestroyDeviceInfoList(IntPtr s);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern SafeFileHandle CreateFileW(string n, uint a, uint s, IntPtr sa, uint c, uint f, IntPtr t);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool DeviceIoControl(SafeFileHandle h, uint code, byte[] inb, int inl, byte[] outb, int outl, out int ret, IntPtr ov);
    public static string Path(Guid g) {
        IntPtr s = SetupDiGetClassDevsW(ref g, IntPtr.Zero, IntPtr.Zero, 0x12);  // PRESENT | DEVICEINTERFACE
        try {
            byte[] ifd = new byte[IntPtr.Size == 8 ? 32 : 28];
            BitConverter.GetBytes(ifd.Length).CopyTo(ifd, 0);
            if (!SetupDiEnumDeviceInterfaces(s, IntPtr.Zero, ref g, 0, ifd)) return null;
            int req; SetupDiGetDeviceInterfaceDetailW(s, ifd, IntPtr.Zero, 0, out req, IntPtr.Zero);
            IntPtr det = Marshal.AllocHGlobal(req);
            try {
                Marshal.WriteInt32(det, IntPtr.Size == 8 ? 8 : 6);
                if (!SetupDiGetDeviceInterfaceDetailW(s, ifd, det, req, out req, IntPtr.Zero)) return null;
                return Marshal.PtrToStringUni(det + 4);
            } finally { Marshal.FreeHGlobal(det); }
        } finally { SetupDiDestroyDeviceInfoList(s); }
    }
    public static byte[] Send(string path, byte[] pkt, out int err) {
        err = 0;
        using (var h = CreateFileW(path, 0xC0000000, 3, IntPtr.Zero, 3, 0, IntPtr.Zero)) {
            if (h.IsInvalid) { err = Marshal.GetLastWin32Error(); return null; }
            byte[] o = new byte[512]; int n;
            if (!DeviceIoControl(h, 0x225C00, pkt, pkt.Length, o, o.Length, out n, IntPtr.Zero)) { err = Marshal.GetLastWin32Error(); return null; }
            byte[] r = new byte[n]; Array.Copy(o, r, n); return r;
        }
    }
}
'@

$path = [SidBat]::Path([Guid]'54f596c1-3b0d-40f2-97a4-fcbcfd253d3b')
if (-not $path) { 'SID device interface not found'; exit 1 }

# Field layouts from BatteryDm.exe metadata (Pack=1). Type: u1 u2 i2 u4 or bN (byte array of N).
$layouts = @{
    'GeneralStandard' = 'batteryPhysicalDetect:u1 batteryStatus:u2 operationStatus:u4 chargingStatus:u4 gaugingStatus:u4 safetyStatus:u4 pfStatus:u4 relativeStateOfCharge:u2 stateOfHealth:u2 chargingVoltage_mV:u2 chargingCurrent_mA:u2 voltage_mV:u2 current_mA:i2 temperatureRaw_dC:u2 temperatureVtsMax_dC:u2 cellVoltage1_mV:u2 cellVoltage2_mV:u2 cellVoltage3_mV:u2 cellVoltage4_mV:u2 maxTurboPwr_cW:u2 susTurboPwr_cW:u2 turboRhfEffective_mOhm:u2 turboVload:u2'
    'GeneralCustom'   = 'chargeMode:u1 batteryChargeLimitEnable:u1 batteryDisable:u1 authenticationStatus:u1 protectionStatus:u4 serializationSerialNumber:b19 actualChargingVoltage_mV:u2 actualChargingCurrent_mA:u2 dischargeRateEwmaLvl:u1 temperatureMax_dC:u2 temperatureAvg_dC:u2 userChargeMode:u1 cmdRevision:u1 userChargeModeNoLimitRestriction:u1'
    'BatteryProtection' = 'bpmDisable:u1 bpmStatus:u1 bpmStartDate:u4 bpmTotalActive_sec:u4 bpmTodayElapsedTime_sec:u4 bpmHvtTime_sec:u4 bpmWindowActiveIndex:u1 bpmExitRsocThreshold_pct:u1 bpmTotalEntryEvents:u4 bpmKioskModeStartTime_sec:u4 bpmKioskModeTotalEngaged_sec:u4 bpmCpAccumulatedTime_sec:u4'
    'CutTheTop'       = 'cttDisable:u1 cttStartDate:u4 cttReduction_mv:u2 cttLastUpdate_sec:u4 eqvTimeAtHvt_sec:u4 eqvTimeAtHvtInBpm_sec:u4'
    'ProtectionPolicy' = 'policy:u1'
    'ChargeLimitStatus' = 'status:u1'
    'ChargeLimitCountdown' = 'seconds:u4'
    'PredictiveCharge' = 'pauseTimeoutTimeUtcSec:u4 lastPredictionUpdateTimeUtcSec:u4 featureSupported:u1 featureEnabled:u1 state:u1 validPredictionCount:u1 currentHourlyPredictions:b24'
    'PsuGeneralInfo'  = 'InadequatePsuPowerMw:u4 RecommendedPsuPowerMw:u4 NumberOfPowerPorts:u1 TotalPortConsumerPowerMw:u4 ActivePsuMaxPowerCapabilityMw:u4 ActivePsuVoltageCapabilityMv:u2 ActivePsuMaxCurrentCapabilityMa:i2 ActivePsuConnectionType:u1 ActivePsuIsDock:u1 ActivePsuCableCurrentCapabilityMa:u4'
    'UserChargeMode'  = 'ModesSupported:u4 CurrentMode:u1 PreviousMode:u1 ModeSettings:b24'
}
# Read-only commands only (BatteryDevice.BatteryCommand). 8 ForceBclDisengage, 9 Authenticate, 10 Shutdown,
# 12/13 PCC set, 15 SetUserChargeMode change state and are deliberately absent.
$cmds = @((1, 'GeneralStandard'), (2, 'GeneralCustom'), (3, 'BatteryProtection'), (4, 'CutTheTop'), (5, 'ProtectionPolicy'),
          (6, 'ChargeLimitStatus'), (7, 'ChargeLimitCountdown'), (11, 'PredictiveCharge'), (14, 'PsuGeneralInfo'), (16, 'UserChargeMode'))

function Decode([byte[]]$b, [string]$layout) {
    $o = 0
    foreach ($f in $layout -split ' ') {
        $n, $t = $f -split ':'
        if ($o -ge $b.Length) { "  {0} = (reply too short)" -f $n; break }
        switch -regex ($t) {
            '^u1$' { $v = $b[$o]; $o += 1 }
            '^u2$' { $v = [BitConverter]::ToUInt16($b, $o); $o += 2 }
            '^i2$' { $v = [BitConverter]::ToInt16($b, $o); $o += 2 }
            '^u4$' { if ($o + 4 -gt $b.Length) { $v = "(only $($b.Length - $o) bytes)"; $o = $b.Length } else { $v = [BitConverter]::ToUInt32($b, $o); $o += 4 } }
            '^b(\d+)$' {
                $len = [int]$Matches[1]; $end = [Math]::Min($o + $len, $b.Length) - 1
                $v = ($b[$o..$end] | ForEach-Object { '{0:x2}' -f $_ }) -join ' '
                if ($n -match 'Serial') { $v = '(masked)' }
                $o += $len
            }
        }
        if ($v -is [string]) { "  {0} = {1}" -f $n, $v } else { "  {0} = {1} (0x{1:x})" -f $n, $v }
    }
}

foreach ($e in $cmds) {
    $c, $name = $e
    $pkt = [byte[]](5, 3, 0, $c, 0)
    $err = 0
    $r = [SidBat]::Send($path, $pkt, [ref]$err)
    "## {0} {1}" -f $c, $name
    if ($null -eq $r) { "  error {0}" -f $err; continue }
    if ($Raw -and $name -ne 'GeneralCustom') { "  raw[{0}]: {1}" -f $r.Length, (($r | ForEach-Object { '{0:x2}' -f $_ }) -join ' ') }
    Decode $r $layouts[$name]
}
