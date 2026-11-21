# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
"""Summarize one baseline run directory into Markdown (stdout). The last line is "VALUE: <text>", the suggested
value/memo for results.md. baseline.ps1 masks machine-unique strings before anything reaches hw/.

Usage: python -I -X utf8 summarize.py --test PEN-1 --raw private/windows/baseline/PEN-1/<stamp>
Layout of a run directory (each part optional):
  markers.csv                 t,step,text            step boundaries written by baseline.ps1
  probe/  pen.csv touch.csv rawinput.csv events.csv devices.txt      sp9probe pointer|events
  hid/ hid-elev/  hid-*.bin hid-collections.txt                      hidlog.py (normal / elevated)
  sampler/  battery.csv perf.csv sensors.csv brightness.csv presence.csv power-mode.csv
  audio/  *.json      cam/  *.txt      win/  winevents.csv      bt/  bt.etl
"""
import argparse
import csv
import glob
import json
import os
import re
import statistics as st
import struct
import sys
from collections import Counter, defaultdict

M = {}  # metrics for the VALUE line
OUT = []


def p(s=""):
    OUT.append(s)


def rows(path):
    if not os.path.exists(path):
        return []
    with open(path, encoding="utf-8-sig", newline="") as f:
        return list(csv.DictReader(f))


def num(v, d=None):
    try:
        return float(v)
    except (TypeError, ValueError):
        return d


def fmt(v, nd=1):
    if v is None:
        return "-"
    if isinstance(v, float):
        return f"{v:.{nd}f}"
    return str(v)


def stats(xs, nd=1):
    xs = [x for x in xs if x is not None]
    if not xs:
        return "-"
    if len(xs) == 1:
        return fmt(xs[0], nd)
    return f"{fmt(min(xs), nd)}..{fmt(max(xs), nd)} (mean {fmt(st.mean(xs), nd)}, SD {fmt(st.pstdev(xs), nd)})"


# ---------- steps ----------
def load_steps(raw):
    ms = rows(os.path.join(raw, "markers.csv"))
    steps = []
    for i, m in enumerate(ms):
        t0 = num(m["t"])
        t1 = num(ms[i + 1]["t"]) if i + 1 < len(ms) else float("inf")
        steps.append((m["step"], m["text"], t0, t1))
    if not steps:
        steps = [("1", "all", 0.0, float("inf"))]
    return steps


def in_step(t, s):
    return s[2] <= t < s[3]


def per_step(items, steps, tkey=lambda r: num(r["t"], 0)):
    out = []
    for s in steps:
        out.append((s, [r for r in items if in_step(tkey(r), s)]))
    return out


# ---------- pointer ----------
def pointer_devices(path):
    devs = {}
    if not os.path.exists(path):
        return devs
    for line in open(path, encoding="utf-8"):
        m = re.match(r"pointer dev (\d+):(.*)", line)
        if m:
            rng = {u: (int(a), int(b)) for u, a, b in re.findall(r"([0-9A-F]{2,4}:[0-9A-F]{2})\[(-?\d+)\.\.(-?\d+)\]", m.group(2))}
            devs[m.group(1)] = rng
    return devs


def parse_raw(s):
    d = {}
    for kv in s.split(";") if s else []:
        if "=" in kv:
            k, v = kv.split("=", 1)
            d[k] = int(v)
    return d


def strokes(rs, flag=0x4):
    """Split pen/touch samples (sorted by t) into contact strokes."""
    out, cur = [], []
    for r in rs:
        if int(r["flags"], 16) & flag:
            cur.append(r)
        elif cur:
            out.append(cur)
            cur = []
    if cur:
        out.append(cur)
    return out


def summarize_pen(raw, steps):
    rs = rows(os.path.join(raw, "probe", "pen.csv"))
    if not rs:
        p("Pen (WM_POINTER): no events")
        return
    devs = pointer_devices(os.path.join(raw, "probe", "devices.txt"))
    p("### Pen (WM_POINTER + GetRawPointerDeviceData)")
    p()
    for dv, rng in devs.items():
        p(f"- pointer device {dv}: pressure logical range {rng.get('0D:30', '-')}, tilt X {rng.get('0D:3D', '-')}, Y {rng.get('0D:3E', '-')}"
          + (", rotation (0D:3F) present" if "0D:3F" in rng else "") + (", hover Z (01:32) present" if "01:32" in rng else ""))
    p()
    p("| Step | Samples | Contact samples | Strokes | Report rate Hz | Pressure (0..1024) max | Raw pressure max | Stroke-start raw pressure min/median | Tilt X° | Tilt Y° | Side button presses | Eraser |")
    p("|---|---|---|---|---|---|---|---|---|---|---|---|")
    all_start, all_rawmax, all_max = [], [], []
    rng_used = None
    for s, rr in per_step(rs, steps):
        if not rr:
            continue
        contact = [r for r in rr if int(r["flags"], 16) & 0x4]
        sk = strokes(rr)
        ts = sorted({num(r["t"]) for r in contact})
        rate = None
        if len(contact) > 10:
            dt = [b - a for a, b in zip(ts, ts[1:]) if 0 < b - a < 0.05]
            rate = 1 / st.median(dt) if dt else None
        press = [int(r["pressure"]) for r in contact]
        rawp = [parse_raw(r["raw"]).get("0D:30") for r in contact]
        rawp = [v for v in rawp if v is not None]
        starts = []
        for k in sk:
            v = [parse_raw(r["raw"]).get("0D:30") for r in k]
            v = [x for x in v if x is not None and x > 0]
            if v:
                starts.append(v[0])
        tx = [num(r["tilt_x"]) for r in contact]
        ty = [num(r["tilt_y"]) for r in contact]
        pf = [int(r["pen_flags"], 16) for r in rr]
        barrel = sum(1 for a, b in zip([0] + pf, pf) if (b & 1) and not (a & 1))
        eraser = sum(1 for a, b in zip([0] + pf, pf) if (b & 6) and not (a & 6))
        if contact:
            rng_used = devs.get(contact[0]["dev"], {}).get("0D:30")
        p(f"| {s[0]} | {len(rr)} | {len(contact)} | {len(sk)} | {fmt(rate, 0)} | {max(press) if press else '-'} | "
          f"{max(rawp) if rawp else '-'} | {min(starts) if starts else '-'} / {int(st.median(starts)) if starts else '-'} | "
          f"{stats(tx)} | {stats(ty)} | {barrel} | {eraser} |")
        all_start += starts
        all_rawmax += rawp
        all_max += press
        M.setdefault("pen_steps", []).append({"step": s[0], "barrel": barrel, "eraser": eraser, "strokes": len(sk),
                                              "tilt_x": st.mean(tx) if tx else None, "tilt_y": st.mean(ty) if ty else None,
                                              "tilt_x_sd": st.pstdev(tx) if len(tx) > 1 else None, "tilt_y_sd": st.pstdev(ty) if len(ty) > 1 else None})
    p()
    M["pen_raw_max"] = max(all_rawmax) if all_rawmax else None
    M["pen_norm_max"] = max(all_max) if all_max else None
    M["pen_start_min"] = min(all_start) if all_start else None
    M["pen_start_med"] = int(st.median(all_start)) if all_start else None
    M["pen_range"] = rng_used


def summarize_touch(raw, steps):
    rs = rows(os.path.join(raw, "probe", "touch.csv"))
    pen = rows(os.path.join(raw, "probe", "pen.csv"))
    if not rs:
        p("Touch (WM_POINTER): no events")
        p()
        M["touch_downs"] = 0
        return
    # pen in-range intervals (for palm rejection checks)
    inrange = sorted(num(r["t"]) for r in pen if int(r["flags"], 16) & 0x2)
    p("### Touch (WM_POINTER)")
    p()
    p("| Step | Contact starts | Short taps (<300 ms) | Max simultaneous contacts | Frames with 3+ contacts | Contact starts with the pen in proximity |")
    p("|---|---|---|---|---|---|")
    for s, rr in per_step(rs, steps):
        if not rr:
            continue
        downs = [r for r in rr if int(r["flags"], 16) & 0x10000]
        ups = {r["id"]: num(r["t"]) for r in rr if int(r["flags"], 16) & 0x40000}
        taps = 0
        for d in downs:
            u = [num(r["t"]) for r in rr if r["id"] == d["id"] and int(r["flags"], 16) & 0x40000 and num(r["t"]) >= num(d["t"])]
            if u and u[0] - num(d["t"]) < 0.3:
                taps += 1
        frames = defaultdict(set)
        for r in rr:
            if int(r["flags"], 16) & 0x4:
                frames[r["frame"]].add(r["id"])
        maxc = max((len(v) for v in frames.values()), default=0)
        multi = sum(1 for v in frames.values() if len(v) >= 3)
        near = 0
        for d in downs:
            t = num(d["t"])
            if any(abs(t - x) < 0.1 for x in inrange):
                near += 1
        p(f"| {s[0]} | {len(downs)} | {taps} | {maxc} | {multi} | {near} |")
        M.setdefault("touch_steps", []).append({"step": s[0], "downs": len(downs), "taps": taps, "max": maxc, "near_pen": near, "multi": multi})
        _ = ups
    p()


def summarize_rawinput(raw, steps):
    rs = rows(os.path.join(raw, "probe", "rawinput.csv"))
    names = {}
    dp = os.path.join(raw, "probe", "devices.txt")
    if os.path.exists(dp):
        for line in open(dp, encoding="utf-8"):
            m = re.match(r"raw dev (\d+):(.*)", line)
            if m:
                names[m.group(1)] = m.group(2).strip()
    p("### Raw Input (whether HID reports reach user space)")
    p()
    if not rs:
        p("No reports (registered pages: 0D:02 pen, 0D:04 touch, 0D:20 stylus, 0C:01 consumer, 01:80 system, 01:06 keyboard in PEN-6 only)")
        p()
        M["raw_total"] = 0
        return
    by = defaultdict(list)
    for r in rs:
        by[r["dev"]].append(r)
    p("| Device | Type | Reports | Count per ID | Decoded value ranges |")
    p("|---|---|---|---|---|")
    for dv, rr in sorted(by.items(), key=lambda kv: int(kv[0])):
        name = names.get(dv, "?")
        short = re.sub(r"\\\\\?\\", "", name)
        short = re.sub(r"#\{[^}]*\}$", "", short)
        ids = Counter(r["report_id"] for r in rr)
        dec = defaultdict(list)
        btn = Counter()
        for r in rr:
            for k, v in re.findall(r"(\w+)=([-\w+]+)", r["decoded"]):
                if k == "btn":
                    for b in v.split("+"):
                        btn[b] += 1
                elif re.fullmatch(r"-?\d+", v):
                    dec[k].append(int(v))
        vals = ", ".join(f"{k} {min(v)}..{max(v)}" for k, v in dec.items() if k in ("pressure", "tilt_x", "tilt_y", "twist", "battery"))
        if btn:
            vals += (", " if vals else "") + "buttons " + " ".join(f"{k}×{n}" for k, n in btn.items())
        if rr[0]["type"] == "kbd":
            keys = Counter(r["decoded"] for r in rr)
            vals = "; ".join(f"{k} ×{n}" for k, n in keys.most_common(8))
        elif rr[0]["type"] == "kbd_other":
            vals = "keyboard other than the pen: key codes are not recorded"
        p(f"| {dv} `{short}` | {rr[0]['type']} | {len(rr)} | {dict(ids) if rr[0]['type'] == 'hid' else '-'} | {vals or '-'} |")
        if "0C52" in name.upper() or "DEV_51D0" in name.upper():
            M["raw_thc"] = M.get("raw_thc", 0) + len(rr)
        if "RID" in name.upper():
            M["raw_heat"] = M.get("raw_heat", 0) + len(rr)
        if "0C0F" in name.upper() or "BTHLE" in name.upper() or "{00001812" in name.upper():
            M["raw_ble"] = M.get("raw_ble", 0) + len(rr)
            if rr[0]["type"] == "kbd":
                M["ble_keys"] = "; ".join(f"{k}" for k, n in Counter(r["decoded"] for r in rr if "down" in r["decoded"]).most_common(6))
    M["raw_total"] = len(rs)
    p()
    p("Device names: `Microsoft HID RID\\000D_xxxx` = virtual device made by HEAT (Surface software processor), `DEV_51D0…ColNN` = THC firmware collection, `BTHLEDEVICE`/`{00001812…}` = Slim Pen 2 BLE")
    p()


def summarize_events(raw, steps):
    rs = rows(os.path.join(raw, "probe", "events.csv"))
    if not rs:
        return
    names = {"ba3e0f4d-b817-4094-a2d1-d56379e6a0f3": "lid", "5d3e9a59-e9d5-4b00-a6bd-ff34ff516548": "power source (AC=0, DC=1)",
             "6fe69556-704a-47a0-8f24-c28d936fda47": "display state (0 off, 1 on, 2 dimmed)", "a7ad8041-b45a-4cae-87a3-eecbb468a9e1": "battery %",
             "245d8541-3943-4422-b025-13a784f679b7": "power personality", "31f9f286-5084-42fe-b720-2b0264993763": "active power scheme",
             "98a7f580-01f7-48aa-9c0f-44352c29e5c0": "monitor power", "e00958c0-c213-4ace-ac77-fecced2eeea5": "energy saver"}
    ev = [r for r in rs if r["kind"] not in ("rawinput_register", "start", "stop")]
    if not ev:
        return
    lid = Counter()
    lines = []
    for s, rr in per_step(ev, steps):
        first = Counter()
        for r in rr:
            k = r["kind"]
            if k == "power_setting" and r["a"].lower().startswith("a7ad8041"):
                continue  # battery percent noise
            a = names.get(r["a"].lower(), r["a"]) if k == "power_setting" else r["a"]
            if k == "power_setting" and r["a"].lower().startswith("ba3e0f4d"):
                lid[r["b"]] += 1
            first[(k, a)] += 1
            if first[(k, a)] <= 20:
                lines.append(f"| {s[0]} | {num(r['t']) - s[2]:.1f} | {k} | {a} {r['b']} |")
    if lines:
        p("### Platform events (sp9probe)")
        p()
        p("| Step | Time (s from step start) | Type | Value |")
        p("|---|---|---|---|")
        for line in lines:
            p(line)
        p()
    M["lid_events"] = dict(lid)
    slate = [r["b"] for r in rs if r["kind"] == "state" and r["a"] == "slate_mode"]
    M["slate_changes"] = max(0, len(slate) - 1)
    disp = [r["b"] for r in rs if r["kind"] == "state" and r["a"].startswith("display0")]
    M["display_modes"] = disp
    M["display_count"] = [r["b"] for r in rs if r["kind"] == "state" and r["a"] == "displays"]


# ---------- HID (hidapi) ----------
def summarize_hid(raw):
    for sub, title in (("hid", "hidapi (normal)"), ("hid-elev", "hidapi (elevated)")):
        d = os.path.join(raw, sub)
        if not os.path.isdir(d):
            continue
        p(f"### HID raw reports, {title}")
        p()
        cp = os.path.join(d, "hid-collections.txt")
        if os.path.exists(cp):
            lines = [line.strip() for line in open(cp, encoding="utf-8") if line.strip()]
            opened = [line.split()[0] for line in lines if "open_ok" in line]
            failed = [line.split()[0] for line in lines if "open_fail" in line]
            p(f"- opened: {', '.join(opened) or 'none'}")
            p(f"- could not open (held exclusively by Windows, or permissions): {', '.join(failed) or 'none'}")
            for line in lines:
                if line.startswith("no matching"):
                    p(f"- {line}")
        p()
        bins = sorted(glob.glob(os.path.join(d, "hid-*.bin")))
        if not bins:
            p("No input reports from the opened collections")
            p()
            continue
        p("| Collection (PID-page-usage) | Reports | Count per ID | Length | Mean Hz |")
        p("|---|---|---|---|---|")
        for f in bins:
            key = os.path.basename(f)[4:-4]
            recs = []
            with open(f, "rb") as fh:
                b = fh.read()
            o = 0
            while o + 10 <= len(b):
                t, n = struct.unpack_from("<dH", b, o)
                recs.append((t, b[o + 10:o + 10 + n]))
                o += 10 + n
            if not recs:
                continue
            ids = Counter(r[1][0] if r[1] else -1 for r in recs)
            lens = sorted({len(r[1]) for r in recs})
            span = recs[-1][0] - recs[0][0]
            p(f"| {key} | {len(recs)} | {dict(ids)} | {lens[:6]} | {fmt(len(recs) / span if span > 0 else None, 1)} |")
            if key.startswith("0c52-fff4"):
                M["col07_ids"] = dict(ids)
            if key.startswith("0c52-000d-000f"):
                M["heatmap_reports"] = len(recs)
            if key.startswith("0c0f"):
                M.setdefault("ble_hid", {})[key] = dict(ids)
        p()
        p("Bytes of reports that contain addresses (Col07 84/110, BLE bonding) are not copied. The raw data is in private/ only.")
        p()


# ---------- samplers ----------
def summarize_battery(raw, steps):
    rs = rows(os.path.join(raw, "sampler", "battery.csv"))
    if len(rs) < 2:
        return
    p("### Battery (root\\wmi BatteryStatus)")
    p()
    p("| Step | Time s | AC | Charge % | Charge mWh start→end | mW from the change in charge | Reported discharge rate mW mean | Reported charge rate mW mean | Voltage mV |")
    p("|---|---|---|---|---|---|---|---|---|")
    for s, rr in per_step(rs, steps):
        if len(rr) < 2:
            continue
        t0, t1 = num(rr[0]["t"]), num(rr[-1]["t"])
        e0, e1 = num(rr[0]["remaining_mwh"]), num(rr[-1]["remaining_mwh"])
        mw = (e0 - e1) / ((t1 - t0) / 3600) if t1 > t0 else None
        dis = [num(r["discharge_rate_mw"]) for r in rr if num(r["discharge_rate_mw"])]
        chg = [num(r["charge_rate_mw"]) for r in rr if num(r.get("charge_rate_mw"))]
        ac = sorted({r["power_online"] for r in rr})
        pct = rr[-1].get("percent", "")
        p(f"| {s[0]} | {t1 - t0:.0f} | {'/'.join(ac)} | {pct} | {e0:.0f}→{e1:.0f} | {fmt(mw, 0)} | {fmt(st.mean(dis) if dis else None, 0)} | "
          f"{fmt(st.mean(chg) if chg else None, 0)} | {rr[-1]['voltage_mv']} |")
        M.setdefault("battery_steps", []).append({"step": s[0], "sec": t1 - t0, "mw": mw, "mwh": e0 - e1, "ac": ac, "pct": pct,
                                                  "dis": st.mean(dis) if dis else None, "chg": st.mean(chg) if chg else None})
    acs = [r["power_online"] for r in rs]
    M["ac_changes"] = sum(1 for a, b in zip(acs, acs[1:]) if a != b)
    p()


def summarize_perf(raw, steps, buckets=0):
    rs = rows(os.path.join(raw, "sampler", "perf.csv"))
    if not rs:
        return
    cols = list(rs[0].keys())

    def col(pat):
        return [c for c in cols if re.search(pat, c, re.I)]
    c_perf = col(r"% processor performance")
    c_freq = col(r"processor frequency")
    c_util = col(r"% processor utility")
    c_pkg = col(r"rapl_package0_pkg\)\\power")
    c_pp0 = col(r"rapl_package0_pp0\)\\power")
    c_tz = col(r"thermal zone")

    def val(r, cs):
        v = [num(r[c]) for c in cs if num(r[c]) is not None]
        return v[0] if v else None

    def tmax(r):
        v = [num(r[c]) / 10 - 273.15 for c in c_tz if num(r[c])]
        return max(v) if v else None

    p("### CPU, power, temperature (performance counters: % Processor Performance, RAPL Energy Meter, Thermal Zone)")
    p()
    p("| Step | Time s | Util % | Effective clock MHz mean/max | Package W mean/max | Core (PP0) W mean | Thermal zone max °C |")
    p("|---|---|---|---|---|---|---|")
    for s, rr in per_step(rs, steps):
        if not rr:
            continue
        eff = [val(r, c_freq) * val(r, c_perf) / 100 for r in rr if val(r, c_freq) and val(r, c_perf)]
        pkg = [val(r, c_pkg) / 1000 for r in rr if val(r, c_pkg) is not None]
        pp0 = [val(r, c_pp0) / 1000 for r in rr if val(r, c_pp0) is not None]
        ut = [val(r, c_util) for r in rr if val(r, c_util) is not None]
        tz = [tmax(r) for r in rr if tmax(r) is not None]
        p(f"| {s[0]} | {num(rr[-1]['t']) - num(rr[0]['t']):.0f} | {fmt(st.mean(ut) if ut else None, 0)} | "
          f"{fmt(st.mean(eff) if eff else None, 0)}/{fmt(max(eff) if eff else None, 0)} | {fmt(st.mean(pkg) if pkg else None)}/{fmt(max(pkg) if pkg else None)} | "
          f"{fmt(st.mean(pp0) if pp0 else None)} | {fmt(max(tz) if tz else None)} |")
        M.setdefault("perf_steps", []).append({"step": s[0], "mhz": st.mean(eff) if eff else None, "pkg": st.mean(pkg) if pkg else None,
                                               "pkg_max": max(pkg) if pkg else None, "tmax": max(tz) if tz else None})
    p()
    if buckets:
        t0 = num(rs[0]["t"])
        bk = defaultdict(list)
        for r in rs:
            bk[int((num(r["t"]) - t0) // buckets)].append(r)
        p(f"Per {buckets} s interval:")
        p()
        p("| Interval s | Effective clock MHz | Package W | Thermal zone max °C |")
        p("|---|---|---|---|")
        for k in sorted(bk):
            rr = bk[k]
            eff = [val(r, c_freq) * val(r, c_perf) / 100 for r in rr if val(r, c_freq) and val(r, c_perf)]
            pkg = [val(r, c_pkg) / 1000 for r in rr if val(r, c_pkg) is not None]
            tz = [tmax(r) for r in rr if tmax(r) is not None]
            p(f"| {k * buckets} | {fmt(st.mean(eff) if eff else None, 0)} | {fmt(st.mean(pkg) if pkg else None)} | {fmt(max(tz) if tz else None)} |")
        p()


def summarize_sensors(raw, steps):
    rs = rows(os.path.join(raw, "sampler", "sensors.csv"))
    if not rs:
        return
    p("### Sensors (WinRT Windows.Devices.Sensors)")
    p()
    p("| Step | Acceleration X/Y/Z g mean | Orientation (SimpleOrientation) | Pitch/roll ° | Light lux mean (min..max) | Gyro max °/s |")
    p("|---|---|---|---|---|---|")
    for s, rr in per_step(rs, steps):
        if not rr:
            continue

        def m(k):
            v = [num(r[k]) for r in rr if num(r[k]) is not None]
            return st.mean(v) if v else None
        lux = [num(r["lux"]) for r in rr if num(r["lux"]) is not None]
        gy = [max(abs(num(r[k], 0)) for k in ("gyro_x", "gyro_y", "gyro_z")) for r in rr]
        ori = Counter(r["orientation"] for r in rr).most_common(1)[0][0]
        p(f"| {s[0]} | {fmt(m('acc_x'), 2)} / {fmt(m('acc_y'), 2)} / {fmt(m('acc_z'), 2)} | {ori} | {fmt(m('pitch'))} / {fmt(m('roll'))} | "
          f"{fmt(st.mean(lux) if lux else None)} ({fmt(min(lux) if lux else None)}..{fmt(max(lux) if lux else None)}) | {fmt(max(gy) if gy else None)} |")
        M.setdefault("sensor_steps", []).append({"step": s[0], "acc": (m("acc_x"), m("acc_y"), m("acc_z")), "ori": ori,
                                                 "lux": st.mean(lux) if lux else None, "lux_min": min(lux) if lux else None})
    p()
    p("Acceleration axes are in the Windows sensor frame (relative to the screen, +Y up in portrait, +Z out of the screen). The mount matrix is set by matching signs and axes with Linux `in_accel_*`.")
    p()


def summarize_simple(raw, steps):
    br = rows(os.path.join(raw, "sampler", "brightness.csv"))
    if br:
        vals = [int(num(r["brightness"])) for r in br if num(r["brightness"]) is not None]
        p(f"### Brightness (WmiMonitorBrightness)\n\n- observed levels: {sorted(set(vals))} ({len(vals) - 1} changes), min {min(vals) if vals else '-'}\n")
        M["bright_min"] = min(vals) if vals else None
        M["bright_n"] = len(vals) - 1
    pm = rows(os.path.join(raw, "sampler", "power-mode.csv"))
    if pm:
        names = {"961cc777-2547-4f9d-8174-7d86181b8a7a": "Best Power Efficiency", "00000000-0000-0000-0000-000000000000": "Balanced",
                 "ded574b5-45a0-4f42-8737-46345c09c238": "Best Performance", "3af9b8d9-7c97-431d-ad78-34a8bfea439f": "Better Performance", "": "Balanced"}
        p("### Power mode")
        p()
        for s, rr in per_step(pm, steps):
            for r in rr:
                p(f"- step {s[0]}: scheme {r['scheme']}, mode AC {names.get(r['overlay_ac'].lower(), r['overlay_ac'])} / DC {names.get(r['overlay_dc'].lower(), r['overlay_dc'])}")
        p()
        M["power_modes"] = [names.get((r["overlay_ac"] or r["overlay_dc"]).lower(), "?") for r in pm]
    pr = rows(os.path.join(raw, "sampler", "presence.csv"))
    if pr:
        t_start = num(pr[0]["t"])
        ch = [r for r in pr if r["event"] != "present" or num(r["t"]) - t_start > 1.5]
        p("### Device node changes (pen BLE, Type Cover)")
        p()
        if not ch:
            p("- no changes during the test")
        for s, rr in per_step(ch, steps):
            for r in rr[:30]:
                p(f"- step {s[0]} +{num(r['t']) - s[2] if s[2] else 0:.1f}s {r['event']}: {r['name']} (error code {r['status']})")
        p()
        M["presence_changes"] = len(ch)
        pen_up = [num(r["t"]) for r in ch if r["event"] in ("present", "change") and re.search("0C0F", r["id"], re.I) and r["id"].upper().startswith("HID")]
        M["pen_hid_up"] = pen_up
        cover = [r for r in ch if re.search("type cover|PID_09A", r["name"] + r["id"], re.I)]
        M["cover_changes"] = len(cover)


def summarize_audio(raw):
    files = sorted(glob.glob(os.path.join(raw, "audio", "*.json")))
    if not files:
        return
    p("### Audio (internal speaker playback + simultaneous recording from the internal 2-channel microphone)")
    p()
    for f in files:
        r = json.load(open(f, encoding="utf-8"))
        name = os.path.basename(f)[:-5]
        mode = "APO on (shared)" if r.get("mode") == "shared" else "render APO bypassed (exclusive playback)"
        if "error" in r:
            p(f"- {name}: failed {r['error']}")
            continue
        if "speaker" in r:
            p(f"**Sweep, {mode}, signal {r['level_dbfs']} dBFS** — left/right balance (R−L) {r['speaker_balance_db_R_minus_L']:+.1f} dB")
            p()
            p("| Speaker | Latency ms | IR SNR dB | Mic L/R level dB | Delay between mics µs | IR polarity L/R | Sum − difference dB |")
            p("|---|---|---|---|---|---|---|")
            for sp, v in r["speaker"].items():
                p(f"| {sp} | {v['latency_ms']:.0f} | {v['ir_snr_db']:.0f}{'' if v['reliable'] else ' (low confidence)'} | {v['mic_level_db'][0]:.1f} / {v['mic_level_db'][1]:.1f} | "
                  f"{v['mic_delay_us']:.0f} | {v['ir_polarity'][0]:+d} / {v['ir_polarity'][1]:+d} | {v['pair']['sum_minus_diff_db']:.1f} |")
            p()
            bands = list(next(iter(r["speaker"].values()))["response_db_rel_1k"][0].keys())
            p("1/3-octave response (dB relative to 1 kHz, mean of the two mic channels):")
            p()
            p("| Speaker | " + " | ".join(bands) + " |")
            p("|---|" + "---|" * len(bands))
            for sp, v in r["speaker"].items():
                avg = [(v["response_db_rel_1k"][0][b] + v["response_db_rel_1k"][1][b]) / 2 for b in bands]
                p(f"| {sp} | " + " | ".join(f"{x:.0f}" for x in avg) + " |")
            p()
            pol = {tuple(v["ir_polarity"]) for v in r["speaker"].values() if v["reliable"]}
            M[f"sweep_{r['mode']}"] = {"balance": r["speaker_balance_db_R_minus_L"],
                                       "delays": [round(v["mic_delay_us"]) for v in r["speaker"].values()],
                                       "same_polarity": all(a == b for a, b in pol) if pol else None}
        elif "tones" in r:
            p(f"**Tones, {mode}, {r.get('tag') or ''} signal {r['level_dbfs']} dBFS**")
            p()
            p("| Frequency Hz | Speaker | Mic level dBFS | SNR dB | THD % (at the mic, approximate) | Clipped samples |")
            p("|---|---|---|---|---|---|")
            for t in r["tones"]:
                p(f"| {t['freq']:.0f} | {t['speaker']} | {t['mic_level_dbfs']:.1f} | {fmt(t.get('snr_db'), 0)} | {fmt(t['thd_pct'])} | {t['clipped_samples']} |")
            p()
            M.setdefault("tones", []).append(r)
        elif "pair" in r:
            q = r["pair"]
            what = "Silent recording (noise floor)" if r.get("tag") == "ambient" else "Recording"
            p(f"**{what} {r['seconds']:.0f} s, {mode}**: channel RMS {q['rms_dbfs'][0]:.1f} / {q['rms_dbfs'][1]:.1f} dBFS, peak {q['peak_dbfs'][0]:.1f} / {q['peak_dbfs'][1]:.1f}, "
              f"clipping {q['clipped_samples']}, delay between channels {q['lag_us']:.0f} µs, correlation polarity {q['polarity']:+d}, sum − difference {q['sum_minus_diff_db']:.1f} dB")
            p()
            M["mic_ambient" if r.get("tag") == "ambient" else "mic_record"] = q
    p("A positive sum − difference means both channels have the same polarity (the sum is larger); negative means one is inverted (the same test as gaps/audio G5).")
    p()


def summarize_text(raw, sub, title):
    d = os.path.join(raw, sub)
    if not os.path.isdir(d):
        return
    txt = sorted(glob.glob(os.path.join(d, "*.txt")))
    if not txt:
        return
    p(f"### {title}")
    p()
    for f in txt:
        body = open(f, encoding="utf-8", errors="replace").read()
        if f.endswith("-end.txt"):
            start = f[:-8] + "-start.txt"
            if os.path.exists(start) and open(start, encoding="utf-8", errors="replace").read() == body:
                continue  # unchanged during the test
        if len(txt) > 1:
            p(f"`{os.path.basename(f)[:-4]}`")
            p()
        p("```text")
        for line in body.splitlines():
            p(line.rstrip())
        p("```")
        p()


def summarize_winevents(raw, steps):
    rs = rows(os.path.join(raw, "win", "winevents.csv"))
    if not rs:
        return
    p("### Windows event log (test interval)")
    p()
    c = Counter((r["log"], r["id"], r["what"]) for r in rs)
    p("| Log | ID | Meaning | Count |")
    p("|---|---|---|---|")
    for (lg, i, w), n in sorted(c.items()):
        p(f"| {lg} | {i} | {w} | {n} |")
    p()
    M["standby_enter"] = sum(n for (lg, i, w), n in c.items() if i == "506")
    M["standby_exit"] = sum(n for (lg, i, w), n in c.items() if i == "507")
    pl = [r for r in rs if r["what"].startswith("SMF")]
    if pl:
        by = defaultdict(list)
        for r in pl:
            by[r["what"]].append(int(r["value"]))
        p("SMF CPU power limit updates (test interval):")
        p()
        for k, v in sorted(by.items()):
            p(f"- {k}: n={len(v)}, min {min(v)}, max {max(v)}, last {v[-1]}")
        p()
        M["smf"] = {k: (min(v), max(v)) for k, v in by.items()}


def summarize_sleepstudy(raw):
    """Modern standby sessions from powercfg /sleepstudy /xml (last day): drain and HW DRIPS per session on battery."""
    f = os.path.join(raw, "power", "sleepstudy.xml")
    if not os.path.exists(f):
        return
    import xml.etree.ElementTree as ET
    try:
        root = ET.parse(f).getroot()
    except ET.ParseError as e:
        p(f"### sleepstudy\n\n- reading the XML failed: {e}\n")
        return
    sess = []
    for el in root.iter():
        if not el.tag.endswith("ScenarioInstance"):
            continue
        a = el.attrib
        dur = num(a.get("Duration"))
        if not dur or a.get("Type") not in ("Sleep", "Screen Off"):
            continue
        h = dur / 1e7 / 3600
        hw = num(a.get("HwLowPowerStateTime"))
        sw = num(a.get("LowPowerStateTime"))
        drain = num(a.get("EnergyDrain"))
        sess.append({"type": a["Type"], "h": h, "ac": a.get("Ac") == "1", "mw": drain / h if drain is not None and h > 0 else None,
                     "hw": 100 * hw / dur if hw is not None else None, "sw": 100 * sw / dur if sw is not None else None})
    p("### Modern Standby record (powercfg /sleepstudy, last day)")
    p()
    if not sess:
        M["sleepstudy"] = []
        p("- no standby sessions recorded in the last day")
        p()
        return
    p("| # | Type | Duration h | Power | Drain mW (change in charge) | SW DRIPS % | HW DRIPS % |")
    p("|---|---|---|---|---|---|---|")
    for i, s in enumerate(sess, 1):
        p(f"| {i} | {s['type']} | {s['h']:.2f} | {'AC' if s['ac'] else 'battery'} | {fmt(s['mw'] if not s['ac'] else None, 0)} | {fmt(s['sw'])} | {fmt(s['hw'])} |")
    p()
    p("Drain mW is left empty for AC sessions, where charging makes it meaningless. For sessions under 30 min the mW value is coarse because of the charge resolution.")
    p()
    M["sleepstudy"] = sess


def brightness_info(raw):
    f = os.path.join(raw, "info", "brightness-start.txt")
    return open(f, encoding="utf-8").read().strip() if os.path.exists(f) else ""


def wlan_info(raw):
    f = os.path.join(raw, "info", "wlan-start.txt")
    if not os.path.exists(f):
        return ""
    out = []
    for line in open(f, encoding="utf-8"):
        line = line.strip()
        m = re.match(r"adapter: (.*?), status (\w+), link rx (\d+) / tx (\d+) Mbit/s", line)
        if m:
            out.append(f"{m.group(1)} {m.group(2)}, link receive {m.group(3)} / send {m.group(4)} Mbit/s")
        elif line.startswith("Radio types supported"):
            out.append("supported " + line.split(":", 1)[1].strip())
        elif line.startswith("signal bars:"):
            out.append("signal bars " + line.split(":", 1)[1].strip().replace(" ", ""))
        elif re.match(r"(Radio type|Band|Channel|Receive rate|Transmit rate|Signal)\s*:", line):
            out.append(re.sub(r"\s+:\s+", " ", line))
    return ", ".join(out)


# ---------- value line per test ----------
def value(test):
    g = M.get
    ps = g("pen_steps", [])
    ts = g("touch_steps", [])
    v = ""
    if test in ("PEN-1", "PEN-2", "PEN-3", "PEN-4") and not ps:
        v = "no pen input"
    elif test == "PEN-1":
        v = f"raw pressure max {g('pen_raw_max')} (range {g('pen_range')}), normalised max {g('pen_norm_max')}/1024, stroke-start raw min {g('pen_start_min')} median {g('pen_start_med')}"
    elif test == "PEN-2":
        v = "; ".join(f"step {s['step']} X {fmt(s['tilt_x'])}±{fmt(s['tilt_x_sd'])} Y {fmt(s['tilt_y'])}±{fmt(s['tilt_y_sd'])}" for s in ps if s["tilt_x"] is not None)
    elif test == "PEN-3":
        v = "; ".join(f"step {s['step']} side button {s['barrel']}x" for s in ps)
    elif test == "PEN-4":
        v = f"eraser switches {sum(s['eraser'] for s in ps)}"
    elif test in ("PEN-5", "PEN-9"):
        v = f"BLE HID {g('ble_hid') or 'none'}, device node changes {g('presence_changes', 0)}"
    elif test == "PEN-6":
        v = f"BLE keys {g('ble_keys') or 'none'}"
    elif test == "PEN-7":
        v = f"Col07 reports {g('col07_ids') or 'none (no pen contact, or could not open)'}"
    elif test == "PEN-8":
        v = f"Raw Input reports: THC collections {g('raw_thc', 0)}, HEAT virtual devices {g('raw_heat', 0)}"
    elif test == "TOUCH-1":
        v = "; ".join(f"step {s['step']} contacts {s['downs']} taps {s['taps']} max simultaneous {s['max']}" for s in ts)
    elif test == "TOUCH-2":
        v = "; ".join(f"step {s['step']} touches with the pen in proximity {s['near_pen']}" for s in ts)
    elif test in ("TOUCH-3", "TOUCH-4", "PWR-1", "PWR-2", "PWR-3"):
        v = f"Modern Standby entries {g('standby_enter', 0)} exits {g('standby_exit', 0)}, lid events {g('lid_events') or {}}"
    elif test.startswith("CAM"):
        v = g("cam_value", "")
    elif test == "SAM-1":
        v = f"cover node changes {g('cover_changes', 0)}, tablet mode switches {g('slate_changes', 0)}"
    elif test == "SAM-2":
        v = f"tablet mode switches {g('slate_changes', 0)}"
    elif test == "SAM-3":
        v = f"Raw Input reports {g('raw_total', 0)}, standby entries {g('standby_enter', 0)}"
    elif test in ("SAM-4", "SAM-5", "PWR-9"):
        v = "; ".join(f"step {s['step']} {fmt(s['mhz'], 0)} MHz {fmt(s['pkg'])} W (max {fmt(s['pkg_max'])}) {fmt(s['tmax'])}°C" for s in g("perf_steps", []) if s["pkg"])
        if g("power_modes"):
            v = f"power mode {g('power_modes')[0]}; " + v
    elif test == "SAM-8":
        v = "; ".join(f"step {s['step']} {fmt(s['pkg'])} W {fmt(s['tmax'])}°C" for s in g("perf_steps", []) if s["pkg"])
        v += "; " + ", ".join(f"{k} {a}..{b}" for k, (a, b) in sorted(g("smf", {}).items()))
    elif test == "SAM-6":
        v = f"AC transitions {g('ac_changes', 0)}"
        for s in g("battery_steps", []):
            v += f"; AC {'/'.join(s['ac'])}, charge {s['pct']}%, charge rate {fmt(s['chg'], 0)} mW, discharge rate {fmt(s['dis'], 0)} mW"
    elif test == "DSP-1":
        v = "; ".join(f"{fmt(s['mhz'], 0)} MHz package {fmt(s['pkg'], 2)} W (max {fmt(s['pkg_max'])})" for s in g("perf_steps", []) if s["pkg"])
        for s in g("battery_steps", []):
            v += f"; AC {'/'.join(s['ac'])}" + (f", discharge {fmt(s['mw'], 0)} mW" if s["ac"] == ["0"] else "")
        if g("bright_min") is not None:
            v += f"; brightness {g('bright_min')}%"
    elif test == "SAM-7":
        v = "; ".join(f"{s['ori']} acc({fmt(s['acc'][0], 2)},{fmt(s['acc'][1], 2)},{fmt(s['acc'][2], 2)})" for s in g("sensor_steps", []))
    elif test == "DSP-4":
        v = "; ".join(f"step {s['step']} {fmt(s['lux'])} lux" for s in g("sensor_steps", []))
    elif test == "DSP-2":
        v = f"modes {' → '.join(g('display_modes', []))}"
    elif test == "DSP-3":
        v = f"brightness min {g('bright_min')}, changes {g('bright_n')}"
        if g("bright_info"):
            v += f"; {g('bright_info')}"
    elif test == "DSP-5":
        v = f"display count {' → '.join(g('display_count', []))}"
    elif test == "PWR-4":
        bs = g("battery_steps", [])
        if bs and all(s["ac"] == ["0"] for s in bs):
            v = "; ".join(f"step {s['step']} {fmt(s['sec'] / 60, 0)} min {fmt(s['mw'], 0)} mW" for s in bs if s["mw"] is not None)
            v += f", standby entries {g('standby_enter', 0)}"
        elif bs:
            v = "on AC, so no discharge rate"
        ss = [s for s in g("sleepstudy", []) if not s["ac"] and s["type"] == "Sleep" and s["h"] >= 0.5]
        if ss:
            v += "; sleepstudy standby on battery (30 min or longer) " + ", ".join(f"{s['h']:.1f} h {fmt(s['mw'], 0)} mW HW DRIPS {fmt(s['hw'])}%" for s in ss)
        elif g("sleepstudy") is not None:
            v += "; no sleepstudy standby session on battery of 30 min or longer"
    elif test in ("AUD-1", "AUD-2"):
        v = "; ".join(f"{r.get('tag')} {r['mode']}: " + ", ".join(f"{t['freq']:.0f}Hz/{t['speaker']} THD {fmt(t['thd_pct'])}%" for t in r["tones"] if t["speaker"] == "LR") for r in g("tones", []))
        s, x = g("sweep_shared"), g("sweep_exclusive")
        if x:
            v = f"left/right balance bypassed {x['balance']:+.1f} dB; " + v
        if s:
            v = f"left/right balance {s['balance']:+.1f} dB; " + v
    elif test == "AUD-3":
        s, q, n = g("sweep_shared"), g("mic_record"), g("mic_ambient")
        v = (f"sweep delay between mics {s['delays']} µs, same polarity {s['same_polarity']}" if s else "") + \
            (f"; speech sum − difference {q['sum_minus_diff_db']:.1f} dB, clipping {q['clipped_samples']}" if q else "") + \
            (f"; silence RMS {n['rms_dbfs'][0]:.1f} / {n['rms_dbfs'][1]:.1f} dBFS" if n else "")
    elif test == "NET-1":
        v = "; ".join(x for x in (g("net_value", ""), g("wlan_info", "")) if x)
    return v.strip("; ")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--test", required=True)
    ap.add_argument("--raw", required=True)
    a = ap.parse_args()
    raw = a.raw
    steps = load_steps(raw)
    area = a.test.split("-")[0]
    if os.path.exists(os.path.join(raw, "probe", "pen.csv")):
        summarize_pen(raw, steps)
    if os.path.exists(os.path.join(raw, "probe", "touch.csv")) and (area == "TOUCH" or a.test in ("PEN-8",)):
        summarize_touch(raw, steps)
    if os.path.exists(os.path.join(raw, "probe", "rawinput.csv")):
        summarize_rawinput(raw, steps)
    summarize_events(raw, steps)
    summarize_hid(raw)
    summarize_battery(raw, steps)
    summarize_perf(raw, steps, buckets=30 if a.test in ("PWR-9", "SAM-4") else 0)
    summarize_sensors(raw, steps)
    summarize_simple(raw, steps)
    summarize_audio(raw)
    summarize_text(raw, "cam", "Camera")
    summarize_text(raw, "net", "Network")
    summarize_text(raw, "info", "State snapshots")
    summarize_winevents(raw, steps)
    summarize_sleepstudy(raw)
    M["bright_info"] = brightness_info(raw)
    M["wlan_info"] = wlan_info(raw)
    camv = os.path.join(raw, "cam", "value.val")
    if os.path.exists(camv):
        M["cam_value"] = open(camv, encoding="utf-8").read().strip()
    netv = os.path.join(raw, "net", "value.val")
    if os.path.exists(netv):
        M["net_value"] = open(netv, encoding="utf-8").read().strip()
    bt = os.path.join(raw, "bt", "bt.etl")
    if os.path.exists(bt):
        p(f"### Bluetooth ETW\n\n- `bt.etl` {os.path.getsize(bt) / 1e6:.1f} MB (private/). For conversion see tools/windows/baseline/README.md\n")
    sys.stdout.reconfigure(encoding="utf-8")
    print("\n".join(OUT))
    print("VALUE: " + value(a.test))


if __name__ == "__main__":
    main()
