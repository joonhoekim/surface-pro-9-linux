// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Joonhoe Kim
// Decode an ETL file offline with TDH (manifest, TraceLogging and registered MOF events) into one TSV line per event:
//   utc-time  provider-guid  provider-name  id  version  task  opcode  event-name  properties
// properties are "Name=value" joined by " ; ". Events TDH cannot decode (WPP without TMF, unknown schemas) keep their
// raw payload as "raw=<hex>". Structs and arrays of structs are not expanded; the rest of such an event is "rest=<hex>".
// Optional provider filter: comma-separated GUIDs; only those providers are written.
// Build: %WINDIR%\Microsoft.NET\Framework64\v4.0.30319\csc.exe /nologo /optimize+ /unsafe /platform:x64 /out:etwdecode.exe etwdecode.cs
// Usage: etwdecode.exe <in.etl> <out.tsv> [guid,guid,...]
using System;
using System.Collections.Generic;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;

static unsafe class EtwDecode
{
    [StructLayout(LayoutKind.Sequential)]
    struct EVENT_DESCRIPTOR { public ushort Id; public byte Version, Channel, Level, Opcode; public ushort Task; public ulong Keyword; }

    [StructLayout(LayoutKind.Sequential)]
    struct EVENT_HEADER
    {
        public ushort Size, HeaderType, Flags, EventProperty;
        public uint ThreadId, ProcessId;
        public long TimeStamp;
        public Guid ProviderId;
        public EVENT_DESCRIPTOR EventDescriptor;
        public ulong ProcessorTime;
        public Guid ActivityId;
    }

    [StructLayout(LayoutKind.Sequential)]
    struct EVENT_RECORD
    {
        public EVENT_HEADER EventHeader;
        public byte ProcessorNumber, Alignment; public ushort LoggerId;
        public ushort ExtendedDataCount, UserDataLength;
        public IntPtr ExtendedData, UserData, UserContext;
    }

    delegate void EventRecordCallback(EVENT_RECORD* r);
    delegate uint BufferCallback(IntPtr logfile);

    [StructLayout(LayoutKind.Explicit, Size = 448)]
    struct EVENT_TRACE_LOGFILEW
    {
        [FieldOffset(0)] public IntPtr LogFileName;
        [FieldOffset(28)] public uint ProcessTraceMode;
        [FieldOffset(400)] public IntPtr BufferCallback;
        [FieldOffset(424)] public IntPtr EventRecordCallback;
    }

    [DllImport("advapi32.dll", CharSet = CharSet.Unicode, SetLastError = true)] static extern ulong OpenTraceW(ref EVENT_TRACE_LOGFILEW logfile);
    [DllImport("advapi32.dll")] static extern uint ProcessTrace(ulong[] handles, uint count, IntPtr start, IntPtr end);
    [DllImport("advapi32.dll")] static extern uint CloseTrace(ulong handle);
    [DllImport("tdh.dll")] static extern uint TdhGetEventInformation(EVENT_RECORD* r, uint tdhCount, IntPtr ctx, byte* info, ref uint size);
    [DllImport("tdh.dll")] static extern uint TdhFormatProperty(byte* info, IntPtr mapInfo, uint pointerSize, ushort inType, ushort outType,
        ushort propLength, ushort userDataLength, byte* userData, ref uint bufferSize, char* buffer, out ushort consumed);

    const uint PROCESS_TRACE_MODE_EVENT_RECORD = 0x10000000;
    const uint ERROR_INSUFFICIENT_BUFFER = 122;
    static StreamWriter w;
    static long n, written, undecoded;
    static HashSet<Guid> filter;
    static byte[] infoBuf = new byte[1 << 16];
    static char[] fmtBuf = new char[1 << 15];
    static readonly char[] hex = "0123456789abcdef".ToCharArray();

    static string Hex(byte* p, int len)
    {
        var sb = new StringBuilder(len * 2);
        for (int i = 0; i < len; i++) { sb.Append(hex[p[i] >> 4]); sb.Append(hex[p[i] & 15]); }
        return sb.ToString();
    }

    static string Str(byte* info, uint off) { return off == 0 ? "" : new string((char*)(info + off)).Trim(); }

    static string Clean(string s) { return s.Replace('\t', ' ').Replace('\r', ' ').Replace('\n', ' '); }

    static void OnEvent(EVENT_RECORD* r)
    {
        n++;
        var h = r->EventHeader;
        if (filter != null && !filter.Contains(h.ProviderId)) return;
        string time = DateTime.FromFileTimeUtc(h.TimeStamp).ToString("yyyy-MM-ddTHH:mm:ss.fffffffZ");
        byte* ud = (byte*)r->UserData; int udLen = r->UserDataLength;
        string pname = "", task = "", opcode = "", ename = "", props;
        uint size = (uint)infoBuf.Length;
        uint rc;
        fixed (byte* ib0 = infoBuf) rc = TdhGetEventInformation(r, 0, IntPtr.Zero, ib0, ref size);
        if (rc == ERROR_INSUFFICIENT_BUFFER) { infoBuf = new byte[size]; fixed (byte* ib1 = infoBuf) rc = TdhGetEventInformation(r, 0, IntPtr.Zero, ib1, ref size); }
        if (rc != 0) { undecoded++; props = "raw=" + Hex(ud, udLen); }
        else fixed (byte* info = infoBuf)
        {
            pname = Str(info, *(uint*)(info + 52));
            task = Str(info, *(uint*)(info + 68));
            opcode = Str(info, *(uint*)(info + 72));
            ename = Str(info, *(uint*)(info + 92));
            uint topCount = *(uint*)(info + 104);
            uint ptrSize = (h.Flags & 0x20) != 0 ? 4u : 8u;   // EVENT_HEADER_FLAG_32_BIT_HEADER
            var sb = new StringBuilder();
            var ints = new long[topCount];
            int pos = 0;
            for (uint i = 0; i < topCount; i++)
            {
                byte* pi = info + 112 + 24 * i;
                uint flags = *(uint*)pi;
                string name = Str(info, *(uint*)(pi + 4));
                ushort inType = *(ushort*)(pi + 8), outType = *(ushort*)(pi + 10);
                ushort count = *(ushort*)(pi + 16), length = *(ushort*)(pi + 18);
                if ((flags & 0x1) != 0) { if (sb.Length > 0) sb.Append(" ; "); sb.Append(name).Append("=rest:").Append(Hex(ud + pos, udLen - pos)); pos = udLen; break; }   // struct
                if ((flags & 0x4) != 0) count = (ushort)(count < topCount ? ints[count] : 0);    // count from another property
                if ((flags & 0x2) != 0) length = (ushort)(length < topCount ? ints[length] : 0); // length from another property
                if ((flags & 0x24) == 0) count = 1;
                var vals = new List<string>();
                for (int k = 0; k < count && pos <= udLen; k++)
                {
                    if (inType == 0x0e /* BINARY */ && length == 0 && (flags & 0x2) == 0) { vals.Add(Hex(ud + pos, udLen - pos)); pos = udLen; break; }
                    uint bs = (uint)fmtBuf.Length; ushort used;
                    uint frc;
                    fixed (char* fb = fmtBuf) frc = TdhFormatProperty(info, IntPtr.Zero, ptrSize, inType, outType, length, (ushort)(udLen - pos), ud + pos, ref bs, fb, out used);
                    if (frc == ERROR_INSUFFICIENT_BUFFER) { fmtBuf = new char[bs]; fixed (char* fb = fmtBuf) frc = TdhFormatProperty(info, IntPtr.Zero, ptrSize, inType, outType, length, (ushort)(udLen - pos), ud + pos, ref bs, fb, out used); }
                    if (frc != 0) { vals.Add("rest:" + Hex(ud + pos, udLen - pos)); pos = udLen; break; }
                    string v; fixed (char* fb = fmtBuf) v = new string(fb);
                    if (k == 0 && count == 1)
                    {
                        long iv;
                        if (used <= 8 && used > 0 && inType >= 2 && inType <= 9) { iv = 0; for (int b = used - 1; b >= 0; b--) iv = (iv << 8) | ud[pos + b]; ints[i] = iv; }
                    }
                    vals.Add(v);
                    pos += used;
                }
                if (sb.Length > 0) sb.Append(" ; ");
                sb.Append(name).Append('=').Append(count == 1 ? (vals.Count > 0 ? vals[0] : "") : "[" + string.Join(",", vals.ToArray()) + "]");
            }
            props = sb.ToString();
        }
        w.Write(time); w.Write('\t'); w.Write(h.ProviderId.ToString("D")); w.Write('\t'); w.Write(Clean(pname)); w.Write('\t');
        w.Write(h.EventDescriptor.Id); w.Write('\t'); w.Write(h.EventDescriptor.Version); w.Write('\t');
        w.Write(Clean(task)); w.Write('\t'); w.Write(Clean(opcode)); w.Write('\t'); w.Write(Clean(ename)); w.Write('\t');
        w.WriteLine(Clean(props));
        written++;
    }

    static uint OnBuffer(IntPtr lf) { return 1; }

    static int Main(string[] args)
    {
        if (args.Length < 2) { Console.Error.WriteLine("usage: etwdecode <in.etl> <out.tsv> [guid,guid,...]"); return 2; }
        if (args.Length > 2)
        {
            filter = new HashSet<Guid>();
            foreach (var g in args[2].Split(',')) if (g.Trim().Length > 0) filter.Add(new Guid(g.Trim()));
        }
        EventRecordCallback ecb = OnEvent; BufferCallback bcb = OnBuffer;
        var lf = new EVENT_TRACE_LOGFILEW();
        lf.LogFileName = Marshal.StringToHGlobalUni(Path.GetFullPath(args[0]));
        lf.ProcessTraceMode = PROCESS_TRACE_MODE_EVENT_RECORD;
        lf.EventRecordCallback = Marshal.GetFunctionPointerForDelegate(ecb);
        lf.BufferCallback = Marshal.GetFunctionPointerForDelegate(bcb);
        ulong th = OpenTraceW(ref lf);
        if (th == ulong.MaxValue) { Console.Error.WriteLine("OpenTrace failed: " + Marshal.GetLastWin32Error()); return 1; }
        using (w = new StreamWriter(args[1], false, new UTF8Encoding(false), 1 << 20))
        {
            w.WriteLine("time\tprovider\tprovider_name\tid\tver\ttask\topcode\tevent\tprops");
            uint rc = ProcessTrace(new[] { th }, 1, IntPtr.Zero, IntPtr.Zero);
            CloseTrace(th);
            Console.Error.WriteLine("events " + n + " written " + written + " undecoded " + undecoded + " rc " + rc);
        }
        GC.KeepAlive(ecb); GC.KeepAlive(bcb);
        return 0;
    }
}
