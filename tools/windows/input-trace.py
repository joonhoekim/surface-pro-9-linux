# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
"""Decode an input-trace.ps1 run: which HID requests (SET/GET feature, output, write) went to which collection in
which phase, the THC DMA writes, and how the read-only feature snapshots changed between phases.

    python -I -X utf8 tools/windows/input-trace.py <run dir>

Needs <run>/input.etl, markers.csv, features/*.txt, procs/*.csv. Writes <run>/input.tsv (etldump),
<run>/requests.tsv and <run>/summary.txt. No TMF is needed:
  - HIDCLASS WPP rundown (one event per device: FDO pointer + instance path; one per collection: FDO, collection
    number, usage page, usage, PDO-extension pointer) maps pointers to collections.
  - Per-request HIDCLASS WPP events carry <FDO ptr><u32 collection><PDO ptr><u32 IOCTL><u32 report id>.
    Found by pattern, because WPP message ids change between Windows builds.
  - IntelTHCBase events 101 (DMA read) / 102 (DMA write): <u32 type><u32 length>...; event 100 is the function name.
    Types follow the THC legacy protocol (ithc: 3 = SET_FEATURE, 4 = GET_FEATURE, 5 = OUTPUT).
Payload bytes of SET requests are not in any of these events.
"""
import csv
import glob
import os
import re
import struct
import subprocess
import sys
from collections import Counter, defaultdict

IOCTLS = {0xB0191: "SET_FEATURE", 0xB0192: "GET_FEATURE", 0xB0195: "SET_OUTPUT", 0xB01A2: "GET_INPUT",
          0xB000F: "WRITE_REPORT", 0xB002B: "IDLE_NOTIFY", 0xB001F: "ACTIVATE", 0xB0023: "DEACTIVATE"}
THC = "937ad94e-ca8d-4b8e-8143-3fce4accb8cb"
THC_TX = {3: "SET_FEATURE", 4: "GET_FEATURE", 5: "OUTPUT"}
THC_RX = {3: "INPUT", 4: "FEATURE", 5: "DESCRIPTOR", 7: "RESET"}
ADDR = re.compile(r"(?i)(?<=[_&])[0-9a-f]{12}(?=[_&#\\]|$)")


def kptr(b):
    return len(b) == 8 and b[6:8] == b"\xff\xff"


def etldump(run):
    tsv = os.path.join(run, "input.tsv")
    if os.path.exists(tsv):
        return tsv
    here = os.path.dirname(os.path.abspath(__file__))
    outdir = os.path.join(here, "..", "..", "out", "input-trace")
    os.makedirs(outdir, exist_ok=True)
    exe = os.path.join(outdir, "etldump.exe")
    if not os.path.exists(exe):
        csc = os.path.join(os.environ["WINDIR"], r"Microsoft.NET\Framework64\v4.0.30319\csc.exe")
        subprocess.check_call([csc, "/nologo", "/optimize+", "/unsafe", "/platform:x64", "/out:" + exe,
                               os.path.join(here, "btetl", "etldump.cs")])
    subprocess.check_call([exe, os.path.join(run, "input.etl"), tsv])
    return tsv


def load_markers(run):
    rows = list(csv.DictReader(open(os.path.join(run, "markers.csv"), encoding="utf-8-sig")))
    spans, starts = [], {}
    for r in rows:
        t = int(r["filetime"])
        if r["event"] == "start":
            starts[r["phase"]] = t
        elif r["event"] == "end" and r["phase"] in starts:
            spans.append((starts.pop(r["phase"]), t, r["phase"]))
    return rows, spans


def phase_of(t, spans):
    for a, b, p in spans:
        if a <= t <= b:
            return p
    return "-"


def procs(run):
    m = {}
    for f in glob.glob(os.path.join(run, "procs", "*.csv")):
        for r in csv.DictReader(open(f, encoding="utf-8-sig")):
            m.setdefault(r["Id"], r["ProcessName"])
    m.setdefault("4", "System")
    return m


def feature_snapshots(run, rows):
    order = []
    for r in rows:
        if r["event"] in ("start", "end", "mid"):
            name = f"{r['phase']}-{r['event']}"
            if os.path.exists(os.path.join(run, "features", name + ".txt")):
                order.append(name)
    vals = defaultdict(dict)  # (collection, id) -> snapshot -> hex
    for name in order:
        coll = None
        for line in open(os.path.join(run, "features", name + ".txt"), encoding="utf-8-sig"):
            line = line.rstrip()
            if line.startswith("\\\\?\\"):
                m = re.search(r"(?i)hid#([^#]+)#([^#]+)#", line)
                coll = f"{m.group(1)}#{m.group(2)}" if m else line
            elif line.strip().startswith("feat ") and coll:
                _, rid, val = line.split(None, 2)
                vals[(coll, int(rid))][name] = val
    return order, vals


def main():
    run = sys.argv[1]
    tsv = etldump(run)
    rows, spans = load_markers(run)
    pname = procs(run)
    dev, col = {}, {}
    reqs, thc_counts, thc_writes = [], Counter(), []
    last_fn = ""
    for e in csv.DictReader(open(tsv, encoding="utf-8"), delimiter="\t"):
        d = bytes.fromhex(e["data"])
        t = int(e["filetime"])
        prov = e["provider"]
        if prov == THC:
            if e["id"] == "100":
                last_fn = d.split(b"\0")[0].decode("ascii", "replace")
            elif e["id"] in ("101", "102") and len(d) >= 8:
                typ, ln = struct.unpack_from("<II", d)
                direction = "write" if e["id"] == "102" else "read"
                name = (THC_TX if direction == "write" else THC_RX).get(typ, f"type{typ}")
                thc_counts[(phase_of(t, spans), direction, name)] += 1
                if direction == "write" or typ not in (3, 4):
                    thc_writes.append((t, phase_of(t, spans), direction + " " + name, ln, e["pid"], last_fn))
            continue
        # HIDCLASS WPP rundown: device and collection maps.
        if len(d) > 16 and kptr(d[:8]) and b"\\\x00" in d[8:]:
            s = d[8:].decode("utf-16le", "replace").split("\0")[0]
            if "\\" in s and ("HID" in s.upper() or "VHF" in s or "PCI\\" in s or "BTH" in s.upper() or "\\" in s[:60]):
                dev.setdefault(d[:8], ADDR.sub("<addr>", s))
                continue
        if len(d) >= 28 and e["id"] != "21" and kptr(d[:8]) and kptr(d[20:28]) and d[14:16] == b"\0\0" and d[18:20] == b"\0\0":
            n, up, us = struct.unpack_from("<III", d, 8)
            col[(d[:8], n)] = (up, us)
            continue
        # Per-request events: <fdo><u32 col><pdo><ioctl><rid>.
        for o in range(20, len(d) - 3):
            code = struct.unpack_from("<I", d, o)[0]
            if code in IOCTLS and kptr(d[o - 20:o - 12]) and kptr(d[o - 8:o]):
                n = struct.unpack_from("<I", d, o - 12)[0]
                rid = struct.unpack_from("<I", d, o + 4)[0] if o + 8 <= len(d) else -1
                reqs.append((t, phase_of(t, spans), IOCTLS[code], d[o - 20:o - 12], n, rid, e["pid"]))
                break

    def cname(fdo, n):
        up, us = col.get((fdo, n), (None, None))
        u = f"{up:04X}:{us:04X}" if up is not None else "?"
        return f"{dev.get(fdo, fdo.hex())} col{n:02X} {u}"

    with open(os.path.join(run, "requests.tsv"), "w", encoding="utf-8") as f:
        f.write("filetime\tphase\trequest\tcollection\treport_id\tprocess\n")
        for t, ph, rq, fdo, n, rid, pid in reqs:
            f.write(f"{t}\t{ph}\t{rq}\t{cname(fdo, n)}\t{rid}\t{pname.get(pid, pid)}\n")

    out = [f"run {run}", f"phases: {', '.join(p for _, _, p in spans)}", ""]
    out.append("== HID requests (phase, request, collection, report id, process): count")
    agg = Counter((ph, rq, cname(fdo, n), rid, pname.get(pid, pid)) for _, ph, rq, fdo, n, rid, pid in reqs)
    for k, v in sorted(agg.items()):
        out.append(f"{v:6}  {k[0]:<12} {k[1]:<12} id {k[3]:<4} {k[4]:<20} {k[2]}")
    out += ["", "== THC DMA (phase, direction, type): count"]
    for k, v in sorted(thc_counts.items()):
        out.append(f"{v:6}  {k[0]:<12} {k[1]:<6} {k[2]}")
    t0 = int(rows[0]["filetime"]) if rows else 0
    out += ["", "== SET/OUTPUT timeline (s from first marker, phase, request, report id, process, collection)"]
    for t, ph, rq, fdo, n, rid, pid in reqs:
        if not rq.startswith("GET"):
            out.append(f"{(t - t0) / 1e7:9.3f}  {ph:<12} {rq:<12} id {rid:<4} {pname.get(pid, pid):<16} {cname(fdo, n)}")
    out += ["", "== THC DMA other than GET_FEATURE writes and input/feature reads (s from first marker, phase, type, length, process, function)"]
    for t, ph, typ, ln, pid, fn in thc_writes:
        if typ != "write GET_FEATURE":
            out.append(f"{(t - t0) / 1e7:9.3f}  {ph:<12} {typ:<12} {ln:5}  {pname.get(pid, pid):<16} {fn}")
    order, vals = feature_snapshots(run, rows)
    out += ["", "== Feature reports that changed between snapshots (" + " > ".join(order) + ")"]
    for (c, rid), sv in sorted(vals.items()):
        seq = [sv.get(n, "") for n in order]
        if len(set(seq)) > 1:
            out.append(f"{ADDR.sub('<addr>', c)} feat {rid}")
            prev = None
            for n, v in zip(order, seq):
                if v != prev:
                    out.append(f"    {n:<22} {v[:120]}")
                prev = v
    kp = os.path.join(run, "kernel-power.csv")
    if os.path.exists(kp):
        out += ["", "== Kernel-Power events"] + [l.rstrip() for l in open(kp, encoding="utf-8-sig") if l.strip()]
    text = "\n".join(out) + "\n"
    open(os.path.join(run, "summary.txt"), "w", encoding="utf-8").write(text)
    print(text)


if __name__ == "__main__":
    main()
