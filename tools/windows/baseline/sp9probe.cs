// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Joonhoe Kim
// sp9probe: Windows-side capture helper for the baseline kit. C# 5 (built by the .NET Framework csc, see setup.ps1).
// Read-only to the system except "volume set", which baseline.ps1 uses to set and then restore one endpoint's volume.
//
//   sp9probe pointer <outdir>   window that logs WM_POINTER pen/touch (incl. raw HID values via GetRawPointerDeviceData),
//                               Raw Input HID reports (digitizer, stylus, keyboard, consumer, system control) and platform events
//   sp9probe events <outdir>    same logging without the drawing window (invisible top-level window)
//   (either mode: --kbd also registers keyboards; key codes are kept only for the Slim Pen 2 BLE keyboard = tail button)
//   sp9probe devices            pointer devices and their HID properties (logical/physical ranges)
//   sp9probe display            displays, current mode, every available mode
//   sp9probe stress <sec> [threads]
//   sp9probe volume list | get <name> | set <name> <0..1>
//
// Logs are CSV with t = Unix time in seconds (shared clock with the other backends). Logging stops when <outdir>\stop appears.
// The window shows <outdir>\step.txt and writes <outdir>\next when Enter or the button is pressed.
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Drawing;
using System.Globalization;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;
using System.Windows.Forms;

static class Clock {
    static readonly double t0 = (DateTime.UtcNow - new DateTime(1970, 1, 1)).TotalSeconds;
    static readonly Stopwatch sw = Stopwatch.StartNew();
    public static string Now() { return (t0 + sw.Elapsed.TotalSeconds).ToString("F4", CultureInfo.InvariantCulture); }
}

class Csv : IDisposable {
    StreamWriter w;
    public Csv(string path, string header) { w = new StreamWriter(path, false, new UTF8Encoding(false)); w.WriteLine(header); }
    public void Row(params object[] f) {
        var sb = new StringBuilder(Clock.Now());
        foreach (var o in f) { sb.Append(','); sb.Append(Convert.ToString(o, CultureInfo.InvariantCulture)); }
        lock (this) { if (w != null) w.WriteLine(sb.ToString()); }
    }
    public void Flush() { lock (this) { if (w != null) w.Flush(); } }
    public void Dispose() { lock (this) { if (w != null) { w.Flush(); w.Dispose(); w = null; } } }
}

static class N {
    // ---- pointer ----
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
    [StructLayout(LayoutKind.Sequential)] public struct POINTER_INFO {
        public int pointerType; public uint pointerId, frameId; public uint pointerFlags;
        public IntPtr sourceDevice, hwndTarget;
        public POINT ptPixelLocation, ptHimetricLocation, ptPixelLocationRaw, ptHimetricLocationRaw;
        public uint dwTime, historyCount; public int InputData; public uint dwKeyStates; public ulong PerformanceCount; public int ButtonChangeType;
    }
    [StructLayout(LayoutKind.Sequential)] public struct POINTER_PEN_INFO {
        public POINTER_INFO pointerInfo; public uint penFlags, penMask, pressure, rotation; public int tiltX, tiltY;
    }
    [StructLayout(LayoutKind.Sequential)] public struct POINTER_TOUCH_INFO {
        public POINTER_INFO pointerInfo; public uint touchFlags, touchMask; public RECT rcContact, rcContactRaw; public uint orientation, pressure;
    }
    [StructLayout(LayoutKind.Sequential)] public struct POINTER_DEVICE_PROPERTY {
        public int logicalMin, logicalMax, physicalMin, physicalMax; public uint unit, unitExponent; public ushort usagePageId, usageId;
    }
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)] public struct POINTER_DEVICE_INFO {
        public uint displayOrientation; public IntPtr device; public int pointerDeviceType; public IntPtr monitor;
        public uint startingCursorId; public ushort maxActiveContacts;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 520)] public string productString;
    }
    [DllImport("user32.dll")] public static extern bool GetPointerType(uint id, out int type);
    [DllImport("user32.dll")] public static extern bool GetPointerPenInfoHistory(uint id, ref uint n, [Out] POINTER_PEN_INFO[] a);
    [DllImport("user32.dll")] public static extern bool GetPointerTouchInfo(uint id, out POINTER_TOUCH_INFO t);
    [DllImport("user32.dll")] public static extern bool GetPointerInfo(uint id, out POINTER_INFO t);
    [DllImport("user32.dll")] public static extern bool GetPointerDevices(ref uint n, [Out] POINTER_DEVICE_INFO[] a);
    [DllImport("user32.dll")] public static extern bool GetPointerDeviceProperties(IntPtr dev, ref uint n, [Out] POINTER_DEVICE_PROPERTY[] a);
    [DllImport("user32.dll")] public static extern bool GetRawPointerDeviceData(uint id, uint hist, uint nprop, [In] POINTER_DEVICE_PROPERTY[] p, [Out] int[] v);
    [DllImport("user32.dll")] public static extern bool GetPointerDeviceRects(IntPtr dev, out RECT dev_r, out RECT disp_r);

    // ---- raw input ----
    [StructLayout(LayoutKind.Sequential)] public struct RAWINPUTDEVICE { public ushort usUsagePage, usUsage; public uint dwFlags; public IntPtr hwndTarget; }
    [StructLayout(LayoutKind.Sequential)] public struct RAWINPUTHEADER { public uint dwType, dwSize; public IntPtr hDevice, wParam; }
    [DllImport("user32.dll", SetLastError = true)] public static extern bool RegisterRawInputDevices(RAWINPUTDEVICE[] d, uint n, uint sz);
    [DllImport("user32.dll")] public static extern uint GetRawInputData(IntPtr h, uint cmd, IntPtr data, ref uint size, uint hdr);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern uint GetRawInputDeviceInfoW(IntPtr h, uint cmd, IntPtr data, ref uint size);
    [DllImport("hid.dll")] public static extern int HidP_GetCaps(IntPtr p, byte[] c);
    [DllImport("hid.dll")] public static extern int HidP_GetUsages(int t, ushort page, ushort link, [Out] ushort[] list, ref uint len, IntPtr p, byte[] r, uint rl);
    [DllImport("hid.dll")] public static extern int HidP_GetUsageValue(int t, ushort page, ushort link, ushort usage, out uint v, IntPtr p, byte[] r, uint rl);
    [DllImport("hid.dll")] public static extern int HidP_GetScaledUsageValue(int t, ushort page, ushort link, ushort usage, out int v, IntPtr p, byte[] r, uint rl);

    // ---- power / display / metrics ----
    [DllImport("user32.dll")] public static extern IntPtr RegisterPowerSettingNotification(IntPtr h, ref Guid g, int flags);
    [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)] public struct DISPLAY_DEVICE {
        public int cb; [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string DeviceName;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 128)] public string DeviceString; public int StateFlags;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 128)] public string DeviceID;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 128)] public string DeviceKey;
    }
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)] public struct DEVMODE {
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string dmDeviceName;
        public short dmSpecVersion, dmDriverVersion, dmSize, dmDriverExtra; public int dmFields;
        public int dmPositionX, dmPositionY, dmDisplayOrientation, dmDisplayFixedOutput;
        public short dmColor, dmDuplex, dmYResolution, dmTTOption, dmCollate;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string dmFormName;
        public short dmLogPixels; public int dmBitsPerPel, dmPelsWidth, dmPelsHeight, dmDisplayFlags, dmDisplayFrequency;
        public int dmICMMethod, dmICMIntent, dmMediaType, dmDitherType, dmReserved1, dmReserved2, dmPanningWidth, dmPanningHeight;
    }
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern bool EnumDisplayDevicesW(string dev, int i, ref DISPLAY_DEVICE d, int f);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern bool EnumDisplaySettingsExW(string dev, int mode, ref DEVMODE dm, int f);

    public static DEVMODE NewDevMode() { var d = new DEVMODE(); d.dmSize = (short)Marshal.SizeOf(typeof(DEVMODE)); return d; }
    public static List<string> ActiveDisplays() {
        var r = new List<string>();
        for (int i = 0; ; i++) {
            var d = new DISPLAY_DEVICE(); d.cb = Marshal.SizeOf(d);
            if (!EnumDisplayDevicesW(null, i, ref d, 0)) break;
            if ((d.StateFlags & 1) != 0) r.Add(d.DeviceName);
        }
        return r;
    }
    public static string CurrentMode(string dev) {
        var m = NewDevMode();
        if (!EnumDisplaySettingsExW(dev, -1, ref m, 0)) return "?";
        return string.Format("{0}x{1}@{2} rot{3}", m.dmPelsWidth, m.dmPelsHeight, m.dmDisplayFrequency, m.dmDisplayOrientation * 90);
    }
}

class ProbeForm : Form {
    readonly string dir; readonly bool canvas, kbd;
    readonly Dictionary<IntPtr, bool> penKbd = new Dictionary<IntPtr, bool>();
    Csv pen, touch, raw, ev;
    readonly Dictionary<IntPtr, N.POINTER_DEVICE_PROPERTY[]> props = new Dictionary<IntPtr, N.POINTER_DEVICE_PROPERTY[]>();
    readonly Dictionary<IntPtr, int> rawDevIdx = new Dictionary<IntPtr, int>();
    readonly Dictionary<IntPtr, IntPtr> rawPrep = new Dictionary<IntPtr, IntPtr>();
    readonly Dictionary<IntPtr, int> ptrDevIdx = new Dictionary<IntPtr, int>();
    StreamWriter devlist;
    System.Windows.Forms.Timer poll;
    Bitmap ink; Point last = Point.Empty; bool lastValid;
    Label stepLabel; Button nextBtn; string stepText = "";
    Dictionary<string, string> lastState = new Dictionary<string, string>();
    readonly Dictionary<uint, uint> lastFrame = new Dictionary<uint, uint>();   // ENTER/DOWN/UPDATE can repeat a frame
    bool NewFrame(uint id, uint frame) { uint f; if (lastFrame.TryGetValue(id, out f) && f == frame) return false; lastFrame[id] = frame; return true; }

    public ProbeForm(string dir, bool canvas, bool kbd) {
        this.dir = dir; this.canvas = canvas; this.kbd = kbd;
        Directory.CreateDirectory(dir);
        devlist = new StreamWriter(Path.Combine(dir, "devices.txt"), false, new UTF8Encoding(false)); devlist.AutoFlush = true;
        ev = new Csv(Path.Combine(dir, "events.csv"), "t,kind,a,b,c");
        raw = new Csv(Path.Combine(dir, "rawinput.csv"), "t,dev,type,report_id,len,decoded,hex");
        if (canvas) {
            pen = new Csv(Path.Combine(dir, "pen.csv"), "t,frame,id,flags,x,y,pressure,rotation,tilt_x,tilt_y,pen_flags,pen_mask,dev,raw");
            touch = new Csv(Path.Combine(dir, "touch.csv"), "t,frame,id,flags,x,y,w,h,orientation,pressure");
            Text = "sp9 baseline"; WindowState = FormWindowState.Maximized; BackColor = Color.White; DoubleBuffered = true;
            KeyPreview = true;
            stepLabel = new Label(); stepLabel.Dock = DockStyle.Top; stepLabel.Height = 90; stepLabel.Font = new Font("Malgun Gothic", 14);
            stepLabel.BackColor = Color.FromArgb(240, 240, 240); stepLabel.Padding = new Padding(8);
            nextBtn = new Button(); nextBtn.Text = "다음 (Enter)"; nextBtn.Width = 160; nextBtn.Height = 60; nextBtn.Font = new Font("Malgun Gothic", 13);
            nextBtn.Anchor = AnchorStyles.Top | AnchorStyles.Right; nextBtn.Click += delegate { Next(); };
            Controls.Add(nextBtn); Controls.Add(stepLabel);
            Resize += delegate { nextBtn.Left = ClientSize.Width - nextBtn.Width - 10; nextBtn.Top = 100; };
            KeyDown += (s, e) => { if (e.KeyCode == Keys.Enter) { Next(); e.Handled = true; } if (e.KeyCode == Keys.Escape) Close(); };
        } else {
            FormBorderStyle = FormBorderStyle.None; ShowInTaskbar = false; StartPosition = FormStartPosition.Manual;
            Location = new Point(-32000, -32000); Size = new Size(1, 1); Opacity = 0;
        }
        poll = new System.Windows.Forms.Timer(); poll.Interval = 200; poll.Tick += delegate { Poll(); }; poll.Start();
    }

    void Next() { try { File.WriteAllText(Path.Combine(dir, "next"), Clock.Now()); } catch { } }

    protected override void OnHandleCreated(EventArgs e) {
        base.OnHandleCreated(e);
        // Raw input: sink mode so reports arrive without focus. Pen/touch digitizers, BLE stylus TLC, consumer, system control,
        // and keyboards only with --kbd (pen tail button). Key codes are logged only for the pen's BLE keyboard; other keyboards
        // (Type Cover, external) are counted without codes so typing never lands in a log.
        ushort[,] want = kbd ? new ushort[,] { { 0x0D, 0x02 }, { 0x0D, 0x04 }, { 0x0D, 0x20 }, { 0x0C, 0x01 }, { 0x01, 0x80 }, { 0x01, 0x06 } }
                             : new ushort[,] { { 0x0D, 0x02 }, { 0x0D, 0x04 }, { 0x0D, 0x20 }, { 0x0C, 0x01 }, { 0x01, 0x80 } };
        for (int i = 0; i < want.GetLength(0); i++) {
            var d = new N.RAWINPUTDEVICE[1];
            d[0].usUsagePage = want[i, 0]; d[0].usUsage = want[i, 1]; d[0].dwFlags = 0x100 | 0x2000; d[0].hwndTarget = Handle;
            bool ok = N.RegisterRawInputDevices(d, 1, (uint)Marshal.SizeOf(typeof(N.RAWINPUTDEVICE)));
            ev.Row("rawinput_register", string.Format("{0:X2}:{1:X2}", want[i, 0], want[i, 1]), ok ? "ok" : "err" + Marshal.GetLastWin32Error(), "");
        }
        string[] guids = {
            "BA3E0F4D-B817-4094-A2D1-D56379E6A0F3", // lid switch
            "5D3E9A59-E9D5-4B00-A6BD-FF34FF516548", // AC/DC source
            "6FE69556-704A-47A0-8F24-C28D936FDA47", // console display state
            "A7AD8041-B45A-4CAE-87A3-EECBB468A9E1", // battery %
            "245D8541-3943-4422-B025-13A784F679B7", // power scheme personality
            "31F9F286-5084-42FE-B720-2B0264993763", // active scheme
            "98A7F580-01F7-48AA-9C0F-44352C29E5C0", // monitor power on
            "E00958C0-C213-4ACE-AC77-FECCED2EEEA5", // energy saver status
        };
        foreach (var s in guids) { var g = new Guid(s); N.RegisterPowerSettingNotification(Handle, ref g, 0); }
        if (canvas) { ink = new Bitmap(Math.Max(1, ClientSize.Width), Math.Max(1, ClientSize.Height)); }
        Poll();
        ev.Row("start", canvas ? "pointer" : "events", "", "");
    }

    protected override void OnResize(EventArgs e) {
        base.OnResize(e);
        if (canvas && ClientSize.Width > 0 && ClientSize.Height > 0) {
            var b = new Bitmap(ClientSize.Width, ClientSize.Height);
            if (ink != null) { using (var g = Graphics.FromImage(b)) g.DrawImage(ink, 0, 0); ink.Dispose(); }
            ink = b; Invalidate();
        }
    }
    protected override void OnPaint(PaintEventArgs e) { if (ink != null) e.Graphics.DrawImage(ink, 0, 0); }

    void State(string key, string val) {
        string old;
        if (lastState.TryGetValue(key, out old) && old == val) return;
        lastState[key] = val; ev.Row("state", key, val, "");
    }

    void Poll() {
        // States that can change without a broadcast: refresh rate (DRR), slate/dock metrics, display list.
        State("slate_mode", N.GetSystemMetrics(0x2003) == 0 ? "tablet" : "laptop");
        State("docked", N.GetSystemMetrics(0x2004).ToString());
        var ds = N.ActiveDisplays();
        State("displays", ds.Count.ToString());
        for (int i = 0; i < ds.Count; i++) State("display" + i, N.CurrentMode(ds[i]));
        if (File.Exists(Path.Combine(dir, "stop"))) { Close(); return; }
        if (canvas) {
            try {
                var p = Path.Combine(dir, "step.txt");
                var s = File.Exists(p) ? File.ReadAllText(p, Encoding.UTF8) : "";
                if (s != stepText) {
                    stepText = s; stepLabel.Text = s;
                    if (s.Contains("[지우기]") && ink != null) { using (var g = Graphics.FromImage(ink)) g.Clear(Color.White); Invalidate(); }
                }
            } catch { }
        }
        pen_flush++;
        if (pen_flush % 5 == 0) { ev.Flush(); raw.Flush(); if (pen != null) { pen.Flush(); touch.Flush(); } }
    }
    int pen_flush;

    protected override void OnFormClosed(FormClosedEventArgs e) {
        ev.Row("stop", "", "", "");
        ev.Dispose(); raw.Dispose(); if (pen != null) { pen.Dispose(); touch.Dispose(); } devlist.Dispose();
        base.OnFormClosed(e);
    }

    N.POINTER_DEVICE_PROPERTY[] Props(IntPtr dev) {
        N.POINTER_DEVICE_PROPERTY[] p;
        if (props.TryGetValue(dev, out p)) return p;
        uint n = 0; N.GetPointerDeviceProperties(dev, ref n, null);
        p = new N.POINTER_DEVICE_PROPERTY[n];
        if (n > 0 && !N.GetPointerDeviceProperties(dev, ref n, p)) p = new N.POINTER_DEVICE_PROPERTY[0];
        props[dev] = p;
        int idx = ptrDevIdx.Count; ptrDevIdx[dev] = idx;
        var sb = new StringBuilder(string.Format("pointer dev {0}:", idx));
        foreach (var q in p) sb.AppendFormat(" {0:X2}:{1:X2}[{2}..{3}]", q.usagePageId, q.usageId, q.logicalMin, q.logicalMax);
        devlist.WriteLine(sb.ToString());
        return p;
    }

    protected override void WndProc(ref Message m) {
        try {
            switch (m.Msg) {
                case 0x0245: case 0x0246: case 0x0247: case 0x0249: case 0x024A: // POINTER UPDATE/DOWN/UP/ENTER/LEAVE
                    if (canvas) { OnPointer(m); m.Result = IntPtr.Zero; return; }
                    break;
                case 0x00FF: OnRawInput(m.LParam); break;
                case 0x00FE: ev.Row("rawinput_device", m.WParam.ToInt32() == 1 ? "arrival" : "removal", DevIdx(m.LParam), ""); break;
                case 0x0218: OnPower(m); break;
                case 0x001A: {
                    string s = m.LParam != IntPtr.Zero ? Marshal.PtrToStringUni(m.LParam) : "";
                    ev.Row("settingchange", s, "slate=" + N.GetSystemMetrics(0x2003), "");
                    break;
                }
                case 0x007E: ev.Row("displaychange", (m.LParam.ToInt64() & 0xFFFF) + "x" + ((m.LParam.ToInt64() >> 16) & 0xFFFF), m.WParam.ToInt32() + "bpp", ""); break;
                case 0x0219: if (m.WParam.ToInt32() == 7) ev.Row("devnodes_changed", "", "", ""); break;
            }
        } catch (Exception ex) { ev.Row("error", ex.GetType().Name, ex.Message.Replace(',', ';'), ""); }
        base.WndProc(ref m);
    }

    void OnPower(Message m) {
        int code = m.WParam.ToInt32();
        if (code == 0x8013 && m.LParam != IntPtr.Zero) {
            byte[] gb = new byte[16]; Marshal.Copy(m.LParam, gb, 0, 16);
            var g = new Guid(gb); int len = Marshal.ReadInt32(m.LParam, 16);
            string val = len >= 16 ? new Guid(ReadBytes(m.LParam + 20, 16)).ToString() : len >= 4 ? Marshal.ReadInt32(m.LParam, 20).ToString() : (len > 0 ? Marshal.ReadByte(m.LParam, 20).ToString() : "");
            ev.Row("power_setting", g.ToString(), val, "");
        } else ev.Row("powerbroadcast", "0x" + code.ToString("X"), "", "");
    }
    static byte[] ReadBytes(IntPtr p, int n) { var b = new byte[n]; Marshal.Copy(p, b, 0, n); return b; }

    void OnPointer(Message m) {
        uint id = (uint)(m.WParam.ToInt64() & 0xFFFF);
        int type; if (!N.GetPointerType(id, out type)) return;
        if (type == 3) { // pen
            N.POINTER_INFO pi; N.GetPointerInfo(id, out pi);
            uint n = Math.Max(1u, pi.historyCount);
            var a = new N.POINTER_PEN_INFO[n];
            if (!N.GetPointerPenInfoHistory(id, ref n, a)) return;
            var pr = Props(pi.sourceDevice);
            int[] rv = null;
            if (pr.Length > 0) { rv = new int[n * pr.Length]; if (!N.GetRawPointerDeviceData(id, n, (uint)pr.Length, pr, rv)) rv = null; }
            for (int k = (int)n - 1; k >= 0; k--) { // history is newest first
                var p = a[k]; var ip = p.pointerInfo;
                if (!NewFrame(id, ip.frameId)) continue;
                string rs = "";
                if (rv != null) {
                    var sb = new StringBuilder();
                    for (int j = 0; j < pr.Length; j++) { if (j > 0) sb.Append(';'); sb.AppendFormat("{0:X2}:{1:X2}={2}", pr[j].usagePageId, pr[j].usageId, rv[k * pr.Length + j]); }
                    rs = sb.ToString();
                }
                pen.Row(ip.frameId, id, "0x" + ip.pointerFlags.ToString("X"), ip.ptHimetricLocationRaw.X, ip.ptHimetricLocationRaw.Y,
                    p.pressure, p.rotation, p.tiltX, p.tiltY, "0x" + p.penFlags.ToString("X"), "0x" + p.penMask.ToString("X"), ptrDevIdx[ip.sourceDevice], rs);
            }
            var cur = a[0].pointerInfo; var pt = PointToClient(new Point(cur.ptPixelLocation.X, cur.ptPixelLocation.Y));
            bool contact = (cur.pointerFlags & 0x4) != 0;
            if (contact && ink != null) {
                bool eraser = (a[0].penFlags & 0x6) != 0;
                float w = 1 + a[0].pressure / 128f;
                using (var g = Graphics.FromImage(ink)) using (var pn = new Pen(eraser ? Color.Red : ((a[0].penFlags & 1) != 0 ? Color.Blue : Color.Black), w)) {
                    if (lastValid) g.DrawLine(pn, last, pt); else g.FillEllipse(Brushes.Black, pt.X - 1, pt.Y - 1, 2, 2);
                }
                Invalidate(new Rectangle(Math.Min(last.X, pt.X) - 20, Math.Min(last.Y, pt.Y) - 20, Math.Abs(last.X - pt.X) + 40, Math.Abs(last.Y - pt.Y) + 40));
                last = pt; lastValid = true;
            } else lastValid = false;
        } else if (type == 2) { // touch
            N.POINTER_TOUCH_INFO t;
            if (!N.GetPointerTouchInfo(id, out t)) return;
            var ip = t.pointerInfo;
            if (!NewFrame(id, ip.frameId)) return;
            touch.Row(ip.frameId, id, "0x" + ip.pointerFlags.ToString("X"), ip.ptHimetricLocationRaw.X, ip.ptHimetricLocationRaw.Y,
                t.rcContact.R - t.rcContact.L, t.rcContact.B - t.rcContact.T, t.orientation, t.pressure);
            var pt = PointToClient(new Point(ip.ptPixelLocation.X, ip.ptPixelLocation.Y));
            if ((ip.pointerFlags & 0x4) != 0 && ink != null) { using (var g = Graphics.FromImage(ink)) g.FillEllipse(Brushes.Green, pt.X - 4, pt.Y - 4, 8, 8); Invalidate(new Rectangle(pt.X - 6, pt.Y - 6, 12, 12)); }
        }
    }

    string DevIdx(IntPtr h) {
        int idx;
        if (rawDevIdx.TryGetValue(h, out idx)) return idx.ToString();
        idx = rawDevIdx.Count; rawDevIdx[h] = idx;
        uint sz = 0; N.GetRawInputDeviceInfoW(h, 0x20000007, IntPtr.Zero, ref sz);
        string name = "?";
        if (sz > 0) { IntPtr b = Marshal.AllocHGlobal((int)sz * 2); if (N.GetRawInputDeviceInfoW(h, 0x20000007, b, ref sz) != unchecked((uint)-1)) name = Marshal.PtrToStringUni(b); Marshal.FreeHGlobal(b); }
        uint ps = 0; N.GetRawInputDeviceInfoW(h, 0x20000005, IntPtr.Zero, ref ps);
        string tlc = "";
        if (ps > 0) {
            IntPtr pp = Marshal.AllocHGlobal((int)ps);
            if (N.GetRawInputDeviceInfoW(h, 0x20000005, pp, ref ps) != unchecked((uint)-1)) {
                rawPrep[h] = pp;
                byte[] c = new byte[64]; N.HidP_GetCaps(pp, c);
                tlc = string.Format(" TLC {0:X4}:{1:X4} in {2}", BitConverter.ToUInt16(c, 2), BitConverter.ToUInt16(c, 0), BitConverter.ToUInt16(c, 4));
            } else Marshal.FreeHGlobal(pp);
        }
        penKbd[h] = System.Text.RegularExpressions.Regex.IsMatch(name, "(?i)0c0f|00001812");
        devlist.WriteLine("raw dev {0}:{1} {2}", idx, tlc, name);
        return idx.ToString();
    }

    // Decode digitizer fields generically through the preparsed data (works for any pen/stylus TLC).
    string Decode(IntPtr prep, byte[] rep) {
        var sb = new StringBuilder();
        var list = new ushort[32]; uint len = 32;
        if (N.HidP_GetUsages(0, 0x0D, 0, list, ref len, prep, rep, (uint)rep.Length) == 0x110000) {
            var names = new List<string>();
            for (int i = 0; i < len; i++) {
                switch (list[i]) { case 0x42: names.Add("tip"); break; case 0x44: names.Add("barrel"); break; case 0x45: names.Add("eraser"); break;
                    case 0x3C: names.Add("invert"); break; case 0x32: names.Add("inrange"); break; case 0x5A: names.Add("barrel2"); break;
                    default: names.Add("0D:" + list[i].ToString("X2")); break; }
            }
            sb.Append("btn=" + string.Join("+", names.ToArray()));
        }
        ushort[,] vals = { { 0x01, 0x30 }, { 0x01, 0x31 }, { 0x0D, 0x30 }, { 0x0D, 0x3D }, { 0x0D, 0x3E }, { 0x0D, 0x41 }, { 0x0D, 0x56 }, { 0x0D, 0x5B }, { 0x0D, 0x3B } };
        string[] vn = { "x", "y", "pressure", "tilt_x", "tilt_y", "twist", "scan", "serial", "battery" };
        for (int i = 0; i < vn.Length; i++) {
            uint v;
            if (N.HidP_GetUsageValue(0, vals[i, 0], 0, vals[i, 1], out v, prep, rep, (uint)rep.Length) == 0x110000) {
                if (vn[i] == "serial") { sb.Append(" serial=present"); continue; }
                int sv;
                if ((vn[i] == "tilt_x" || vn[i] == "tilt_y") && N.HidP_GetScaledUsageValue(0, vals[i, 0], 0, vals[i, 1], out sv, prep, rep, (uint)rep.Length) == 0x110000)
                    sb.AppendFormat(" {0}={1}", vn[i], sv);
                else sb.AppendFormat(" {0}={1}", vn[i], v);
            }
        }
        return sb.ToString().Trim();
    }

    void OnRawInput(IntPtr h) {
        uint hs = (uint)Marshal.SizeOf(typeof(N.RAWINPUTHEADER)); uint sz = 0;
        N.GetRawInputData(h, 0x10000003, IntPtr.Zero, ref sz, hs);
        if (sz == 0) return;
        IntPtr b = Marshal.AllocHGlobal((int)sz);
        try {
            if (N.GetRawInputData(h, 0x10000003, b, ref sz, hs) != sz) return;
            var hdr = (N.RAWINPUTHEADER)Marshal.PtrToStructure(b, typeof(N.RAWINPUTHEADER));
            string dev = DevIdx(hdr.hDevice);
            IntPtr body = b + (int)hs;
            if (hdr.dwType == 1) { // keyboard: MakeCode, Flags, Reserved, VKey, Message
                ushort make = (ushort)Marshal.ReadInt16(body, 0), fl = (ushort)Marshal.ReadInt16(body, 2), vk = (ushort)Marshal.ReadInt16(body, 6);
                bool isPen; penKbd.TryGetValue(hdr.hDevice, out isPen);
                if (isPen) raw.Row(dev, "kbd", "", "", string.Format("vk=0x{0:X2} make=0x{1:X2} {2}{3}", vk, make, (fl & 1) != 0 ? "up" : "down", (fl & 2) != 0 ? " e0" : ""), "");
                else raw.Row(dev, "kbd_other", "", "", (fl & 1) != 0 ? "up" : "down", "");
            } else if (hdr.dwType == 2) {
                int each = Marshal.ReadInt32(body, 0), count = Marshal.ReadInt32(body, 4);
                IntPtr prep; rawPrep.TryGetValue(hdr.hDevice, out prep);
                for (int i = 0; i < count; i++) {
                    var rep = new byte[each]; Marshal.Copy(body + 8 + i * each, rep, 0, each);
                    string dec = prep != IntPtr.Zero ? Decode(prep, rep) : "";
                    raw.Row(dev, "hid", rep.Length > 0 ? rep[0].ToString() : "", each, dec, BitConverter.ToString(rep).Replace("-", ""));
                }
            }
        } finally { Marshal.FreeHGlobal(b); }
    }
}

// ---- audio endpoint volume (Core Audio) ----
[ComImport, Guid("BCDE0395-E52F-467C-8E3D-C4579291692E")] class MMDeviceEnumeratorCo { }
[Guid("A95664D2-9614-4F35-A746-DE8DB63617E6"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IMMDeviceEnumerator {
    [PreserveSig] int EnumAudioEndpoints(int flow, int mask, out IMMDeviceCollection c);
    [PreserveSig] int GetDefaultAudioEndpoint(int flow, int role, out IMMDevice d);
}
[Guid("0BD7A1BE-7A1A-44DB-8397-CC5392387B5E"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IMMDeviceCollection { [PreserveSig] int GetCount(out int n); [PreserveSig] int Item(int i, out IMMDevice d); }
[Guid("D666063F-1587-4E43-81F1-B948E807363F"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IMMDevice {
    [PreserveSig] int Activate(ref Guid iid, int ctx, IntPtr p, [MarshalAs(UnmanagedType.IUnknown)] out object o);
    [PreserveSig] int OpenPropertyStore(int access, out IPropertyStore ps);
}
[Guid("886d8eeb-8cf2-4446-8d02-cdba1dbdcf99"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IPropertyStore { [PreserveSig] int GetCount(out int n); [PreserveSig] int GetAt(int i, out PROPERTYKEY k); [PreserveSig] int GetValue(ref PROPERTYKEY k, out PROPVARIANT v); }
[StructLayout(LayoutKind.Sequential)] struct PROPERTYKEY { public Guid fmtid; public int pid; }
[StructLayout(LayoutKind.Explicit, Size = 24)] struct PROPVARIANT { [FieldOffset(0)] public short vt; [FieldOffset(8)] public IntPtr p; }
[Guid("5CDF2C82-841E-4546-9722-0CF74078229A"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IAudioEndpointVolume {
    [PreserveSig] int RegisterControlChangeNotify(IntPtr p); [PreserveSig] int UnregisterControlChangeNotify(IntPtr p);
    [PreserveSig] int GetChannelCount(out int n);
    [PreserveSig] int SetMasterVolumeLevel(float l, ref Guid g); [PreserveSig] int SetMasterVolumeLevelScalar(float l, ref Guid g);
    [PreserveSig] int GetMasterVolumeLevel(out float l); [PreserveSig] int GetMasterVolumeLevelScalar(out float l);
    [PreserveSig] int SetChannelVolumeLevel(int c, float l, ref Guid g); [PreserveSig] int SetChannelVolumeLevelScalar(int c, float l, ref Guid g);
    [PreserveSig] int GetChannelVolumeLevel(int c, out float l); [PreserveSig] int GetChannelVolumeLevelScalar(int c, out float l);
    [PreserveSig] int SetMute([MarshalAs(UnmanagedType.Bool)] bool m, ref Guid g); [PreserveSig] int GetMute([MarshalAs(UnmanagedType.Bool)] out bool m);
}

static class Vol {
    static List<KeyValuePair<string, IMMDevice>> Render() {
        var r = new List<KeyValuePair<string, IMMDevice>>();
        var e = (IMMDeviceEnumerator)new MMDeviceEnumeratorCo();
        IMMDeviceCollection c; e.EnumAudioEndpoints(0, 1, out c);
        int n; c.GetCount(out n);
        var key = new PROPERTYKEY(); key.fmtid = new Guid("a45c254e-df1c-4efd-8020-67d146a850e0"); key.pid = 14;
        for (int i = 0; i < n; i++) {
            IMMDevice d; c.Item(i, out d);
            IPropertyStore ps; d.OpenPropertyStore(0, out ps);
            PROPVARIANT v; ps.GetValue(ref key, out v);
            r.Add(new KeyValuePair<string, IMMDevice>(v.vt == 31 ? Marshal.PtrToStringUni(v.p) : "?", d));
        }
        return r;
    }
    static IAudioEndpointVolume Ep(IMMDevice d) {
        var iid = new Guid("5CDF2C82-841E-4546-9722-0CF74078229A"); object o;
        d.Activate(ref iid, 23, IntPtr.Zero, out o); return (IAudioEndpointVolume)o;
    }
    public static int Run(string[] a) {
        var all = Render();
        if (a.Length < 2 || a[1] == "list") {
            foreach (var kv in all) { float l; bool mu; var v = Ep(kv.Value); v.GetMasterVolumeLevelScalar(out l); v.GetMute(out mu); Console.WriteLine("{0:F3}\t{1}\t{2}", l, mu ? "muted" : "unmuted", kv.Key); }
            return 0;
        }
        foreach (var kv in all) {
            if (kv.Key.IndexOf(a[2], StringComparison.OrdinalIgnoreCase) < 0) continue;
            var v = Ep(kv.Value);
            if (a[1] == "get") { float l; bool mu; v.GetMasterVolumeLevelScalar(out l); v.GetMute(out mu); Console.WriteLine("{0:F3} {1}", l, mu ? 1 : 0); return 0; }
            if (a[1] == "set") {
                var g = Guid.Empty;
                float l = float.Parse(a[3], CultureInfo.InvariantCulture);
                v.SetMasterVolumeLevelScalar(Math.Max(0f, Math.Min(1f, l)), ref g);
                if (a.Length > 4) v.SetMute(a[4] == "1", ref g);
                return 0;
            }
        }
        Console.Error.WriteLine("no render endpoint matches " + (a.Length > 2 ? a[2] : ""));
        return 2;
    }
}

static class Program {
    static void Devices() {
        uint n = 0; N.GetPointerDevices(ref n, null);
        var a = new N.POINTER_DEVICE_INFO[n];
        if (n == 0 || !N.GetPointerDevices(ref n, a)) { Console.WriteLine("no pointer devices"); return; }
        string[] types = { "?", "integrated_pen", "external_pen", "touch", "touch_pad" };
        foreach (var d in a) {
            N.RECT dr, sr; N.GetPointerDeviceRects(d.device, out dr, out sr);
            Console.WriteLine("device type={0} max_contacts={1} product=\"{2}\" rect={3}x{4} himetric display={5}x{6}",
                d.pointerDeviceType < types.Length ? types[d.pointerDeviceType] : d.pointerDeviceType.ToString(), d.maxActiveContacts, d.productString,
                dr.R - dr.L, dr.B - dr.T, sr.R - sr.L, sr.B - sr.T);
            uint pn = 0; N.GetPointerDeviceProperties(d.device, ref pn, null);
            var p = new N.POINTER_DEVICE_PROPERTY[pn];
            if (pn > 0 && N.GetPointerDeviceProperties(d.device, ref pn, p))
                foreach (var q in p) Console.WriteLine("  usage {0:X2}:{1:X2} logical {2}..{3} physical {4}..{5} unit 0x{6:X} exp {7}",
                    q.usagePageId, q.usageId, q.logicalMin, q.logicalMax, q.physicalMin, q.physicalMax, q.unit, q.unitExponent);
        }
    }

    static void Display() {
        foreach (var dev in N.ActiveDisplays()) {
            Console.WriteLine("display {0} current {1}", dev, N.CurrentMode(dev));
            // group refresh rates by resolution, largest resolution first
            var byRes = new Dictionary<long, SortedSet<int>>();
            for (int i = 0; ; i++) {
                var m = N.NewDevMode();
                if (!N.EnumDisplaySettingsExW(dev, i, ref m, 2)) break; // EDS_RAWMODE
                long key = (long)m.dmPelsWidth * 100000 + m.dmPelsHeight;
                if (!byRes.ContainsKey(key)) byRes[key] = new SortedSet<int>();
                byRes[key].Add(m.dmDisplayFrequency);
            }
            var keys = new List<long>(byRes.Keys); keys.Sort(); keys.Reverse();
            foreach (var k in keys) {
                var rs = new List<string>(); foreach (var r in byRes[k]) rs.Add(r.ToString());
                Console.WriteLine("  mode {0}x{1} Hz {2}", k / 100000, k % 100000, string.Join(",", rs.ToArray()));
            }
        }
    }

    static void Stress(int sec, int threads) {
        var end = DateTime.UtcNow.AddSeconds(sec); var ts = new List<Thread>();
        Console.WriteLine("stress start {0} threads {1} s at {2}", threads, sec, Clock.Now());
        for (int i = 0; i < threads; i++) {
            var t = new Thread(() => { double x = 1.0001; while (DateTime.UtcNow < end) { for (int k = 0; k < 100000; k++) x = x * 1.0000001 + 1e-9; } if (x == 0) Console.Write(""); });
            t.IsBackground = true; t.Start(); ts.Add(t);
        }
        foreach (var t in ts) t.Join();
        Console.WriteLine("stress end {0}", Clock.Now());
    }

    [STAThread]
    static int Main(string[] a) {
        Thread.CurrentThread.CurrentCulture = CultureInfo.InvariantCulture;
        Console.OutputEncoding = new UTF8Encoding(false);
        if (a.Length == 0) { Console.Error.WriteLine("usage: sp9probe pointer|events <dir> | devices | display | stress <sec> [threads] | volume list|get|set"); return 1; }
        switch (a[0]) {
            case "pointer": case "events":
                Application.EnableVisualStyles();
                Application.Run(new ProbeForm(a[1], a[0] == "pointer", Array.IndexOf(a, "--kbd") > 0));
                return 0;
            case "devices": Devices(); return 0;
            case "display": Display(); return 0;
            case "stress": Stress(int.Parse(a[1]), a.Length > 2 ? int.Parse(a[2]) : Environment.ProcessorCount); return 0;
            case "volume": return Vol.Run(a);
        }
        return 1;
    }
}
