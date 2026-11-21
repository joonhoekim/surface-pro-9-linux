// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Joonhoe Kim
// Timed exit from Modern Standby, run as a Windows service (services keep running in standby; processes started by
// Task Scheduler and desktop apps are frozen until the system leaves standby, so their timers never act).
// Build: %WINDIR%\Microsoft.NET\Framework64\v4.0.30319\csc.exe /nologo /optimize+ /platform:x64 /r:System.ServiceProcess.dll /out:sp9wake.exe standby-wake.cs
// Install: sc create sp9wake binPath= "<exe> <seconds> <log> <nudge-task>" type= own start= demand; sc start sp9wake
// On start it arms a resume timer for <seconds>; when it fires and the system is still in standby (last Kernel-Power
// event 506), it tries in order, stopping at the first that produces 507:
//   request  PowerSetRequest(DisplayRequired, SystemRequired) + SetThreadExecutionState
//   nudge    schtasks /run <nudge-task> (a task that runs as the logged-on user: SendInput + SC_MONITORPOWER -1)
//   button   NtPowerInformation(PhysicalPowerButtonPress)
// Each step and the result go to <log>.
using System;
using System.Diagnostics;
using System.Diagnostics.Eventing.Reader;
using System.IO;
using System.Runtime.InteropServices;
using System.ServiceProcess;
using System.Threading;

class Sp9WakeService : ServiceBase
{
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    struct REASON_CONTEXT { public uint Version, Flags; public IntPtr Reason; }
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)] static extern IntPtr CreateWaitableTimerExW(IntPtr a, string n, uint flags, uint access);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool SetWaitableTimerEx(IntPtr h, ref long due, int period, IntPtr f, IntPtr a, ref REASON_CONTEXT ctx, uint tolerable);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool SetWaitableTimer(IntPtr h, ref long due, int period, IntPtr f, IntPtr a, bool resume);
    [DllImport("kernel32.dll")] static extern uint WaitForSingleObject(IntPtr h, uint ms);
    [DllImport("kernel32.dll", SetLastError = true)] static extern IntPtr PowerCreateRequest(ref REASON_CONTEXT ctx);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool PowerSetRequest(IntPtr h, int type);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool PowerClearRequest(IntPtr h, int type);
    [DllImport("kernel32.dll")] static extern uint SetThreadExecutionState(uint f);
    [DllImport("ntdll.dll")] static extern int NtPowerInformation(int level, ref byte inBuf, int inLen, IntPtr outBuf, int outLen);
    [DllImport("ntdll.dll")] static extern int RtlAdjustPrivilege(int priv, bool enable, bool thread, out bool was);

    int seconds; string log, nudgeTask; Thread worker; DateTime start;

    Sp9WakeService(string[] a)
    {
        ServiceName = "sp9wake";
        seconds = a.Length > 0 ? int.Parse(a[0]) : 600;
        log = a.Length > 1 ? a[1] : @"C:\ProgramData\sp9-standby-wake.log";
        nudgeTask = a.Length > 2 ? a[2] : "sp9-standby-nudge";
    }

    void Log(string m) { try { File.AppendAllText(log, DateTime.Now.ToString("o") + " " + m + Environment.NewLine); } catch { } }

    static REASON_CONTEXT Reason(string s) { REASON_CONTEXT c; c.Version = 0; c.Flags = 1; c.Reason = Marshal.StringToHGlobalUni(s); return c; }

    // Last Kernel-Power 506/507 since the service started: 506 = in standby.
    int LastStandbyEvent()
    {
        string q = "*[System[Provider[@Name='Microsoft-Windows-Kernel-Power'] and (EventID=506 or EventID=507) and TimeCreated[@SystemTime>='"
                   + start.ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ss.fffZ") + "']]]";
        int last = 0;
        using (var r = new EventLogReader(new EventLogQuery("System", PathType.LogName, q)))
            for (EventRecord e = r.ReadEvent(); e != null; e = r.ReadEvent()) { last = e.Id; e.Dispose(); }
        return last;
    }

    void Run()
    {
        try
        {
            IntPtr t = CreateWaitableTimerExW(IntPtr.Zero, null, 1, 0x1F0003);
            long due = -10000000L * seconds;
            REASON_CONTEXT c = Reason("sp9 standby trace timed wake");
            bool ok = SetWaitableTimerEx(t, ref due, 0, IntPtr.Zero, IntPtr.Zero, ref c, 0);
            if (!ok) ok = SetWaitableTimer(t, ref due, 0, IntPtr.Zero, IntPtr.Zero, true);
            Log("service: armed " + seconds + " s ok=" + ok);
            bool fired = WaitForSingleObject(t, (uint)((seconds + 3600) * 1000)) == 0;
            Log("service: timer fired=" + fired + " last=" + LastStandbyEvent());
            string exit = "none";
            if (LastStandbyEvent() != 506) exit = "already-awake";
            IntPtr req = IntPtr.Zero;
            if (exit == "none")
            {
                REASON_CONTEXT rc = Reason("sp9 standby trace timed wake");
                req = PowerCreateRequest(ref rc);
                bool d = PowerSetRequest(req, 0), s = PowerSetRequest(req, 1);
                uint st = SetThreadExecutionState(0x80000000 | 0x2 | 0x1);
                Log("service: request display=" + d + " system=" + s + " stes=0x" + st.ToString("X"));
                Thread.Sleep(8000);
                if (LastStandbyEvent() != 506) exit = "request";
            }
            if (exit == "none")
            {
                var p = Process.Start(new ProcessStartInfo("schtasks.exe", "/run /tn \"" + nudgeTask + "\"") { UseShellExecute = false, CreateNoWindow = true });
                p.WaitForExit(10000);
                Log("service: nudge task started rc=" + p.ExitCode);
                Thread.Sleep(10000);
                if (LastStandbyEvent() != 506) exit = "nudge";
            }
            if (exit == "none")
            {
                bool was; RtlAdjustPrivilege(19, true, false, out was);
                byte b = 1;
                int st = NtPowerInformation(90, ref b, 1, IntPtr.Zero, 0);
                Log("service: button status=0x" + st.ToString("X8"));
                Thread.Sleep(8000);
                if (LastStandbyEvent() != 506) exit = "button";
            }
            Log("wake: exit=" + exit + " last=" + LastStandbyEvent());
            Thread.Sleep(15000);
            if (req != IntPtr.Zero) { PowerClearRequest(req, 0); PowerClearRequest(req, 1); }
            SetThreadExecutionState(0x80000000);
        }
        catch (Exception e) { Log("service: error " + e); }
        finally { new Thread(() => { try { Stop(); } catch { } }).Start(); }
    }

    protected override void OnStart(string[] args)
    {
        if (args != null && args.Length > 0) { seconds = int.Parse(args[0]); if (args.Length > 1) log = args[1]; if (args.Length > 2) nudgeTask = args[2]; }
        start = DateTime.Now.AddSeconds(-5);
        worker = new Thread(Run) { IsBackground = true };
        worker.Start();
    }

    protected override void OnStop() { }

    static void Main(string[] a) { ServiceBase.Run(new Sp9WakeService(a)); }
}
