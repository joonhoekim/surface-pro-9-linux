# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# Print HID capabilities (link collections, button caps, value caps) of every present HID collection
# whose interface path matches -Filter (regex, case-insensitive). Read-only: opens handles with no access rights.
# Collections with restrictive security (e.g. the THC heatmap and pen-bonding collections) open only elevated (run-elevated.ps1).
# Bluetooth addresses in paths are masked. Usage: powershell -ExecutionPolicy Bypass -File tools\windows\hid-caps.ps1 [-Filter 'vid_045e']
param([string]$Filter = '.')

Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class HidCaps {
  [DllImport("hid.dll")] static extern void HidD_GetHidGuid(out Guid g);
  [DllImport("hid.dll")] static extern bool HidD_GetPreparsedData(IntPtr h, out IntPtr p);
  [DllImport("hid.dll")] static extern bool HidD_FreePreparsedData(IntPtr p);
  [DllImport("hid.dll")] static extern bool HidD_GetAttributes(IntPtr h, byte[] a);
  [DllImport("hid.dll")] static extern bool HidD_GetProductString(IntPtr h, byte[] b, int len);
  [DllImport("hid.dll")] static extern int HidP_GetCaps(IntPtr p, byte[] c);
  [DllImport("hid.dll")] static extern int HidP_GetValueCaps(int t, byte[] c, ref ushort n, IntPtr p);
  [DllImport("hid.dll")] static extern int HidP_GetButtonCaps(int t, byte[] c, ref ushort n, IntPtr p);
  [DllImport("hid.dll")] static extern int HidP_GetLinkCollectionNodes(byte[] c, ref uint n, IntPtr p);
  [DllImport("setupapi.dll", CharSet=CharSet.Unicode)] static extern IntPtr SetupDiGetClassDevsW(ref Guid g, IntPtr e, IntPtr w, int f);
  [DllImport("setupapi.dll")] static extern bool SetupDiEnumDeviceInterfaces(IntPtr s, IntPtr d, ref Guid g, int i, byte[] ifd);
  [DllImport("setupapi.dll", CharSet=CharSet.Unicode)] static extern bool SetupDiGetDeviceInterfaceDetailW(IntPtr s, byte[] ifd, IntPtr det, int sz, out int req, IntPtr di);
  [DllImport("setupapi.dll")] static extern bool SetupDiDestroyDeviceInfoList(IntPtr s);
  [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern IntPtr CreateFileW(string n, uint a, uint s, IntPtr sa, uint c, uint f, IntPtr t);
  [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);

  public static List<string> Paths() {
    Guid g; HidD_GetHidGuid(out g);
    var r = new List<string>();
    IntPtr s = SetupDiGetClassDevsW(ref g, IntPtr.Zero, IntPtr.Zero, 0x12);
    for (int i = 0; ; i++) {
      byte[] ifd = new byte[32]; BitConverter.GetBytes(32).CopyTo(ifd, 0);
      if (!SetupDiEnumDeviceInterfaces(s, IntPtr.Zero, ref g, i, ifd)) break;
      int req; SetupDiGetDeviceInterfaceDetailW(s, ifd, IntPtr.Zero, 0, out req, IntPtr.Zero);
      IntPtr det = Marshal.AllocHGlobal(req); Marshal.WriteInt32(det, 8);
      if (SetupDiGetDeviceInterfaceDetailW(s, ifd, det, req, out req, IntPtr.Zero))
        r.Add(Marshal.PtrToStringUni(det + 4));
      Marshal.FreeHGlobal(det);
    }
    SetupDiDestroyDeviceInfoList(s);
    return r;
  }
  static ushort U16(byte[] b, int o) { return BitConverter.ToUInt16(b, o); }
  static int I32(byte[] b, int o) { return BitConverter.ToInt32(b, o); }
  static string[] RT = { "Input", "Output", "Feature" };

  public static string Dump(string path) {
    var sb = new StringBuilder();
    IntPtr h = CreateFileW(path, 0, 3, IntPtr.Zero, 3, 0, IntPtr.Zero);
    if (h == new IntPtr(-1)) return "  open failed, err " + Marshal.GetLastWin32Error() + "\n";
    try {
      byte[] at = new byte[12]; BitConverter.GetBytes(12).CopyTo(at, 0);
      if (HidD_GetAttributes(h, at)) sb.AppendFormat("  VID {0:X4} PID {1:X4} ver {2:X4}\n", U16(at,4), U16(at,6), U16(at,8));
      byte[] ps = new byte[256];
      if (HidD_GetProductString(h, ps, ps.Length)) sb.AppendFormat("  product \"{0}\"\n", Encoding.Unicode.GetString(ps).TrimEnd('\0'));
      IntPtr p; if (!HidD_GetPreparsedData(h, out p)) { sb.Append("  no preparsed data\n"); return sb.ToString(); }
      byte[] c = new byte[64]; HidP_GetCaps(p, c);
      sb.AppendFormat("  TLC UP {0:X4} U {1:X4}  report bytes in {2} out {3} feat {4}  links {5}\n", U16(c,2), U16(c,0), U16(c,4), U16(c,6), U16(c,8), U16(c,44));
      uint nl = U16(c,44); byte[] lc = new byte[24 * Math.Max(nl,1)];
      if (HidP_GetLinkCollectionNodes(lc, ref nl, p) == 0x110000) for (int i = 0; i < nl; i++) {
        int o = 24*i; sb.AppendFormat("  link[{0}] UP {1:X4} U {2:X4} parent {3} type {4}\n", i, U16(lc,o+2), U16(lc,o), U16(lc,o+4), lc[o+12]);
      }
      for (int t = 0; t < 3; t++) {
        ushort nb = U16(c, 46 + 6*t), nv = U16(c, 48 + 6*t);
        if (nb > 0) { byte[] b = new byte[72*nb]; HidP_GetButtonCaps(t, b, ref nb, p);
          for (int i = 0; i < nb; i++) { int o = 72*i; bool rng = b[o+12] != 0;
            sb.AppendFormat("  {0,-7} btn  id {1,3} UP {2:X4} U {3} link {4} ({5:X4}:{6:X4}) cnt {7}\n", RT[t], b[o+2], U16(b,o),
              rng ? string.Format("{0:X4}-{1:X4}", U16(b,o+56), U16(b,o+58)) : string.Format("{0:X4}", U16(b,o+56)),
              U16(b,o+6), U16(b,o+10), U16(b,o+8), U16(b,o+16)); } }
        if (nv > 0) { byte[] v = new byte[72*nv]; HidP_GetValueCaps(t, v, ref nv, p);
          for (int i = 0; i < nv; i++) { int o = 72*i; bool rng = v[o+12] != 0;
            sb.AppendFormat("  {0,-7} val  id {1,3} UP {2:X4} U {3} link {4} ({5:X4}:{6:X4}) bits {7}x{8} log {9}..{10} phys {11}..{12} unit {13:X} exp {14} {15}\n",
              RT[t], v[o+2], U16(v,o),
              rng ? string.Format("{0:X4}-{1:X4}", U16(v,o+56), U16(v,o+58)) : string.Format("{0:X4}", U16(v,o+56)),
              U16(v,o+6), U16(v,o+10), U16(v,o+8), U16(v,o+18), U16(v,o+20), I32(v,o+40), I32(v,o+44), I32(v,o+48), I32(v,o+52),
              I32(v,o+36), I32(v,o+32), v[o+15] != 0 ? "abs" : "rel"); } }
      }
      HidD_FreePreparsedData(p);
    } finally { CloseHandle(h); }
    return sb.ToString();
  }
}
'@

foreach ($path in [HidCaps]::Paths() | Where-Object { $_ -match $Filter } | Sort-Object) {
    ($path -replace '(?i)(?<=[_&])[0-9a-f]{12}(?=[_&#])', '<addr>')
    [HidCaps]::Dump($path)
}
