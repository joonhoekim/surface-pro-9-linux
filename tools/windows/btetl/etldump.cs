// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Joonhoe Kim
// Dump every event of an ETL file as one tab-separated line:
//   filetime  provider-guid  event-id  version  opcode  level  flags  pid  tid  user-data-hex
// The raw user data is kept as is (WPP messages carry their arguments without format strings),
// so a later script can look for HCI packets in it. No TMF or manifest is needed.
// Build: %WINDIR%\Microsoft.NET\Framework64\v4.0.30319\csc.exe /nologo /optimize+ /unsafe /platform:x64 /out:etldump.exe etldump.cs
// Usage: etldump.exe <in.etl> <out.tsv>
using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;

static unsafe class EtlDump
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
        public byte ProcessorNumber, Alignment; public ushort LoggerId;   // ETW_BUFFER_CONTEXT
        public ushort ExtendedDataCount, UserDataLength;
        public IntPtr ExtendedData, UserData, UserContext;
    }

    delegate void EventRecordCallback(EVENT_RECORD* r);
    delegate uint BufferCallback(IntPtr logfile);

    // EVENT_TRACE_LOGFILEW, x64 layout (only the fields we set are named).
    [StructLayout(LayoutKind.Explicit, Size = 448)]
    struct EVENT_TRACE_LOGFILEW
    {
        [FieldOffset(0)] public IntPtr LogFileName;
        [FieldOffset(8)] public IntPtr LoggerName;
        [FieldOffset(16)] public long CurrentTime;
        [FieldOffset(24)] public uint BuffersRead;
        [FieldOffset(28)] public uint ProcessTraceMode;
        [FieldOffset(400)] public IntPtr BufferCallback;
        [FieldOffset(408)] public uint BufferSize;
        [FieldOffset(412)] public uint Filled;
        [FieldOffset(416)] public uint EventsLost;
        [FieldOffset(424)] public IntPtr EventRecordCallback;
        [FieldOffset(432)] public uint IsKernelTrace;
        [FieldOffset(440)] public IntPtr Context;
    }

    [DllImport("advapi32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    static extern ulong OpenTraceW(ref EVENT_TRACE_LOGFILEW logfile);
    [DllImport("advapi32.dll")]
    static extern uint ProcessTrace(ulong[] handles, uint count, IntPtr start, IntPtr end);
    [DllImport("advapi32.dll")]
    static extern uint CloseTrace(ulong handle);

    const uint PROCESS_TRACE_MODE_EVENT_RECORD = 0x10000000, PROCESS_TRACE_MODE_RAW_TIMESTAMP = 0x1000;
    static StreamWriter w;
    static long n;
    static readonly char[] hex = "0123456789abcdef".ToCharArray();

    static void OnEvent(EVENT_RECORD* r)
    {
        var h = r->EventHeader;
        var sb = new StringBuilder(64 + r->UserDataLength * 2);
        sb.Append(h.TimeStamp).Append('\t').Append(h.ProviderId.ToString("D")).Append('\t')
          .Append(h.EventDescriptor.Id).Append('\t').Append(h.EventDescriptor.Version).Append('\t')
          .Append(h.EventDescriptor.Opcode).Append('\t').Append(h.EventDescriptor.Level).Append('\t')
          .Append(h.Flags).Append('\t').Append(h.ProcessId).Append('\t').Append(h.ThreadId).Append('\t');
        byte* p = (byte*)r->UserData;
        for (int i = 0; i < r->UserDataLength; i++) { sb.Append(hex[p[i] >> 4]); sb.Append(hex[p[i] & 15]); }
        w.WriteLine(sb.ToString());
        n++;
    }

    static uint OnBuffer(IntPtr lf) { return 1; }

    static int Main(string[] args)
    {
        if (args.Length != 2) { Console.Error.WriteLine("usage: etldump <in.etl> <out.tsv>"); return 2; }
        EventRecordCallback ecb = OnEvent; BufferCallback bcb = OnBuffer;
        var lf = new EVENT_TRACE_LOGFILEW();
        lf.LogFileName = Marshal.StringToHGlobalUni(Path.GetFullPath(args[0]));
        lf.ProcessTraceMode = PROCESS_TRACE_MODE_EVENT_RECORD;   // timestamps converted to FILETIME
        lf.EventRecordCallback = Marshal.GetFunctionPointerForDelegate(ecb);
        lf.BufferCallback = Marshal.GetFunctionPointerForDelegate(bcb);
        ulong th = OpenTraceW(ref lf);
        if (th == ulong.MaxValue) { Console.Error.WriteLine("OpenTrace failed: " + Marshal.GetLastWin32Error()); return 1; }
        using (w = new StreamWriter(args[1], false, new UTF8Encoding(false), 1 << 20))
        {
            w.WriteLine("filetime\tprovider\tid\tver\topcode\tlevel\tflags\tpid\ttid\tdata");
            uint rc = ProcessTrace(new[] { th }, 1, IntPtr.Zero, IntPtr.Zero);
            CloseTrace(th);
            Console.Error.WriteLine("events " + n + " rc " + rc);
        }
        GC.KeepAlive(ecb); GC.KeepAlive(bcb);
        return 0;
    }
}
