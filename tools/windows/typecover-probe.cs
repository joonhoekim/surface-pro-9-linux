// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Joonhoe Kim
// Type Cover (KIP: SHP devices 1 keyboard, 2 pen charger, 3 touchpad, 5 firmware/colour) probe. Read-only toward the
// cover: GET_FEATURE / GET_INPUT_REPORT / ReadFile / Raw Input only, never SET_FEATURE, SET_OUTPUT or WriteFile.
// C# 5 (.NET Framework csc); typecover-probe.ps1 builds it into out/typecover/.
//
//   typecover-probe.exe layout   <out.txt>          every KIP collection: caps, link collections, and the bit position of
//                                                   each field (rebuilt with HidP_Set* on a scratch buffer)
//   typecover-probe.exe features <out.txt>          GET of every feature report (+ GET_INPUT of Fn state / backlight)
//   typecover-probe.exe session  <dir> [--auto N]   guided steps (Korean prompts, Enter between steps) with Raw Input,
//                                                   ReadFile on shareable KIP collections, low-level mouse hook (wheel
//                                                   and buttons only), feature snapshots per step, polls; --auto N runs
//                                                   every step for N seconds without prompts (self test)
//
// Masking: keyboard-page (07) usages for letters, digits, punctuation, space and keypad are written as 07:** and their
// report bytes are dropped; Raw Input keyboard records of character keys keep only down/up. Pen-bonding collections
// (FFF4, Bluetooth address) are never read.
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Drawing;
using System.IO;
using System.Linq;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.RegularExpressions;
using System.Threading;
using System.Windows.Forms;

static class N {
    [DllImport("hid.dll")] public static extern void HidD_GetHidGuid(out Guid g);
    [DllImport("hid.dll")] public static extern bool HidD_GetPreparsedData(IntPtr h, out IntPtr p);
    [DllImport("hid.dll")] public static extern bool HidD_FreePreparsedData(IntPtr p);
    [DllImport("hid.dll")] public static extern bool HidD_GetAttributes(IntPtr h, byte[] a);
    [DllImport("hid.dll")] public static extern bool HidD_GetProductString(IntPtr h, byte[] b, int len);
    [DllImport("hid.dll", SetLastError = true)] public static extern bool HidD_GetFeature(IntPtr h, byte[] b, int len);
    [DllImport("hid.dll", SetLastError = true)] public static extern bool HidD_GetInputReport(IntPtr h, byte[] b, int len);
    [DllImport("hid.dll")] public static extern int HidP_GetCaps(IntPtr p, byte[] c);
    [DllImport("hid.dll")] public static extern int HidP_GetValueCaps(int t, byte[] c, ref ushort n, IntPtr p);
    [DllImport("hid.dll")] public static extern int HidP_GetButtonCaps(int t, byte[] c, ref ushort n, IntPtr p);
    [DllImport("hid.dll")] public static extern int HidP_GetLinkCollectionNodes(byte[] c, ref uint n, IntPtr p);
    [DllImport("hid.dll")] public static extern int HidP_GetUsages(int t, ushort page, ushort link, [Out] ushort[] list, ref uint len, IntPtr p, byte[] r, uint rl);
    [DllImport("hid.dll")] public static extern int HidP_GetUsageValue(int t, ushort page, ushort link, ushort usage, out uint v, IntPtr p, byte[] r, uint rl);
    [DllImport("hid.dll")] public static extern int HidP_GetUsageValueArray(int t, ushort page, ushort link, ushort usage, byte[] v, ushort vl, IntPtr p, byte[] r, uint rl);
    [DllImport("hid.dll")] public static extern int HidP_SetUsageValue(int t, ushort page, ushort link, ushort usage, uint v, IntPtr p, byte[] r, uint rl);
    [DllImport("hid.dll")] public static extern int HidP_SetUsageValueArray(int t, ushort page, ushort link, ushort usage, byte[] v, ushort vl, IntPtr p, byte[] r, uint rl);
    [DllImport("hid.dll")] public static extern int HidP_SetUsages(int t, ushort page, ushort link, ushort[] list, ref uint len, IntPtr p, byte[] r, uint rl);
    [DllImport("hid.dll")] public static extern int HidP_InitializeReportForID(int t, byte id, IntPtr p, byte[] r, uint rl);
    [DllImport("setupapi.dll", CharSet = CharSet.Unicode)] public static extern IntPtr SetupDiGetClassDevsW(ref Guid g, IntPtr e, IntPtr w, int f);
    [DllImport("setupapi.dll")] public static extern bool SetupDiEnumDeviceInterfaces(IntPtr s, IntPtr d, ref Guid g, int i, byte[] ifd);
    [DllImport("setupapi.dll", CharSet = CharSet.Unicode)] public static extern bool SetupDiGetDeviceInterfaceDetailW(IntPtr s, byte[] ifd, IntPtr det, int sz, out int req, IntPtr di);
    [DllImport("setupapi.dll")] public static extern bool SetupDiDestroyDeviceInfoList(IntPtr s);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)] public static extern IntPtr CreateFileW(string n, uint a, uint s, IntPtr sa, uint c, uint f, IntPtr t);
    [DllImport("kernel32.dll")] public static extern bool CloseHandle(IntPtr h);
    [DllImport("kernel32.dll", SetLastError = true)] public static extern bool ReadFile(IntPtr h, byte[] b, int n, out int read, IntPtr ov);
    [DllImport("kernel32.dll")] public static extern bool CancelIoEx(IntPtr h, IntPtr ov);
    [DllImport("kernel32.dll")] public static extern void GetSystemTimePreciseAsFileTime(out long ft);
    [DllImport("kernel32.dll")] public static extern uint SetThreadExecutionState(uint f);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr GetModuleHandleW(string n);

    [StructLayout(LayoutKind.Sequential)] public struct RAWINPUTDEVICE { public ushort usUsagePage, usUsage; public uint dwFlags; public IntPtr hwndTarget; }
    [StructLayout(LayoutKind.Sequential)] public struct RAWINPUTHEADER { public uint dwType, dwSize; public IntPtr hDevice, wParam; }
    [DllImport("user32.dll", SetLastError = true)] public static extern bool RegisterRawInputDevices(RAWINPUTDEVICE[] d, uint n, uint sz);
    [DllImport("user32.dll")] public static extern uint GetRawInputData(IntPtr h, uint cmd, IntPtr data, ref uint size, uint hdr);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern uint GetRawInputDeviceInfoW(IntPtr h, uint cmd, IntPtr data, ref uint size);

    public delegate IntPtr HookProc(int code, IntPtr w, IntPtr l);
    [DllImport("user32.dll", SetLastError = true)] public static extern IntPtr SetWindowsHookExW(int id, HookProc fn, IntPtr mod, uint tid);
    [DllImport("user32.dll")] public static extern bool UnhookWindowsHookEx(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr CallNextHookEx(IntPtr h, int code, IntPtr w, IntPtr l);

    public const int OK = 0x110000;
    public static long Now() { long t; GetSystemTimePreciseAsFileTime(out t); return t; }
    public static ushort U16(byte[] b, int o) { return BitConverter.ToUInt16(b, o); }
    public static int I32(byte[] b, int o) { return BitConverter.ToInt32(b, o); }
    public static string Hex(byte[] b, int n) { return BitConverter.ToString(b, 0, Math.Min(n, b.Length)).Replace("-", ""); }
}

// One HID collection: path, attributes, caps, and the decoder built from its preparsed data.
class Coll {
    public string Path, Label; public ushort Vid, Pid, Page, Usage; public int InLen, OutLen, FeatLen; public IntPtr Prep;
    public List<Cap> Caps = new List<Cap>();
    public bool IsPen { get { return Page == 0xFFF4; } }
    public bool IsKeyboardTlc { get { return Page == 0x01 && Usage == 0x06; } }

    public static readonly string[] RT = { "In", "Out", "Feat" };

    public class Cap {
        public int Type; public bool Button, Range; public byte Id; public ushort Page, Link, UMin, UMax, BitSize, Count, BitField;
        public int LMin, LMax, PMin, PMax, Unit, Exp; public bool Abs;
        public bool Array { get { return (BitField & 2) == 0; } }
    }

    public static List<string> Paths() {
        Guid g; N.HidD_GetHidGuid(out g); var r = new List<string>();
        IntPtr s = N.SetupDiGetClassDevsW(ref g, IntPtr.Zero, IntPtr.Zero, 0x12);
        for (int i = 0; ; i++) {
            byte[] ifd = new byte[32]; BitConverter.GetBytes(32).CopyTo(ifd, 0);
            if (!N.SetupDiEnumDeviceInterfaces(s, IntPtr.Zero, ref g, i, ifd)) break;
            int req; N.SetupDiGetDeviceInterfaceDetailW(s, ifd, IntPtr.Zero, 0, out req, IntPtr.Zero);
            IntPtr det = Marshal.AllocHGlobal(req); Marshal.WriteInt32(det, 8);
            if (N.SetupDiGetDeviceInterfaceDetailW(s, ifd, det, req, out req, IntPtr.Zero)) r.Add(Marshal.PtrToStringUni(det + 4));
            Marshal.FreeHGlobal(det);
        }
        N.SetupDiDestroyDeviceInfoList(s);
        r.Sort(StringComparer.OrdinalIgnoreCase);
        return r;
    }

    public static string ColNo(string path) {
        var m = Regex.Match(path, "(?i)&col([0-9a-f]{2})");
        return m.Success ? m.Groups[1].Value.ToUpper() : "00";
    }

    public static List<Coll> Kip() {
        var r = new List<Coll>();
        foreach (var p in Paths()) {
            if (!Regex.IsMatch(p, "(?i)target_kip")) continue;
            var c = Open(p); if (c != null) r.Add(c);
        }
        return r;
    }

    public static Coll Open(string path) {
        IntPtr h = N.CreateFileW(path, 0, 3, IntPtr.Zero, 3, 0, IntPtr.Zero);
        if (h == new IntPtr(-1)) return null;
        try {
            var c = new Coll(); c.Path = path;
            byte[] at = new byte[12]; BitConverter.GetBytes(12).CopyTo(at, 0);
            if (N.HidD_GetAttributes(h, at)) { c.Vid = N.U16(at, 4); c.Pid = N.U16(at, 6); }
            IntPtr p; if (!N.HidD_GetPreparsedData(h, out p)) return null;
            c.Init(p);
            return c;
        } finally { N.CloseHandle(h); }
    }

    public void Init(IntPtr p) {
        Prep = p;
        byte[] c = new byte[64]; N.HidP_GetCaps(p, c);
        Usage = N.U16(c, 0); Page = N.U16(c, 2); InLen = N.U16(c, 4); OutLen = N.U16(c, 6); FeatLen = N.U16(c, 8);
        Label = string.Format("{0:X4} c{1} {2:X4}:{3:X4}", Pid, ColNo(Path ?? ""), Page, Usage);
        for (int t = 0; t < 3; t++) {
            ushort nb = N.U16(c, 46 + 6 * t), nv = N.U16(c, 48 + 6 * t);
            if (nb > 0) {
                byte[] b = new byte[72 * nb]; N.HidP_GetButtonCaps(t, b, ref nb, p);
                for (int i = 0; i < nb; i++) {
                    int o = 72 * i; var k = new Cap(); k.Type = t; k.Button = true; k.Page = N.U16(b, o); k.Id = b[o + 2];
                    k.BitField = N.U16(b, o + 4); k.Link = N.U16(b, o + 6); k.Range = b[o + 12] != 0; k.Abs = b[o + 15] != 0;
                    k.Count = N.U16(b, o + 16); k.UMin = N.U16(b, o + 56); k.UMax = k.Range ? N.U16(b, o + 58) : k.UMin; k.BitSize = 1;
                    Caps.Add(k);
                }
            }
            if (nv > 0) {
                byte[] v = new byte[72 * nv]; N.HidP_GetValueCaps(t, v, ref nv, p);
                for (int i = 0; i < nv; i++) {
                    int o = 72 * i; var k = new Cap(); k.Type = t; k.Page = N.U16(v, o); k.Id = v[o + 2];
                    k.BitField = N.U16(v, o + 4); k.Link = N.U16(v, o + 6); k.Range = v[o + 12] != 0; k.Abs = v[o + 15] != 0;
                    k.BitSize = N.U16(v, o + 18); k.Count = N.U16(v, o + 20); k.Exp = N.I32(v, o + 32); k.Unit = N.I32(v, o + 36);
                    k.LMin = N.I32(v, o + 40); k.LMax = N.I32(v, o + 44); k.PMin = N.I32(v, o + 48); k.PMax = N.I32(v, o + 52);
                    k.UMin = N.U16(v, o + 56); k.UMax = k.Range ? N.U16(v, o + 58) : k.UMin;
                    Caps.Add(k);
                }
            }
        }
    }

    int Len(int t) { return t == 0 ? InLen : t == 1 ? OutLen : FeatLen; }

    public IEnumerable<byte> Ids(int t) { return Caps.Where(k => k.Type == t).Select(k => k.Id).Distinct().OrderBy(x => x); }

    // Keyboard page usages that reveal typed text.
    public static bool Secret(ushort page, ushort u) {
        if (page != 0x07) return false;
        return (u >= 0x04 && u <= 0x27) || (u >= 0x2C && u <= 0x38) || (u >= 0x54 && u <= 0x67) || (u >= 0x85 && u <= 0x99) || u == 0x64;
    }
    public bool ReportHasKeys(int t, byte id) { return Caps.Any(k => k.Type == t && k.Id == id && k.Page == 0x07); }

    // Decode one report: "link/PAGE:USAGE=value" for values, "link/PAGE:USAGE" for pressed buttons.
    public string Decode(int t, byte[] r) {
        if (r.Length == 0) return "";
        byte id = r[0]; var sb = new StringBuilder(); uint rl = (uint)Math.Min(r.Length, Math.Max(Len(t), 1));
        if (r.Length < rl) return "short";
        foreach (var k in Caps) {
            if (k.Type != t || k.Id != id) continue;
            if (k.Button) {
                var list = new ushort[256]; uint n = 256;
                if (N.HidP_GetUsages(t, k.Page, k.Link, list, ref n, Prep, r, rl) != N.OK) continue;
                for (int i = 0; i < n; i++) {
                    if (list[i] < k.UMin || list[i] > k.UMax) continue;
                    sb.AppendFormat(Secret(k.Page, list[i]) ? " {0}/{1:X4}:**" : " {0}/{1:X4}:{2:X4}", k.Link, k.Page, list[i]);
                }
            } else {
                for (int u = k.UMin; u <= k.UMax; u++) {
                    if (!k.Range && k.Count > 1) {
                        int bytes = (k.BitSize * k.Count + 7) / 8; var a = new byte[bytes];
                        if (N.HidP_GetUsageValueArray(t, k.Page, k.Link, (ushort)u, a, (ushort)bytes, Prep, r, rl) == N.OK)
                            sb.AppendFormat(" {0}/{1:X4}:{2:X4}=[{3}]", k.Link, k.Page, u, N.Hex(a, 64));
                    } else {
                        uint v;
                        if (N.HidP_GetUsageValue(t, k.Page, k.Link, (ushort)u, out v, Prep, r, rl) == N.OK)
                            sb.AppendFormat(" {0}/{1:X4}:{2:X4}={3}", k.Link, k.Page, u, v);
                    }
                }
            }
        }
        return sb.ToString().Trim();
    }

    // Bit position of every field, found by writing all-ones through HidP_Set* into a zeroed report.
    string Bits(int t, Cap k, ushort u) {
        int len = Len(t); if (len == 0) return "?";
        var r = new byte[len];
        // InitializeReportForID fails for some report ids of these collections; a zeroed buffer with the id works the same.
        if (N.HidP_InitializeReportForID(t, k.Id, Prep, r, (uint)len) != N.OK) { Array.Clear(r, 0, len); r[0] = k.Id; }
        var z = (byte[])r.Clone(); int st;
        if (k.Button) {
            var l = new ushort[] { u }; uint n = 1; st = N.HidP_SetUsages(t, k.Page, k.Link, l, ref n, Prep, r, (uint)len);
        } else if (!k.Range && k.Count > 1) {
            int bytes = (k.BitSize * k.Count + 7) / 8; var a = Enumerable.Repeat((byte)0xFF, bytes).ToArray();
            st = N.HidP_SetUsageValueArray(t, k.Page, k.Link, u, a, (ushort)bytes, Prep, r, (uint)len);
        } else {
            uint v = k.BitSize >= 32 ? uint.MaxValue : (1u << k.BitSize) - 1;
            st = N.HidP_SetUsageValue(t, k.Page, k.Link, u, v, Prep, r, (uint)len);
        }
        if (st != N.OK) return string.Format("st {0:X}", st);
        int first = -1, last = -1;
        for (int i = 8; i < len * 8; i++) if (((r[i / 8] ^ z[i / 8]) >> (i % 8) & 1) != 0) { if (first < 0) first = i; last = i; }
        if (first < 0) return "nochange";
        return string.Format("bit {0}..{1} (byte {2}.{3})", first - 8, last - 8, first / 8, first % 8);
    }

    public string Layout(bool product) {
        var sb = new StringBuilder();
        sb.AppendFormat("{0}\n  VID {1:X4} PID {2:X4} TLC {3:X4}:{4:X4} report bytes in {5} out {6} feat {7}\n", Mask(Path), Vid, Pid, Page, Usage, InLen, OutLen, FeatLen);
        uint nl = 64; byte[] lc = new byte[24 * nl];
        if (N.HidP_GetLinkCollectionNodes(lc, ref nl, Prep) == N.OK)
            for (int i = 0; i < nl; i++) { int o = 24 * i; sb.AppendFormat("  link[{0}] {1:X4}:{2:X4} parent {3} type {4}\n", i, N.U16(lc, o + 2), N.U16(lc, o), N.U16(lc, o + 4), lc[o + 12]); }
        for (int t = 0; t < 3; t++) foreach (byte id in Ids(t)) {
            sb.AppendFormat("  {0} {1}\n", RT[t], id);
            foreach (var k in Caps.Where(x => x.Type == t && x.Id == id)) {
                string us = k.Range ? string.Format("{0:X4}-{1:X4}", k.UMin, k.UMax) : string.Format("{0:X4}", k.UMin);
                string flags = string.Format("main {0:X3}{1}{2}", k.BitField, k.Array ? " array" : " var", k.Abs ? " abs" : " rel");
                if (k.Button) {
                    string pos = k.Array ? "array index, first slot " + Bits(t, k, k.UMax) : (k.Range ? Bits(t, k, k.UMin) + " / " + Bits(t, k, k.UMax) : Bits(t, k, k.UMin));
                    sb.AppendFormat("    btn  link {0} {1:X4}:{2} count {3} {4}  {5}\n", k.Link, k.Page, us, k.Count, flags, pos);
                } else {
                    string pos = k.Range ? Bits(t, k, k.UMin) + " / " + Bits(t, k, k.UMax) : Bits(t, k, k.UMin);
                    sb.AppendFormat("    val  link {0} {1:X4}:{2} {3}x{4} log {5}..{6} phys {7}..{8} unit {9:X} exp {10} {11}  {12}\n",
                        k.Link, k.Page, us, k.BitSize, k.Count, k.LMin, k.LMax, k.PMin, k.PMax, k.Unit, k.Exp, flags, pos);
                }
            }
        }
        return sb.ToString();
    }

    public static string Mask(string p) { return Regex.Replace(p, "(?i)(?<=[_&])[0-9a-f]{12}(?=[_&#])", "<addr>"); }
}

static class Reports {
    static readonly HashSet<string> failed = new HashSet<string>();
    static readonly object gate = new object();

    // GET one report; null on failure (remembered so a write-only feature's ~8 s timeout is paid once).
    public static byte[] Get(Coll c, int t, byte id, out string err) {
        err = null; string key = c.Path + "|" + t + "|" + id;
        lock (gate) { if (failed.Contains(key)) { err = "skipped"; return null; } }
        IntPtr h = N.CreateFileW(c.Path, 0, 3, IntPtr.Zero, 3, 0, IntPtr.Zero);
        if (h == new IntPtr(-1)) { err = "open " + Marshal.GetLastWin32Error(); return null; }
        try {
            int len = t == 2 ? c.FeatLen : c.InLen; var b = new byte[len]; b[0] = id;
            bool ok = t == 2 ? N.HidD_GetFeature(h, b, len) : N.HidD_GetInputReport(h, b, len);
            if (!ok) { err = "err " + Marshal.GetLastWin32Error(); lock (gate) failed.Add(key); return null; }
            return b;
        } finally { N.CloseHandle(h); }
    }

    // GET_INPUT is tried only for the Fn state (System Control) and backlight collections: SurfaceHidMiniDriver answers
    // every GET_INPUT_REPORT of the cover with error 121 after ~8 s, so a wider sweep only costs time.
    public static bool InputGettable(Coll c) {
        return (c.Page == 0x01 && c.Usage == 0x80) || (c.Page == 0x0C && c.Usage == 0x07);
    }

    // Session snapshots skip what cannot be read anyway (each failure is an ~8 s timeout): the CFU collection and the
    // write-only PTP configuration features (latency mode 0D:60, input mode 0D:52, surface/button switch 0D:57/58).
    public static bool SessionSkip(Coll c, byte id) {
        if (c.Page == 0xFF0B) return true;
        return c.Caps.Any(k => k.Type == 2 && k.Id == id && k.Page == 0x0D && (k.UMin == 0x60 || k.UMin == 0x52 || k.UMin == 0x57 || k.UMin == 0x58));
    }

    public static string Snapshot(List<Coll> cols, bool withInput) {
        var sb = new StringBuilder();
        foreach (var c in cols) {
            if (c.IsPen) continue;
            var lines = new StringBuilder();
            foreach (byte id in c.Ids(2)) {
                if (!withInput && SessionSkip(c, id)) continue;
                string err; var b = Get(c, 2, id, out err);
                if (b != null) { int last = b.Length; while (last > 1 && b[last - 1] == 0) last--; lines.AppendFormat("  feat {0} {1}\n", id, N.Hex(b, last)); }
                else if (err != "skipped") lines.AppendFormat("  # feat {0} {1}\n", id, err);
            }
            if (withInput && InputGettable(c)) foreach (byte id in c.Ids(0)) {
                if (c.ReportHasKeys(0, id)) continue;
                string err; var b = Get(c, 0, id, out err);
                if (b != null) { int last = b.Length; while (last > 1 && b[last - 1] == 0) last--; lines.AppendFormat("  input {0} {1}\n", id, N.Hex(b, last)); }
                else if (err != "skipped") lines.AppendFormat("  # input {0} {1}\n", id, err);
            }
            if (lines.Length > 0) { sb.AppendLine(c.Path); sb.Append(lines); }
        }
        return sb.ToString();
    }

    public static string Decoded(List<Coll> cols) {
        var sb = new StringBuilder();
        foreach (var c in cols) {
            if (c.IsPen) continue;
            sb.AppendFormat("{0}\n", c.Label);
            foreach (byte id in c.Ids(2)) {
                string err; var b = Get(c, 2, id, out err);
                if (b == null) { sb.AppendFormat("  Feat {0}: {1}\n", id, err); continue; }
                int last = b.Length; while (last > 1 && b[last - 1] == 0) last--;
                sb.AppendFormat("  Feat {0}: {1}\n      {2}\n", id, N.Hex(b, last), c.Decode(2, b));
            }
            if (InputGettable(c)) foreach (byte id in c.Ids(0)) {
                if (c.ReportHasKeys(0, id)) continue;
                string err; var b = Get(c, 0, id, out err);
                if (b == null) { sb.AppendFormat("  GetInput {0}: {1}\n", id, err); continue; }
                sb.AppendFormat("  GetInput {0}: {1}\n      {2}\n", id, N.Hex(b, b.Length), c.Decode(0, b));
            }
        }
        return sb.ToString();
    }
}

class Log : IDisposable {
    readonly StreamWriter w; readonly object gate = new object();
    public string Phase = "-";
    public Log(string path) { w = new StreamWriter(path, false, new UTF8Encoding(false)); w.WriteLine("filetime\tphase\tsrc\tdev\tkind\tdetail\thex"); }
    public void Row(string src, string dev, string kind, string detail, string hex) {
        long t = N.Now();
        lock (gate) w.WriteLine("{0}\t{1}\t{2}\t{3}\t{4}\t{5}\t{6}", t, Phase, src, dev, kind, detail, hex);
    }
    public void Flush() { lock (gate) w.Flush(); }
    public void Dispose() { lock (gate) w.Dispose(); }
}

class Step {
    public string Id, Text, Ask; public int Auto; public bool PollLight, PollScroll;
    public Step(string id, int auto, string text) { Id = id; Auto = auto; Text = text; }
}

class Session : Form {
    readonly string dir; readonly int auto;
    readonly List<Coll> kip; readonly Log log;
    readonly StreamWriter markers, answers;
    readonly Dictionary<IntPtr, Coll> rawDev = new Dictionary<IntPtr, Coll>();
    readonly List<IntPtr> readers = new List<IntPtr>();
    readonly List<Step> steps = new List<Step>();
    int cur = -1; long stepStart;
    Label title, body; ListBox list; TextBox answer;
    System.Windows.Forms.Timer tick, fast; System.Threading.Timer lightTimer;
    IntPtr hook; N.HookProc hookProc; int mouseMoves, rawMoves;
    string lastLight = ""; int lastTop = -1;
    Coll light;

    static Step S(string id, int auto, string text) { return new Step(id, auto, text); }

    public Session(string dir, int auto) {
        this.dir = dir; this.auto = auto;
        Directory.CreateDirectory(Path.Combine(dir, "features"));
        log = new Log(Path.Combine(dir, "events.tsv"));
        markers = new StreamWriter(Path.Combine(dir, "markers.csv"), false, new UTF8Encoding(false)); markers.WriteLine("filetime,phase,event"); markers.AutoFlush = true;
        answers = new StreamWriter(Path.Combine(dir, "answers.tsv"), false, new UTF8Encoding(false)); answers.WriteLine("phase\tquestion\tanswer"); answers.AutoFlush = true;
        kip = Coll.Kip();
        using (var d = new StreamWriter(Path.Combine(dir, "collections.txt"), false, new UTF8Encoding(false)))
            foreach (var c in kip) d.WriteLine("{0}\t{1}", c.Label, c.Path);
        light = kip.FirstOrDefault(c => c.Page == 0x0C && c.Usage == 0x07);

        steps.Add(S("idle", 5, "준비: 아무것도 만지지 마라. 5초 뒤 자동으로 넘어간다."));
        var s = S("fn-toggle", 0, "Fn 키만 한 번 눌렀다 떼고 2초 쉬기를 4번 해라(짝수 번이라 처음 상태로 끝난다). 누를 때마다 Fn 키 표시등이 바뀌는지 봐라.\n끝나면 Enter.");
        s.Ask = "이 단계를 시작할 때 Fn 표시등이 켜져 있었나? (1 = 켜짐, 0 = 꺼짐) 입력 후 Enter"; steps.Add(s);
        steps.Add(S("frow-1", 0, "맨 윗줄 키를 Esc 바로 오른쪽 키부터 오른쪽 끝 키까지 하나씩 천천히 눌러라(Fn은 건드리지 않는다).\n재생/일시정지 키는 누르지 마라(소리 금지). 화면 캡처·Copilot·검색 창이 뜨면 Esc나 닫기로 닫고 이 창을 다시 클릭해라.\n끝나면 Enter."));
        steps.Add(S("fn-switch", 0, "Fn 키를 한 번 눌러 표시등 상태를 바꿔라.\n끝나면 Enter."));
        steps.Add(S("frow-2", 0, "다시 맨 윗줄 키를 Esc 오른쪽부터 끝까지 하나씩 천천히 눌러라. 재생/일시정지 키는 누르지 마라.\n끝나면 Enter."));
        steps.Add(S("fn-switch-back", 0, "Fn 키를 한 번 눌러 표시등을 처음 상태로 돌려라.\n끝나면 Enter."));
        steps.Add(S("frow-held", 0, "Fn 키를 누른 채로 맨 윗줄 키를 Esc 오른쪽부터 끝까지 하나씩 눌러라. 재생/일시정지 키는 누르지 마라.\n그다음 Fn을 누른 채로 Esc를 한 번 눌러라(Fn 잠금 조합이면 표시등이 바뀐다. 바뀌었으면 Fn+Esc로 되돌린다).\n끝나면 Enter."));
        s = S("backlight-cycle", 0, "키보드 백라이트 키를 2초 간격으로 눌러 밝기 단계를 한 바퀴 돌고 처음 밝기에서 멈춰라.\n백라이트 키가 Fn 조합이면 Fn을 같이 누른다.\n끝나면 Enter.");
        s.PollLight = true; s.Ask = "백라이트 키를 몇 번 눌렀나? 입력 후 Enter"; steps.Add(s);
        s = S("backlight-timeout", 90, "백라이트가 켜져 있는지 확인하고(꺼져 있으면 백라이트 키로 켠다) 키보드와 터치패드에서 손을 떼라.\n90초 동안 아무것도 만지지 말고 백라이트가 저절로 꺼지는지 봐라. 화면은 켜진 채로 유지된다. 자동으로 넘어간다.");
        s.Ask = "백라이트가 저절로 꺼졌나? 꺼졌으면 대략 몇 초 뒤였나(안 꺼졌으면 0). 입력 후 Enter"; steps.Add(s);
        s = S("als", 0, "백라이트를 켠 상태로 두고, 화면 위쪽 카메라 옆 조도 센서를 손바닥으로 10초 가렸다가 떼고 10초 기다려라.\n끝나면 Enter.");
        s.PollLight = true; s.Ask = "센서를 가렸을 때 키보드 백라이트 밝기가 바뀌었나? (1 = 바뀜, 0 = 그대로) 입력 후 Enter"; steps.Add(s);
        s = S("scroll-edge", 0, "커서를 아래 목록 위에 두고, 두 손가락으로 아래로 스크롤하다가 손가락을 터치패드 위쪽(또는 아래쪽) 끝까지 끌고 가서 그대로 3초 멈췄다가 뗀다. 2번 해라.\n끝나면 Enter.");
        s.PollScroll = true; s.Ask = "가장자리에서 멈춰 있는 동안 목록이 계속 움직였나? (1 = 계속 움직임, 0 = 멈춤) 입력 후 Enter"; steps.Add(s);
        s = S("scroll-mid", 0, "커서를 목록 위에 두고, 두 손가락으로 스크롤하다가 터치패드 가운데에서 3초 멈췄다가 뗀다. 2번 해라.\n끝나면 Enter.");
        s.PollScroll = true; s.Ask = "가운데에서 멈춰 있는 동안 목록이 계속 움직였나? (1/0) 입력 후 Enter"; steps.Add(s);
        s = S("scroll-side", 0, "두 손가락으로 오른쪽(또는 왼쪽)으로 가로 스크롤하다가 터치패드 옆 끝에서 3초 멈췄다가 뗀다. 1번.\n끝나면 Enter.");
        s.PollScroll = true; steps.Add(s);
        steps.Add(S("swipe3", 0, "세 손가락으로 위, 아래, 왼쪽, 오른쪽으로 한 번씩 쓸어라. 화면이 바뀌면 반대 방향으로 쓸어 돌아오고 이 창을 다시 클릭해라.\n끝나면 Enter."));
        steps.Add(S("swipe4", 0, "네 손가락으로 위, 아래, 왼쪽, 오른쪽으로 한 번씩 쓸어라. 화면이 바뀌면 되돌리고 이 창을 다시 클릭해라.\n끝나면 Enter."));
        steps.Add(S("tap-click", 0, "커서를 목록 위에 두고: 한 손가락 탭 2번, 두 손가락 탭 1번, 세 손가락 탭 1번, 패드 왼쪽 아래를 눌러 클릭 1번, 오른쪽 아래를 눌러 클릭 1번, 두 번 탭한 뒤 끌기 1번.\n메뉴가 뜨면 Esc. 끝나면 Enter."));
        steps.Add(S("capslock", 0, "Caps Lock을 두 번 눌러 표시등을 켰다가 끈다.\n끝나면 Enter."));
        steps.Add(S("special", 0, "남은 특수 키를 하나씩 눌러라: 음소거, 볼륨 −, 볼륨 +, 화면 밝기 −, 화면 밝기 +(원래대로 맞춘다), Print Screen(캡처 화면은 Esc), Copilot 키(있으면, 뜬 창은 닫는다), Home, End, PgUp, PgDn, Insert, Delete, 화살표 키.\n재생/일시정지는 누르지 마라. 음소거는 다시 눌러 되돌린다. 끝나면 Enter."));
        steps.Add(S("done", 2, "끝. 잠시 뒤 창이 닫힌다."));

        Text = "Type Cover probe"; Font = new Font("Malgun Gothic", 12f); Size = new Size(1100, 760); StartPosition = FormStartPosition.CenterScreen;
        KeyPreview = true; TopMost = false;
        title = new Label { Dock = DockStyle.Top, Height = 40, Font = new Font("Malgun Gothic", 14f, FontStyle.Bold), Padding = new Padding(8) };
        body = new Label { Dock = DockStyle.Top, Height = 170, Padding = new Padding(8) };
        answer = new TextBox { Dock = DockStyle.Top, Visible = false };
        list = new ListBox { Dock = DockStyle.Fill, IntegralHeight = false };
        for (int i = 1; i <= 3000; i++) list.Items.Add(string.Format("──────── 줄 {0} ────────", i));
        Controls.Add(list); Controls.Add(answer); Controls.Add(body); Controls.Add(title);
        tick = new System.Windows.Forms.Timer { Interval = 250 }; tick.Tick += delegate { OnTick(); };
        fast = new System.Windows.Forms.Timer { Interval = 40 }; fast.Tick += delegate { OnFast(); };
    }

    protected override void OnHandleCreated(EventArgs e) {
        base.OnHandleCreated(e);
        N.SetThreadExecutionState(0x80000003); // ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED: display stays on
        ushort[,] want = { { 0x01, 0x06 }, { 0x01, 0x02 }, { 0x01, 0x80 }, { 0x0C, 0x01 }, { 0x01, 0x13 }, { 0x0C, 0x07 }, { 0x0D, 0x05 },
                           { 0xFF07, 0x01 }, { 0xFF05, 0x04 }, { 0xFF05, 0x01 } };
        for (int i = 0; i < want.GetLength(0); i++) {
            var d = new N.RAWINPUTDEVICE[1];
            d[0].usUsagePage = want[i, 0]; d[0].usUsage = want[i, 1]; d[0].dwFlags = 0x100 | 0x2000; d[0].hwndTarget = Handle; // INPUTSINK | DEVNOTIFY
            bool ok = N.RegisterRawInputDevices(d, 1, (uint)Marshal.SizeOf(typeof(N.RAWINPUTDEVICE)));
            log.Row("meta", "", "rawinput_register", string.Format("{0:X4}:{1:X4} {2}", want[i, 0], want[i, 1], ok ? "ok" : "err" + Marshal.GetLastWin32Error()), "");
        }
        hookProc = MouseHook;
        hook = N.SetWindowsHookExW(14, hookProc, N.GetModuleHandleW(null), 0); // WH_MOUSE_LL
        log.Row("meta", "", "mouse_ll_hook", hook != IntPtr.Zero ? "ok" : "err" + Marshal.GetLastWin32Error(), "");
        StartReaders();
        WriteProcs("start");
        tick.Start(); fast.Start();
        lightTimer = new System.Threading.Timer(delegate { PollLight(); }, null, 250, 250);
        BeginInvoke(new Action(Next));
    }

    void WriteProcs(string name) {
        try {
            Directory.CreateDirectory(Path.Combine(dir, "procs"));
            using (var w = new StreamWriter(Path.Combine(dir, "procs", name + ".csv"), false, new UTF8Encoding(false))) {
                w.WriteLine("\"Id\",\"ProcessName\"");
                foreach (var p in Process.GetProcesses()) w.WriteLine("\"{0}\",\"{1}\"", p.Id, p.ProcessName);
            }
        } catch { }
    }

    // ReadFile on every KIP collection Windows lets us share (keyboard, mouse and touchpad TLCs are exclusive).
    void StartReaders() {
        foreach (var c in kip) {
            if (c.IsPen || c.InLen == 0) continue;
            IntPtr h = N.CreateFileW(c.Path, 0x80000000, 3, IntPtr.Zero, 3, 0, IntPtr.Zero);
            if (h == new IntPtr(-1)) { log.Row("meta", c.Label, "reader", "open err " + Marshal.GetLastWin32Error(), ""); continue; }
            readers.Add(h); log.Row("meta", c.Label, "reader", "open ok", "");
            var col = c; var hh = h;
            var th = new Thread(() => {
                var buf = new byte[Math.Max(col.InLen, 1)];
                while (true) {
                    int n;
                    if (!N.ReadFile(hh, buf, buf.Length, out n, IntPtr.Zero)) { log.Row("meta", col.Label, "reader", "read err " + Marshal.GetLastWin32Error(), ""); return; }
                    var r = new byte[n]; Array.Copy(buf, r, n);
                    Hid("read", col, r);
                }
            });
            th.IsBackground = true; th.Start();
        }
    }

    void Hid(string src, Coll c, byte[] r) {
        string dec = c.Decode(0, r);
        string hex = r.Length > 0 && c.ReportHasKeys(0, r[0]) ? "masked" : N.Hex(r, r.Length);
        log.Row(src, c.Label, "hid " + (r.Length > 0 ? r[0].ToString() : ""), dec, hex);
    }

    IntPtr MouseHook(int code, IntPtr w, IntPtr l) {
        if (code >= 0) {
            int msg = w.ToInt32();
            if (msg == 0x0200) mouseMoves++;
            else {
                int data = Marshal.ReadInt32(l, 8), flags = Marshal.ReadInt32(l, 12);
                string name = msg == 0x020A ? "wheel" : msg == 0x020E ? "hwheel" : "msg" + msg.ToString("X");
                string d = (msg == 0x020A || msg == 0x020E) ? "delta=" + (short)(data >> 16) : "";
                log.Row("ll", "", name, (d + " flags=" + flags.ToString("X")).Trim(), "");
            }
        }
        return N.CallNextHookEx(hook, code, w, l);
    }

    static readonly Dictionary<int, string> VK = new Dictionary<int, string> {
        {0x08,"Back"},{0x09,"Tab"},{0x0D,"Enter"},{0x10,"Shift"},{0x11,"Ctrl"},{0x12,"Alt"},{0x13,"Pause"},{0x14,"CapsLock"},{0x1B,"Esc"},
        {0x21,"PgUp"},{0x22,"PgDn"},{0x23,"End"},{0x24,"Home"},{0x25,"Left"},{0x26,"Up"},{0x27,"Right"},{0x28,"Down"},{0x2C,"PrtSc"},
        {0x2D,"Ins"},{0x2E,"Del"},{0x5B,"LWin"},{0x5C,"RWin"},{0x5D,"Apps"},{0x5F,"Sleep"},{0x90,"NumLock"},{0x91,"ScrLock"},
        {0xA0,"LShift"},{0xA1,"RShift"},{0xA2,"LCtrl"},{0xA3,"RCtrl"},{0xA4,"LAlt"},{0xA5,"RAlt"},{0xA6,"BrowserBack"},{0xA7,"BrowserFwd"},
        {0xA8,"BrowserRefresh"},{0xA9,"BrowserStop"},{0xAA,"BrowserSearch"},{0xAB,"Favorites"},{0xAC,"BrowserHome"},{0xAD,"VolMute"},
        {0xAE,"VolDown"},{0xAF,"VolUp"},{0xB0,"NextTrack"},{0xB1,"PrevTrack"},{0xB2,"Stop"},{0xB3,"PlayPause"},{0xB4,"Mail"},
        {0xB5,"MediaSelect"},{0xB6,"App1"},{0xB7,"App2"},{0x15,"Kana/Hangul"},{0x19,"Hanja"},{0xFF,"none"}
    };
    // Character-producing virtual keys: letters, digits, space, keypad, OEM punctuation.
    static bool SecretVk(int vk) { return vk == 0x20 || (vk >= 0x30 && vk <= 0x39) || (vk >= 0x41 && vk <= 0x5A) || (vk >= 0x60 && vk <= 0x6F) || (vk >= 0xBA && vk <= 0xC0) || (vk >= 0xDB && vk <= 0xDF) || vk == 0xE2; }
    static string VkName(int vk) { string s; if (VK.TryGetValue(vk, out s)) return s; if (vk >= 0x70 && vk <= 0x87) return "F" + (vk - 0x6F); return "vk" + vk.ToString("X2"); }

    static string InstanceKey(string path) {
        var m = Regex.Match(path ?? "", @"(?i)hid#([^#]+)#([^#]+)#");
        return m.Success ? (m.Groups[1].Value + "#" + m.Groups[2].Value).ToLowerInvariant() : null;
    }

    Coll RawColl(IntPtr h) {
        Coll c;
        if (rawDev.TryGetValue(h, out c)) return c;
        c = new Coll(); c.Path = "?";
        if (h == IntPtr.Zero) { c.Label = "synthetic(hDevice=0)"; rawDev[h] = c; return c; }
        uint sz = 0; N.GetRawInputDeviceInfoW(h, 0x20000007, IntPtr.Zero, ref sz);
        if (sz > 0) { IntPtr b = Marshal.AllocHGlobal((int)sz * 2); if (N.GetRawInputDeviceInfoW(h, 0x20000007, b, ref sz) != unchecked((uint)-1)) c.Path = Marshal.PtrToStringUni(b); Marshal.FreeHGlobal(b); }
        // Raw Input names keyboard/mouse collections with their class interface GUID; match on the instance part.
        string key = InstanceKey(c.Path);
        var k = key == null ? null : kip.FirstOrDefault(x => InstanceKey(x.Path) == key);
        if (k != null) { rawDev[h] = k; log.Row("meta", k.Label, "rawdev", Coll.Mask(c.Path), ""); return k; }
        uint ps = 0; N.GetRawInputDeviceInfoW(h, 0x20000005, IntPtr.Zero, ref ps);
        if (ps > 0) {
            IntPtr pp = Marshal.AllocHGlobal((int)ps);
            if (N.GetRawInputDeviceInfoW(h, 0x20000005, pp, ref ps) != unchecked((uint)-1)) c.Init(pp); else Marshal.FreeHGlobal(pp);
        }
        var m = Regex.Match(c.Path, "(?i)vid[_&]?([0-9a-f]{4}).*?pid[_&]?([0-9a-f]{4})");
        string id = Regex.IsMatch(c.Path, "(?i)target_kip") ? "KIP" : m.Success ? (m.Groups[1].Value + ":" + m.Groups[2].Value).ToUpper() : Regex.Match(c.Path, @"(?i)\\\\\?\\([^#]+)#([^#]+)").Groups[2].Value;
        c.Label = string.Format("other {0} c{1} {2:X4}:{3:X4}", id, Coll.ColNo(c.Path), c.Page, c.Usage);
        rawDev[h] = c; log.Row("meta", c.Label, "rawdev", Coll.Mask(c.Path), "");
        return c;
    }

    void OnRawInput(IntPtr h) {
        uint hs = (uint)Marshal.SizeOf(typeof(N.RAWINPUTHEADER)); uint sz = 0;
        N.GetRawInputData(h, 0x10000003, IntPtr.Zero, ref sz, hs);
        if (sz == 0) return;
        IntPtr b = Marshal.AllocHGlobal((int)sz);
        try {
            if (N.GetRawInputData(h, 0x10000003, b, ref sz, hs) != sz) return;
            var hdr = (N.RAWINPUTHEADER)Marshal.PtrToStructure(b, typeof(N.RAWINPUTHEADER));
            var c = RawColl(hdr.hDevice); IntPtr body = b + (int)hs;
            if (hdr.dwType == 1) { // RAWKEYBOARD: MakeCode, Flags, Reserved, VKey, Message, ExtraInformation
                int make = (ushort)Marshal.ReadInt16(body, 0), fl = (ushort)Marshal.ReadInt16(body, 2), vk = (ushort)Marshal.ReadInt16(body, 6);
                string ud = (fl & 1) != 0 ? "up" : "down";
                if (SecretVk(vk)) log.Row("raw", c.Label, "key", "char " + ud, "");
                else log.Row("raw", c.Label, "key", string.Format("{0} {1} make={2:X2}{3}{4}", VkName(vk), ud, make, (fl & 2) != 0 ? " e0" : "", (fl & 4) != 0 ? " e1" : ""), "");
            } else if (hdr.dwType == 0) { // RAWMOUSE: usFlags, (pad), usButtonFlags, usButtonData, ulRawButtons, lLastX, lLastY
                int bf = (ushort)Marshal.ReadInt16(body, 4), bd = Marshal.ReadInt16(body, 6);
                if (bf == 0) { rawMoves++; return; }
                log.Row("raw", c.Label, "mouse", string.Format("buttons={0:X} data={1}", bf, bd), "");
            } else if (hdr.dwType == 2) {
                int each = Marshal.ReadInt32(body, 0), count = Marshal.ReadInt32(body, 4);
                for (int i = 0; i < count; i++) {
                    var rep = new byte[each]; Marshal.Copy(body + 8 + i * each, rep, 0, each);
                    if (c.Prep != IntPtr.Zero) Hid("raw", c, rep); else log.Row("raw", c.Label, "hid", "", N.Hex(rep, rep.Length));
                }
            }
        } finally { Marshal.FreeHGlobal(b); }
    }

    protected override void WndProc(ref Message m) {
        try {
            if (m.Msg == 0x00FF) OnRawInput(m.LParam);
            else if (m.Msg == 0x00FE) log.Row("meta", RawColl(m.LParam).Label, "rawdev_" + (m.WParam.ToInt32() == 1 ? "arrival" : "removal"), "", "");
        } catch (Exception ex) { log.Row("meta", "", "error", ex.Message.Replace('\t', ' '), ""); }
        base.WndProc(ref m);
    }

    void Mark(string phase, string ev) { lock (markers) markers.WriteLine("{0},{1},{2}", N.Now(), phase, ev); log.Row("meta", "", "marker", phase + " " + ev, ""); }

    void Snap(string name) {
        File.WriteAllText(Path.Combine(dir, "features", name + ".txt"), Reports.Snapshot(kip, false), new UTF8Encoding(false));
    }

    // Snapshots and polls run off the UI thread: the UI thread owns WM_INPUT and the low-level mouse hook, and Windows
    // drops a low-level hook that does not answer in time.
    bool busy;
    void Next() {
        if (busy) return;
        busy = true; pollLight = false; answer.Visible = false; asking = false;
        Step prev = cur >= 0 ? steps[cur] : null;
        cur++;
        Step s = cur < steps.Count ? steps[cur] : null;
        if (prev != null) log.Row("meta", "", "counts", string.Format("ll_mouse_moves={0} raw_mouse_moves={1}", mouseMoves, rawMoves), "");
        body.Text = "기록 준비 중… 잠깐 기다려라.";
        ThreadPool.QueueUserWorkItem(delegate {
            if (prev != null) { Snap(prev.Id + "-end"); Mark(prev.Id, "end"); log.Phase = "-"; }
            if (s != null) { Snap(s.Id + "-start"); log.Phase = s.Id; Mark(s.Id, "start"); }
            log.Flush();
            BeginInvoke(new Action(() => Show(s)));
        });
    }

    void Show(Step s) {
        mouseMoves = 0; rawMoves = 0; busy = false;
        if (s == null) { Finish(); return; }
        stepStart = Environment.TickCount; pollLight = s.PollLight;
        title.Text = string.Format("{0}/{1}  {2}", cur + 1, steps.Count, s.Id);
        body.Text = s.Text + (auto > 0 ? "\n[자동 모드: " + Math.Min(auto, s.Auto > 0 ? s.Auto : auto) + "초]" : "");
        list.Focus();
    }

    bool asking;
    void Ask() {
        var s = steps[cur];
        asking = true; body.Text = s.Ask; answer.Text = ""; answer.Visible = true; answer.Focus();
        if (auto > 0) { answers.WriteLine("{0}\t{1}\t{2}", s.Id, s.Ask.Replace('\n', ' '), "auto"); Next(); }
    }

    void Advance() {
        if (busy) return;
        var s = steps[cur];
        if (s.Ask != null && !asking) { Ask(); return; }
        if (asking) answers.WriteLine("{0}\t{1}\t{2}", s.Id, s.Ask.Replace('\n', ' '), answer.Text.Trim().Replace('\t', ' '));
        Next();
    }

    protected override void OnKeyDown(KeyEventArgs e) {
        if (e.KeyCode == Keys.Enter && cur >= 0 && cur < steps.Count && (steps[cur].Auto == 0 || asking)) { e.SuppressKeyPress = true; Advance(); return; }
        if (!asking) e.SuppressKeyPress = true; // keys only exercise the cover; nothing reaches the list
        base.OnKeyDown(e);
    }
    // Enter is a dialog key: it reaches here before OnKeyDown, so swallowing every dialog key also ate Enter.
    protected override bool ProcessDialogKey(Keys k) {
        if ((k & Keys.KeyCode) == Keys.Enter && cur >= 0 && cur < steps.Count && (steps[cur].Auto == 0 || asking)) { Advance(); return true; }
        return asking ? base.ProcessDialogKey(k) : true;
    }

    void OnTick() {
        if (busy || cur < 0 || cur >= steps.Count) return;
        var s = steps[cur];
        int el = Environment.TickCount - (int)stepStart;
        int limit = auto > 0 ? Math.Min(auto, s.Auto > 0 ? s.Auto : auto) : s.Auto;
        if (!asking && limit > 0 && el >= limit * 1000) { Advance(); return; }
        if (el % 2000 < 250) log.Flush();
    }

    volatile bool pollLight; int polling;
    void PollLight() {
        if (!pollLight || light == null || Interlocked.Exchange(ref polling, 1) == 1) return;
        try {
            string err; var b = Reports.Get(light, 2, 214, out err);
            string v = b != null ? light.Decode(2, b) : err;
            if (v != lastLight) { log.Row("poll", light.Label, "feat 214", v, b != null ? N.Hex(b, b.Length) : ""); lastLight = v; }
        } finally { polling = 0; }
    }

    void OnFast() {
        if (busy || cur < 0 || cur >= steps.Count || !steps[cur].PollScroll) return;
        int top = list.TopIndex;
        if (top != lastTop) { log.Row("ui", "", "scroll", "top=" + top, ""); lastTop = top; }
    }

    void Finish() {
        tick.Stop(); fast.Stop(); pollLight = false; if (lightTimer != null) lightTimer.Dispose();
        WriteProcs("end");
        if (hook != IntPtr.Zero) N.UnhookWindowsHookEx(hook);
        foreach (var h in readers) { N.CancelIoEx(h, IntPtr.Zero); }
        N.SetThreadExecutionState(0x80000000);
        log.Dispose(); markers.Dispose(); answers.Dispose();
        Close();
    }
}

static class Program {
    [STAThread]
    static int Main(string[] a) {
        Console.OutputEncoding = new UTF8Encoding(false);
        if (a.Length < 2) { Console.Error.WriteLine("usage: typecover-probe layout|features <out.txt> | session <dir> [--auto N]"); return 2; }
        var kip = Coll.Kip();
        if (kip.Count == 0) { Console.Error.WriteLine("no Target_KIP HID collections (cover detached?)"); return 1; }
        switch (a[0]) {
            case "layout": {
                var sb = new StringBuilder();
                foreach (var c in kip) sb.AppendLine(c.Layout(true));
                File.WriteAllText(a[1], sb.ToString(), new UTF8Encoding(false));
                Console.WriteLine("{0} collections -> {1}", kip.Count, a[1]);
                return 0;
            }
            case "features": {
                File.WriteAllText(a[1], Reports.Decoded(kip), new UTF8Encoding(false));
                Console.WriteLine("features -> {0}", a[1]);
                return 0;
            }
            case "session": {
                int auto = 0;
                for (int i = 2; i + 1 < a.Length; i++) if (a[i] == "--auto") auto = int.Parse(a[i + 1]);
                Application.EnableVisualStyles();
                Application.Run(new Session(a[1], auto));
                return 0;
            }
        }
        return 2;
    }
}
