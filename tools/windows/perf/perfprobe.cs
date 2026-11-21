// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Joonhoe Kim
// perfprobe: power-mode overlay, MSR power/thermal state and CPU load for tools/windows/perf-modes.ps1.
// C# 5 (.NET Framework csc, see perf-modes.ps1). MSRs are read through LibreHardwareMonitorLib + PawnIO (read only).
// The only write is "overlay set", the Windows power-mode overlay (what Settings > Power > Power mode changes).
//
//   perfprobe overlay get                  effective and actual overlay GUID
//   perfprobe overlay set <guid>           PowerSetActiveOverlayScheme
//   perfprobe msr                          one-shot decode of the power/thermal MSRs (needs admin)
//   perfprobe sample <dir> <ms> [sec]      <dir>\msr.csv every <ms> until <dir>\stop exists (needs admin)
//   perfprobe stress <sec> [threads] [--default-qos]
//                                          all-core FP loop. The process opts out of power throttling (EcoQoS)
//                                          unless --default-qos, so the load does not depend on window focus.
//
// LibreHardwareMonitorLib.dll is looked up in %LHM_DIR% or the winget package folder.
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;

static class Clock {
    static readonly double t0 = (DateTime.UtcNow - new DateTime(1970, 1, 1)).TotalSeconds;
    static readonly Stopwatch sw = Stopwatch.StartNew();
    public static double Now() { return t0 + sw.Elapsed.TotalSeconds; }
}

static class Native {
    [DllImport("powrprof.dll")] public static extern uint PowerGetEffectiveOverlayScheme(out Guid g);
    [DllImport("powrprof.dll")] public static extern uint PowerGetActualOverlayScheme(out Guid g);
    [DllImport("powrprof.dll")] public static extern uint PowerSetActiveOverlayScheme(Guid g);

    [StructLayout(LayoutKind.Sequential)] public struct PROCESS_POWER_THROTTLING_STATE { public uint Version, ControlMask, StateMask; }
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool SetProcessInformation(IntPtr h, int cls, ref PROCESS_POWER_THROTTLING_STATE s, int size);
    [DllImport("kernel32.dll")] public static extern IntPtr GetCurrentProcess();
}

// Late-bound access to LibreHardwareMonitorLib so this file compiles without referencing it.
class Msr {
    object msr; MethodInfo read64, readAff; Type affType;
    public Msr() {
        string dir = Environment.GetEnvironmentVariable("LHM_DIR");
        if (string.IsNullOrEmpty(dir)) {
            string pk = Path.Combine(Environment.GetEnvironmentVariable("LOCALAPPDATA"), @"Microsoft\WinGet\Packages");
            dir = Directory.GetDirectories(pk, "LibreHardwareMonitor.LibreHardwareMonitor*").FirstOrDefault();
        }
        if (dir == null) throw new Exception("LibreHardwareMonitorLib not found (winget install LibreHardwareMonitor.LibreHardwareMonitor)");
        AppDomain.CurrentDomain.AssemblyResolve += (s, e) => {
            string f = Path.Combine(dir, new AssemblyName(e.Name).Name + ".dll");
            return File.Exists(f) ? Assembly.LoadFrom(f) : null;
        };
        Assembly a = Assembly.LoadFrom(Path.Combine(dir, "LibreHardwareMonitorLib.dll"));
        Type t = a.GetType("LibreHardwareMonitor.PawnIo.IntelMsr", true);
        affType = a.GetType("LibreHardwareMonitor.Hardware.GroupAffinity", true);
        msr = Activator.CreateInstance(t);
        read64 = t.GetMethod("ReadMsr", new[] { typeof(uint), typeof(ulong).MakeByRefType() });
        readAff = t.GetMethod("ReadMsr", new[] { typeof(uint), typeof(uint).MakeByRefType(), typeof(uint).MakeByRefType(), affType });
    }
    public bool Read(uint idx, out ulong v) {
        object[] p = { idx, 0UL };
        bool ok = (bool)read64.Invoke(msr, p);
        v = (ulong)p[1]; return ok;
    }
    public bool ReadOn(uint idx, int cpu, out ulong v) {
        object aff = Activator.CreateInstance(affType, (ushort)0, 1UL << cpu);
        object[] p = { idx, 0u, 0u, aff };
        bool ok = (bool)readAff.Invoke(msr, p);
        v = ((ulong)(uint)p[2] << 32) | (uint)p[1]; return ok;
    }
}

static class Decode {
    public static double PowerUnit = 0.125, TimeUnit = 1.0 / 1024, EnergyUnit = 1.0 / 16384;
    public static void Units(Msr m) {
        ulong u;
        if (m.Read(0x606, out u)) {
            PowerUnit = 1.0 / (1 << (int)(u & 0xf));
            EnergyUnit = 1.0 / (1 << (int)((u >> 8) & 0x1f));
            TimeUnit = 1.0 / (1 << (int)((u >> 16) & 0xf));
        }
    }
    // RAPL limit field: power[14:0], enable[15], clamp[16], time Y[21:17] Z[23:22]
    public static double W(ulong f) { return (f & 0x7fff) * PowerUnit; }
    public static double Tau(ulong f) {
        int y = (int)((f >> 17) & 0x1f), z = (int)((f >> 22) & 3);
        return Math.Pow(2, y) * (1 + z / 4.0) * TimeUnit;
    }
    public static int En(ulong f) { return (int)((f >> 15) & 1); }
}

static class Program {
    static readonly CultureInfo Inv = CultureInfo.InvariantCulture;
    static string F(double d) { return d.ToString("0.###", Inv); }

    static int Main(string[] a) {
        Thread.CurrentThread.CurrentCulture = Inv;
        if (a.Length == 0) { Console.Error.WriteLine("usage: perfprobe overlay get|set <guid> | msr | sample <dir> <ms> [sec] | stress <sec> [threads] [--default-qos]"); return 1; }
        switch (a[0]) {
            case "overlay": return Overlay(a);
            case "msr": return MsrOnce();
            case "sample": return Sample(a[1], int.Parse(a[2]), a.Length > 3 ? double.Parse(a[3], Inv) : 0);
            case "stress": return Stress(int.Parse(a[1]), a.Length > 2 && !a[2].StartsWith("--") ? int.Parse(a[2]) : Environment.ProcessorCount, Array.IndexOf(a, "--default-qos") > 0);
        }
        return 1;
    }

    static int Overlay(string[] a) {
        Guid g;
        if (a.Length > 2 && a[1] == "set") {
            uint r = Native.PowerSetActiveOverlayScheme(new Guid(a[2]));
            if (r != 0) { Console.Error.WriteLine("PowerSetActiveOverlayScheme: " + r); return (int)r; }
        }
        uint e = Native.PowerGetEffectiveOverlayScheme(out g);
        Console.WriteLine("effective " + (e == 0 ? g.ToString() : "error " + e));
        e = Native.PowerGetActualOverlayScheme(out g);
        Console.WriteLine("actual " + (e == 0 ? g.ToString() : "error " + e));
        return 0;
    }

    // Package-scope MSRs (name, index). Per-CPU ones are read separately.
    static readonly uint[] PkgMsrs = { 0x606, 0x610, 0x614, 0x615, 0x618, 0x638, 0x639, 0x640, 0x641, 0x65C, 0x601, 0x1A2, 0x1B1, 0x64F, 0x6B0, 0x6B1, 0x772, 0x770, 0x611, 0x619, 0x64D };

    static int MsrOnce() {
        var m = new Msr(); Decode.Units(m);
        Console.WriteLine("units power {0} W, energy {1} J, time {2} s", F(Decode.PowerUnit), Decode.EnergyUnit.ToString("G4", Inv), F(Decode.TimeUnit));
        foreach (uint i in PkgMsrs) {
            ulong v; bool ok = m.Read(i, out v);
            Console.WriteLine("0x{0:X3} {1}", i, ok ? "0x" + v.ToString("X16") + Explain(i, v) : "unreadable");
        }
        for (int c = 0; c < Environment.ProcessorCount; c++) {
            ulong req, cap, ps;
            bool o1 = m.ReadOn(0x774, c, out req), o2 = m.ReadOn(0x771, c, out cap), o3 = m.ReadOn(0x198, c, out ps);
            Console.WriteLine("cpu{0,-2} HWP_REQUEST {1} CAP {2} PERF_STATUS ratio {3}", c,
                o1 ? string.Format("min {0} max {1} des {2} epp {3} pkgctl {4}", req & 0xff, (req >> 8) & 0xff, (req >> 16) & 0xff, (req >> 24) & 0xff, (req >> 42) & 1) : "-",
                o2 ? string.Format("highest {0} guar {1} eff {2} lowest {3}", cap & 0xff, (cap >> 8) & 0xff, (cap >> 16) & 0xff, (cap >> 24) & 0xff) : "-",
                o3 ? ((ps >> 8) & 0xff).ToString() : "-");
        }
        return 0;
    }

    static string Explain(uint i, ulong v) {
        switch (i) {
            case 0x610:
                return string.Format("  PL1 {0} W en {1} clamp {2} tau {3} s | PL2 {4} W en {5} clamp {6} tau {7} s | lock {8}",
                    F(Decode.W(v)), Decode.En(v), (v >> 16) & 1, F(Decode.Tau(v)), F(Decode.W(v >> 32)), Decode.En(v >> 32), (v >> 48) & 1, F(Decode.Tau(v >> 32)), v >> 63);
            case 0x638: case 0x640:
                return string.Format("  limit {0} W en {1} tau {2} s lock {3}", F(Decode.W(v)), Decode.En(v), F(Decode.Tau(v)), (v >> 31) & 1);
            case 0x65C: // PSys: PL1 [16:0] en 17 clamp 18 time 23:19 ; PL2 [48:32] en 49
                return string.Format("  PSys PL1 {0} W en {1} | PL2 {2} W en {3} | lock {4}",
                    F((v & 0x1ffff) * Decode.PowerUnit), (v >> 17) & 1, F(((v >> 32) & 0x1ffff) * Decode.PowerUnit), (v >> 49) & 1, v >> 63);
            case 0x614: return string.Format("  TDP {0} W", F(Decode.W(v)));
            case 0x601: return string.Format("  PL4/current limit {0} A lock {1}", F((v & 0x1fff) * 0.125), v >> 31);
            case 0x1A2: return string.Format("  TjMax {0} C offset {1}", (v >> 16) & 0xff, (v >> 24) & 0x3f);
            case 0x1B1: return string.Format("  pkg digital readout {0} below TjMax", (v >> 16) & 0x7f);
            case 0x772: return string.Format("  HWP_REQUEST_PKG min {0} max {1} epp {2}", v & 0xff, (v >> 8) & 0xff, (v >> 24) & 0xff);
            case 0x64F: return "  CORE_PERF_LIMIT_REASONS status " + (v & 0xffff).ToString("X4") + " log " + ((v >> 16) & 0xffff).ToString("X4");
        }
        return "";
    }

    // CSV columns: t, PL1/PL2 (0x610), PSys PL1/PL2 (0x65C), PL4 (0x601), TCC offset, package temp, package power (0x611 delta),
    // PP0 power (0x639), PSys power (0x64D, if any), limit reasons, per-CPU ratio (0x198) and EPP (0x774).
    static int Sample(string dir, int ms, double sec) {
        Directory.CreateDirectory(dir);
        var m = new Msr(); Decode.Units(m);
        int n = Environment.ProcessorCount;
        string stop = Path.Combine(dir, "stop");
        using (var w = new StreamWriter(Path.Combine(dir, "msr.csv"), false, new UTF8Encoding(false))) {
            var h = new StringBuilder("t,msr610,pl1_w,pl1_en,pl1_tau_s,pl2_w,pl2_en,pl2_tau_s,msr65c,psys_pl1_w,psys_pl2_w,pl4_a,tcc_offset,tjmax,pkg_temp_c,pkg_w,pp0_w,psys_w,limit_reasons,gt_limit_reasons");
            for (int c = 0; c < n; c++) h.Append(",ratio" + c);
            for (int c = 0; c < n; c++) h.Append(",epp" + c);
            w.WriteLine(h);
            ulong e0, p0, s0; m.Read(0x611, out e0); m.Read(0x639, out p0); m.Read(0x64D, out s0);
            double t0 = Clock.Now(), tStart = t0;
            while (!File.Exists(stop) && (sec <= 0 || Clock.Now() - tStart < sec)) {
                Thread.Sleep(ms);
                double t = Clock.Now();
                ulong v610, v65c, v601, v1a2, v1b1, e, p, s, lr, glr;
                m.Read(0x610, out v610); m.Read(0x65C, out v65c); m.Read(0x601, out v601); m.Read(0x1A2, out v1a2); m.Read(0x1B1, out v1b1);
                m.Read(0x611, out e); m.Read(0x639, out p); bool hasPsys = m.Read(0x64D, out s); m.Read(0x64F, out lr); m.Read(0x6B0, out glr);
                double dt = t - t0; t0 = t;
                double pkg = (uint)((uint)e - (uint)e0) * Decode.EnergyUnit / dt, pp0 = (uint)((uint)p - (uint)p0) * Decode.EnergyUnit / dt;
                double psys = hasPsys ? (uint)((uint)s - (uint)s0) * Decode.EnergyUnit / dt : double.NaN;
                e0 = e; p0 = p; s0 = s;
                int tj = (int)((v1a2 >> 16) & 0xff);
                var r = new StringBuilder();
                r.Append(t.ToString("F3", Inv));
                r.AppendFormat(Inv, ",{0:X16},{1},{2},{3},{4},{5},{6}", v610, F(Decode.W(v610)), Decode.En(v610), F(Decode.Tau(v610)), F(Decode.W(v610 >> 32)), Decode.En(v610 >> 32), F(Decode.Tau(v610 >> 32)));
                r.AppendFormat(Inv, ",{0:X16},{1},{2},{3}", v65c, F((v65c & 0x1ffff) * Decode.PowerUnit), F(((v65c >> 32) & 0x1ffff) * Decode.PowerUnit), F((v601 & 0x1fff) * 0.125));
                r.AppendFormat(Inv, ",{0},{1},{2},{3},{4},{5},{6:X4},{7:X4}", (v1a2 >> 24) & 0x3f, tj, tj - (int)((v1b1 >> 16) & 0x7f), F(pkg), F(pp0), hasPsys ? F(psys) : "", lr & 0xffff, glr & 0xffff);
                var epp = new StringBuilder();
                for (int c = 0; c < n; c++) {
                    ulong ps, req;
                    r.Append(','); if (m.ReadOn(0x198, c, out ps)) r.Append((ps >> 8) & 0xff);
                    epp.Append(','); if (m.ReadOn(0x774, c, out req)) epp.Append((req >> 24) & 0xff);
                }
                r.Append(epp);
                w.WriteLine(r); w.Flush();
            }
        }
        return 0;
    }

    static int Stress(int sec, int threads, bool defaultQos) {
        if (!defaultQos) {
            var s = new Native.PROCESS_POWER_THROTTLING_STATE { Version = 1, ControlMask = 1, StateMask = 0 };
            if (!Native.SetProcessInformation(Native.GetCurrentProcess(), 4, ref s, Marshal.SizeOf(s)))
                Console.Error.WriteLine("SetProcessInformation(ProcessPowerThrottling) failed: " + Marshal.GetLastWin32Error());
        }
        var end = DateTime.UtcNow.AddSeconds(sec); var ts = new List<Thread>();
        Console.WriteLine("stress start {0} threads {1} s qos {2} at {3}", threads, sec, defaultQos ? "default" : "high", Clock.Now().ToString("F3", Inv));
        for (int i = 0; i < threads; i++) {
            var t = new Thread(() => { double x = 1.0001; while (DateTime.UtcNow < end) { for (int k = 0; k < 100000; k++) x = x * 1.0000001 + 1e-9; } if (x == 0) Console.Write(""); });
            t.IsBackground = true; t.Start(); ts.Add(t);
        }
        foreach (var t in ts) t.Join();
        Console.WriteLine("stress end {0}", Clock.Now().ToString("F3", Inv));
        return 0;
    }
}
