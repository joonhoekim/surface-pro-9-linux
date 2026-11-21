# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
"""Decode the HCI packets that BTHPORT logs into a WPR Bluetooth trace (busiotools BluetoothStack.wprp).

BTHPORT (provider Microsoft-Windows-BTH-BTHPORT, 8a1f9517-...) writes event 402 to its
"HCIRAW" channel: u8 CommandPacketsAllowed, u16 AclWriteCredits, u8 type, u32 length, bytes.
type 1 = command, 2 = event, 3 = ACL controller->host, 4 = ACL host->controller.
Windows truncates some packets (LE Start Encryption parameters, ATT writes and notifications are
cut to 11 bytes = ACL + L2CAP header + ATT opcode + handle); the summary marks those as "cut".

Input is the TSV from etldump.exe (same folder), or an .etl, which is dumped first.
Output: a text timeline (addresses masked unless --show-addr), optionally a btsnoop file
(raw bytes, keep it under private/ or out/), and the HID report writes seen by a WPP message
that carries the report ID and the writing process (seen on Windows 26200 as WPP message
cdd5026b-... id 97, presumably hidclass; the GUID is build specific).

Usage: python -I -X utf8 btetl.py <bt.etl|dump.tsv> [--tsv dump.tsv] [--btsnoop out.btsnoop]
           [--markers markers.csv] [--rawinput rawinput.csv] [--data] [--show-addr] [--t0 UNIX]
"""

import argparse
import csv
import os
import struct
import subprocess
import sys

BTHPORT = "8a1f9517-3a8c-4a9e-a018-4f17a200f277"
HID_WRITE_MSG = ("cdd5026b-79c6-3dfe-3848-cc6ee6b30910", "97")
FT_UNIX = 116444736000000000

CMDS = {
    0x0406: "Disconnect", 0x0C01: "Set Event Mask", 0x0C03: "Reset", 0x0C13: "Write Local Name",
    0x1009: "Read BD_ADDR", 0x2001: "LE Set Event Mask", 0x2005: "LE Set Random Address",
    0x200B: "LE Set Scan Parameters", 0x200C: "LE Set Scan Enable", 0x200D: "LE Create Connection",
    0x200E: "LE Create Connection Cancel", 0x2011: "LE Add Device To Filter Accept List",
    0x2012: "LE Remove Device From Filter Accept List", 0x2013: "LE Connection Update",
    0x2015: "LE Read Channel Map", 0x2016: "LE Read Remote Features", 0x2019: "LE Enable Encryption",
    0x201A: "LE LTK Request Reply", 0x2020: "LE Remote Conn Param Request Reply",
    0x2021: "LE Remote Conn Param Request Negative Reply", 0x2022: "LE Set Data Length",
    0x2027: "LE Add Device To Resolving List", 0x2028: "LE Remove Device From Resolving List",
    0x202D: "LE Set Address Resolution Enable", 0x2030: "LE Read PHY", 0x2032: "LE Set PHY",
    0x2035: "LE Set Advertising Set Random Address", 0x2036: "LE Set Extended Advertising Parameters",
    0x2037: "LE Set Extended Advertising Data", 0x2038: "LE Set Extended Scan Response Data",
    0x2039: "LE Set Extended Advertising Enable", 0x203C: "LE Remove Advertising Set",
    0x2041: "LE Set Extended Scan Parameters", 0x2042: "LE Set Extended Scan Enable",
    0x2043: "LE Extended Create Connection", 0x204E: "LE Set Privacy Mode",
}
EVENTS = {0x05: "Disconnection Complete", 0x08: "Encryption Change", 0x0E: "Command Complete",
          0x0F: "Command Status", 0x13: "Number Of Completed Packets", 0x30: "Encryption Key Refresh Complete",
          0x3E: "LE Meta", 0x57: "Authenticated Payload Timeout Expired", 0xFF: "Vendor"}
LE_SUB = {0x01: "LE Connection Complete", 0x02: "LE Advertising Report", 0x03: "LE Connection Update Complete",
          0x04: "LE Read Remote Features Complete", 0x05: "LE LTK Request", 0x06: "LE Remote Conn Param Request",
          0x07: "LE Data Length Change", 0x0A: "LE Enhanced Connection Complete", 0x0C: "LE PHY Update Complete",
          0x0D: "LE Extended Advertising Report", 0x12: "LE Advertising Set Terminated"}
REASONS = {0x08: "connection timeout", 0x13: "remote user terminated", 0x15: "remote power off",
           0x16: "local host terminated", 0x22: "LL response timeout", 0x3B: "unacceptable conn params",
           0x3D: "MIC failure", 0x3E: "failed to establish"}
ATT = {0x01: "Error Rsp", 0x02: "Exchange MTU Req", 0x03: "Exchange MTU Rsp", 0x04: "Find Info Req",
       0x05: "Find Info Rsp", 0x08: "Read By Type Req", 0x09: "Read By Type Rsp", 0x0A: "Read Req",
       0x0B: "Read Rsp", 0x0C: "Read Blob Req", 0x0D: "Read Blob Rsp", 0x10: "Read By Group Type Req",
       0x11: "Read By Group Type Rsp", 0x12: "Write Req", 0x13: "Write Rsp", 0x1B: "Notification",
       0x1D: "Indication", 0x1E: "Confirmation", 0x52: "Write Cmd"}
L2CAP_SIG = {0x01: "Command Reject", 0x12: "Conn Param Update Req", 0x13: "Conn Param Update Rsp"}


def addr(b, show):
    a = ":".join("%02X" % x for x in reversed(b))
    return a if show else "xx:xx:xx:xx:xx:" + a[-2:]


def ms125(v):
    return "%g ms" % (v * 1.25)


def load_tsv(path):
    """Yield (filetime, provider, id, pid, data bytes) for the events we use."""
    with open(path, encoding="utf-8") as f:
        next(f)
        for line in f:
            p = line.rstrip("\n").split("\t")
            if len(p) < 10:
                continue
            if p[1] == BTHPORT and p[2] == "402" or (p[1], p[2]) == HID_WRITE_MSG:
                yield int(p[0]), p[1], p[2], int(p[7]), bytes.fromhex(p[9])


def dump_etl(etl, tsv):
    exe = os.path.join(os.path.dirname(os.path.abspath(tsv)), "etldump.exe")
    if not os.path.exists(exe):
        src = os.path.join(os.path.dirname(os.path.abspath(__file__)), "etldump.cs")
        csc = os.path.join(os.environ["WINDIR"], r"Microsoft.NET\Framework64\v4.0.30319\csc.exe")
        subprocess.check_call([csc, "/nologo", "/optimize+", "/unsafe", "/platform:x64", "/out:" + exe, src])
    subprocess.check_call([exe, etl, tsv])


class Decoder:
    def __init__(self, show):
        self.show = show
        self.pending = {}   # opcode -> name, for Command Complete/Status

    def command(self, d):
        op, plen = struct.unpack_from("<HB", d)
        p = d[3:]
        name = CMDS.get(op, "Vendor 0x%04x" % op if op >> 10 == 0x3F else "0x%04x" % op)
        cut = " (params cut: %d of %d B)" % (len(p), plen) if len(p) < plen else ""
        s = "%s%s" % (name, cut)
        if op == 0x2043 and len(p) >= 26:
            filt, own, ptype = p[0], p[1], p[2]
            scan_i, scan_w, cmin, cmax, lat, to = struct.unpack_from("<HHHHHH", p, 10)
            s += (": filter=%d own=%d peer type %d %s, scan %s/%s, interval %s..%s, latency %d, timeout %d ms"
                  % (filt, own, ptype, addr(p[3:9], self.show), ms125(scan_i / 2), ms125(scan_w / 2),
                     ms125(cmin), ms125(cmax), lat, to * 10))
        elif op == 0x2013 and len(p) >= 14:
            h, cmin, cmax, lat, to = struct.unpack_from("<HHHHH", p)
            s += ": handle 0x%03x interval %s..%s latency %d timeout %d ms" % (h, ms125(cmin), ms125(cmax), lat, to * 10)
        elif op in (0x2011, 0x2012) and len(p) >= 7:
            s += ": type %d %s" % (p[0], addr(p[1:7], self.show))
        elif op == 0x2042 and len(p) >= 2:
            s += ": enable=%d filter_dup=%d" % (p[0], p[1])
        elif op == 0x2041 and len(p) >= 3:
            s += ": own=%d filter policy=%d phys=0x%x" % (p[0], p[1], p[2])
            off = 3
            for _ in range(bin(p[2]).count("1")):
                if len(p) >= off + 5:
                    t, i, w = struct.unpack_from("<BHH", p, off)
                    s += " [%s interval %s window %s]" % ("active" if t else "passive", ms125(i / 2), ms125(w / 2))
                    off += 5
        elif op == 0x0406 and len(p) >= 3:
            s += ": handle 0x%03x reason 0x%02x %s" % (p[0] | p[1] << 8, p[2], REASONS.get(p[2], ""))
        elif op == 0x0C13 or op >> 10 == 0x3F or op in (0x2005, 0x2035, 0x2037, 0x2027):
            s += " [%d B params not shown]" % len(p)
        else:
            s += (": " + p.hex()) if p else ""
        return s

    def event(self, d):
        code, plen = d[0], d[1]
        p = d[2:]
        s = EVENTS.get(code, "Event 0x%02x" % code)
        if code == 0x0E and len(p) >= 4:
            op = p[1] | p[2] << 8
            s += ": %s status 0x%02x" % (CMDS.get(op, "0x%04x" % op), p[3])
        elif code == 0x0F and len(p) >= 4:
            op = p[2] | p[3] << 8
            s += ": %s status 0x%02x" % (CMDS.get(op, "0x%04x" % op), p[0])
        elif code == 0x05 and len(p) >= 4:
            s += ": status 0x%02x handle 0x%03x reason 0x%02x %s" % (p[0], p[1] | p[2] << 8, p[3], REASONS.get(p[3], ""))
        elif code == 0x08 and len(p) >= 4:
            s += ": status 0x%02x handle 0x%03x enabled %d" % (p[0], p[1] | p[2] << 8, p[3])
        elif code == 0x13 and len(p) >= 5:
            s += ": handle 0x%03x n=%d" % (p[1] | p[2] << 8, p[3] | p[4] << 8)
        elif code == 0x3E and p:
            sub = p[0]
            s = LE_SUB.get(sub, "LE Meta 0x%02x" % sub)
            q = p[1:]
            if sub in (0x01, 0x0A) and len(q) >= 18:
                st, h, role, ptype = q[0], q[1] | q[2] << 8, q[3], q[4]
                o = 11 if sub == 0x01 else 23
                iv, lat, to = struct.unpack_from("<HHH", q, o)
                s += (": status 0x%02x handle 0x%03x role %s peer type %d %s interval %s latency %d timeout %d ms"
                      % (st, h, "central" if role == 0 else "peripheral", ptype, addr(q[5:11], self.show),
                         ms125(iv), lat, to * 10))
            elif sub == 0x03 and len(q) >= 9:
                st, h, iv, lat, to = struct.unpack_from("<BHHHH", q)
                s += ": status 0x%02x handle 0x%03x interval %s latency %d timeout %d ms" % (st, h, ms125(iv), lat, to * 10)
            elif sub == 0x06 and len(q) >= 10:
                h, cmin, cmax, lat, to = struct.unpack_from("<HHHHH", q)
                s += ": handle 0x%03x interval %s..%s latency %d timeout %d ms" % (h, ms125(cmin), ms125(cmax), lat, to * 10)
            elif sub == 0x0D and len(q) >= 25:
                et = q[1] | q[2] << 8
                rssi = q[14] - 256 if q[14] > 127 else q[14]
                s += ": type 0x%04x addr type %d %s rssi %d dBm data %d B" % (et, q[3], addr(q[4:10], self.show), rssi, q[24])
            elif sub == 0x04 and len(q) >= 11:
                s += ": status 0x%02x handle 0x%03x features %s" % (q[0], q[1] | q[2] << 8, q[3:11].hex())
            elif sub == 0x07 and len(q) >= 10:
                h, tx, txt, rx, rxt = struct.unpack_from("<HHHHH", q)
                s += ": handle 0x%03x tx %d B/%d us rx %d B/%d us" % (h, tx, txt, rx, rxt)
            elif sub == 0x0C and len(q) >= 5:
                s += ": status 0x%02x handle 0x%03x tx phy %d rx phy %d" % (q[0], q[1] | q[2] << 8, q[3], q[4])
            elif sub == 0x05:
                s += " [params not shown]"
        elif code == 0xFF:
            s += " [%d B not shown]" % len(p)
        return s

    def acl(self, d, outgoing):
        hf, alen = struct.unpack_from("<HH", d)
        h, pb = hf & 0xFFF, hf >> 12 & 3
        p = d[4:]
        cut = len(p) < alen
        s = "%s handle 0x%03x" % ("host->pen" if outgoing else "pen->host", h)
        if pb == 1 or len(p) < 4:
            return s + " continuation %d B" % alen, None
        llen, cid = struct.unpack_from("<HH", p)
        body = p[4:]
        info = None
        if cid == 4 and body:
            op = body[0]
            s += " ATT %s" % ATT.get(op, "0x%02x" % op)
            if op in (0x0A, 0x12, 0x52, 0x1B, 0x1D, 0x0C) and len(body) >= 3:
                handle = body[1] | body[2] << 8
                s += " 0x%04x" % handle
                vlen = llen - 3
                s += " value %d B" % vlen
                info = (op, handle, vlen, body[3:])
            elif op in (0x02, 0x03) and len(body) >= 3:
                s += " MTU %d" % (body[1] | body[2] << 8)
            elif op == 0x0B:
                s += " value %d B" % (llen - 1)
                info = (op, None, llen - 1, body[1:])
            elif op == 0x01 and len(body) >= 5:
                s += " req 0x%02x handle 0x%04x error 0x%02x" % (body[1], body[2] | body[3] << 8, body[4])
        elif cid == 5 and body:
            s += " L2CAP signaling %s" % L2CAP_SIG.get(body[0], "0x%02x" % body[0])
            if body[0] == 0x12 and len(body) >= 12:
                cmin, cmax, lat, to = struct.unpack_from("<HHHH", body, 4)
                s += ": interval %s..%s latency %d timeout %d ms" % (ms125(cmin), ms125(cmax), lat, to * 10)
            elif body[0] == 0x13 and len(body) >= 6:
                s += ": result %d" % (body[4] | body[5] << 8)
        elif cid == 6:
            s += " SMP 0x%02x [not shown]" % body[0] if body else " SMP"
        else:
            s += " CID 0x%04x %d B" % (cid, llen)
        if cut:
            s += " (cut: %d of %d B logged)" % (len(p), alen)
        return s, info


def btsnoop_writer(path):
    f = open(path, "wb")
    f.write(b"btsnoop\0" + struct.pack(">II", 1, 1002))   # H4

    def write(ft, typ, data):
        h4 = {1: 0x01, 2: 0x04, 3: 0x02, 4: 0x02}[typ]
        flags = (1 if typ in (2, 3) else 0) | (2 if typ in (1, 2) else 0)
        pkt = bytes([h4]) + data
        # btsnoop time: microseconds since 0000-01-01; the unix epoch is 0x00dcddb30f2f8000
        us = (ft - FT_UNIX) // 10 + 0x00DCDDB30F2F8000
        f.write(struct.pack(">IIIIq", len(pkt), len(pkt), flags, 0, us) + pkt)
    return f, write


def load_markers(path):
    out = []
    if path and os.path.exists(path):
        with open(path, encoding="utf-8-sig") as f:
            for r in csv.DictReader(f):
                out.append((float(r["t"]), r["step"]))
    return out


def tip_spans(path, gap=0.5):
    """Pen-in-contact spans from sp9probe rawinput.csv (HEAT pen device, 'tip' in decoded buttons)."""
    spans = []
    if not path or not os.path.exists(path):
        return spans
    with open(path, encoding="utf-8-sig") as f:
        r = csv.reader(f)
        next(r)
        for row in r:
            if len(row) < 6 or row[1] != "0" or "tip" not in row[5].split()[0]:
                continue
            t = float(row[0])
            if spans and t - spans[-1][1] <= gap:
                spans[-1][1] = t
            else:
                spans.append([t, t])
    return spans


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("src", help="bt.etl or etldump TSV")
    ap.add_argument("--tsv", help="where to put the dump of an .etl (default: next to --btsnoop or cwd)")
    ap.add_argument("--btsnoop", help="write raw HCI as btsnoop (H4); contains addresses, keep it private")
    ap.add_argument("--markers", help="markers.csv of the baseline run (step boundaries)")
    ap.add_argument("--rawinput", help="probe/rawinput.csv of the run (pen contact spans)")
    ap.add_argument("--show-addr", action="store_true")
    ap.add_argument("--data", action="store_true", help="print ATT Read Rsp values (may hold pen serials)")
    ap.add_argument("--t0", type=float, help="print times relative to this unix time (default: first marker)")
    a = ap.parse_args(argv)

    src = a.src
    if src.lower().endswith(".etl"):
        tsv = a.tsv or os.path.splitext(a.btsnoop or os.path.basename(src))[0] + ".tsv"
        dump_etl(src, tsv)
        src = tsv
    markers = load_markers(a.markers)
    t0 = a.t0 if a.t0 is not None else (markers[0][0] if markers else None)
    spans = tip_spans(a.rawinput)
    dec = Decoder(a.show_addr)
    snoop = btsnoop_writer(a.btsnoop) if a.btsnoop else None
    rows, last = [], None
    for ft, prov, eid, pid, d in load_tsv(src):
        t = (ft - FT_UNIX) / 1e7
        if prov == BTHPORT:
            if len(d) < 8:
                continue
            typ, n = d[3], struct.unpack_from("<I", d, 4)[0]
            pkt = d[8:8 + n]
            if (typ, pkt) == last and rows and t - rows[-1][0] < 0.001:
                continue   # BTHPORT and BTHPORT.WithStacks log the same packet twice
            last = (typ, pkt)
            if snoop:
                snoop[1](ft, typ, pkt)
            if typ == 1:
                s = "CMD  " + dec.command(pkt)
            elif typ == 2:
                s = "EVT  " + dec.event(pkt)
            else:
                text, info = dec.acl(pkt, typ == 4)
                s = "ACL  " + text
                if a.data and info and info[0] == 0x0B and info[3]:
                    s += "\n       data " + info[3].hex()
            rows.append((t, s))
        else:
            rid = d[-8] if len(d) >= 8 else -1
            rows.append((t, "HID  report write id %d by pid %d (WPP %s/%s)" % (rid, pid, prov[:8], eid)))
    for t, step in markers:
        rows.append((t, "---- marker step %s" % step))
    for s, e in spans:
        rows.append((s, "PEN  tip down (%.2f s)" % (e - s)))
    rows.sort(key=lambda r: r[0])
    for t, s in rows:
        print("%10.3f  %s" % (t - t0 if t0 else t, s))
    if snoop:
        snoop[0].close()


if __name__ == "__main__":
    sys.exit(main())
