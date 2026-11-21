# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
"""Summarize a typecover-probe.ps1 session per step: keys, HID reports and usages per collection, touchpad holds
(contacts still while reports continue) against wheel output and list scrolling, backlight/Fn polls, feature snapshot
changes, answers, and the HID requests ETW saw in the step (requests.tsv from input-trace.py).

    python -I -X utf8 tools/windows/typecover-summary.py <run dir>

Writes <run>/summary.md.
"""
import csv
import glob
import os
import re
import sys
from collections import Counter, OrderedDict, defaultdict

TP = "000D:0005"
X_MAX, Y_MAX, MM_PER_UNIT = 1924, 1049, 0.05  # touchpad logical range, 9.62 x 5.24 cm (layout.txt)
STILL_UNITS = 6        # contact counts as still while it stays within 0.3 mm of the anchor
HOLD_MIN_S = 0.8


def load(run):
    rows = list(csv.DictReader(open(os.path.join(run, "events.tsv"), encoding="utf-8"), delimiter="\t"))
    marks = list(csv.DictReader(open(os.path.join(run, "markers.csv"), encoding="utf-8-sig")))
    return rows, marks


def spans(marks):
    out, starts = OrderedDict(), {}
    for m in marks:
        t = int(m["filetime"])
        if m["event"] == "start":
            starts[m["phase"]] = t
        elif m["event"] == "end" and m["phase"] in starts:
            out[m["phase"]] = (starts.pop(m["phase"]), t)
    return out


def tokens(detail):
    """'1/0001:0030=12 0/0009:0001' -> {(1,'0001:0030'): '12', (0,'0009:0001'): True}"""
    d = {}
    for tok in detail.split():
        m = re.match(r"(\d+)/([0-9A-F]{4}:[0-9A-F*]{2,4})(?:=(.*))?$", tok)
        if m:
            d[(int(m.group(1)), m.group(2))] = m.group(3) if m.group(3) is not None else True
    return d


def contacts(tok):
    c = {}
    for link in range(1, 6):
        if tok.get((link, "000D:0042")) is True:
            try:
                c[int(tok.get((link, "000D:0051"), link))] = (int(tok[(link, "0001:0030")]), int(tok[(link, "0001:0031")]))
            except (KeyError, ValueError):
                pass
    return c


def edge_mm(x, y):
    return min(x, X_MAX - x, y, Y_MAX - y) * MM_PER_UNIT


def touchpad(rs, t0, wheel, scroll):
    """rs: [(t_seconds, tokens)] of touchpad reports. Returns text lines."""
    out = []
    if not rs:
        return out
    dur = rs[-1][0] - rs[0][0]
    maxc = max(len(contacts(tok)) for _, tok in rs)
    clicks = sum(1 for i in range(1, len(rs)) if rs[i][1].get((0, "0009:0001")) is True and rs[i - 1][1].get((0, "0009:0001")) is not True)
    out.append(f"- touchpad: {len(rs)} reports over {dur:.1f} s, max contacts {maxc}, button presses {clicks}")
    # Holds: >= 2 contacts, every contact within STILL_UNITS of its anchor, for >= HOLD_MIN_S.
    holds, anchor, start, n = [], None, None, 0
    last_t = None
    for t, tok in rs:
        c = contacts(tok)
        still = anchor is not None and len(c) >= 2 and set(c) == set(anchor) and all(
            abs(c[k][0] - anchor[k][0]) <= STILL_UNITS and abs(c[k][1] - anchor[k][1]) <= STILL_UNITS for k in c)
        if still:
            n += 1
            last_t = t
            continue
        if anchor is not None and last_t is not None and last_t - start >= HOLD_MIN_S:
            holds.append((start, last_t, n, anchor, t))
        anchor = c if len(c) >= 2 else None
        start, n, last_t = t, 1, t
    if anchor is not None and last_t is not None and last_t - start >= HOLD_MIN_S:
        holds.append((start, last_t, n, anchor, last_t))
    for a, b, n, anc, nxt in holds:
        pts = list(anc.values())
        e = min(edge_mm(x, y) for x, y in pts)
        w = [d for t, d in wheel if a <= t <= b]
        s = [v for t, v in scroll if a <= t <= b]
        lifted = nxt  # first report after the hold (movement or lift)
        after = [d for t, d in wheel if lifted < t <= lifted + 1.5]
        out.append(f"  - hold {a:.2f}–{b:.2f} s ({b - a:.2f} s): {n} reports ({n / max(b - a, 1e-3):.0f}/s) with contacts still, "
                   f"{len(anc)} contacts at {', '.join(f'({x},{y})' for x, y in pts)}, nearest edge {e:.1f} mm; "
                   f"wheel events during hold {len(w)} (sum {sum(w)}), list moved {len(s)} times; wheel within 1.5 s after {len(after)}")
    if not holds:
        out.append("  - no hold of >= 2 still contacts")
    return out


def feature_diff(run, phase):
    def read(name):
        p = os.path.join(run, "features", name + ".txt")
        vals, coll = {}, None
        if not os.path.exists(p):
            return vals
        for line in open(p, encoding="utf-8-sig"):
            line = line.rstrip()
            if line.startswith("\\\\?\\"):
                m = re.search(r"(?i)&col([0-9a-f]{2})#([^#]+)#", line)
                coll = f"col{m.group(1)} {m.group(2)}" if m else line
            elif line.strip() and not line.strip().startswith("#") and coll:
                kind, rid, val = line.split(None, 2)
                vals[(coll, kind, int(rid))] = val
        return vals
    a, b = read(phase + "-start"), read(phase + "-end")
    return [f"  - {k[0]} {k[1]} {k[2]}: {a.get(k, '-')} -> {b.get(k, '-')}" for k in sorted(set(a) | set(b)) if a.get(k) != b.get(k)]


def main():
    run = sys.argv[1]
    rows, marks = load(run)
    sp = spans(marks)
    t0 = int(marks[0]["filetime"]) if marks else int(rows[0]["filetime"])
    sec = lambda ft: (int(ft) - t0) / 1e7
    answers = defaultdict(list)
    ap = os.path.join(run, "answers.tsv")
    if os.path.exists(ap):
        for r in csv.DictReader(open(ap, encoding="utf-8"), delimiter="\t"):
            answers[r["phase"]].append((r["question"], r["answer"]))
    reqs = defaultdict(Counter)
    rp = os.path.join(run, "requests.tsv")
    if os.path.exists(rp):
        for r in csv.DictReader(open(rp, encoding="utf-8"), delimiter="\t"):
            if r["process"].lower().startswith("typecover-probe") and r["request"].startswith("GET"):
                continue  # our own snapshots and polls
            coll = re.sub(r"^.*?(SHP_DEVICE_\d{8})_[0-9A-F]{8}", r"\1", r["collection"], flags=re.I)
            reqs[r["phase"]][(r["request"], r["report_id"], r["process"], coll)] += 1

    out = [f"# Type Cover session {os.path.basename(run.rstrip(os.sep))}", ""]
    meta = [r for r in rows if r["src"] == "meta" and r["kind"] in ("rawinput_register", "mouse_ll_hook", "reader")
            and "err 995" not in r["detail"]]  # 995: reads cancelled at the end
    out.append("## Capture setup")
    for r in meta:
        out.append(f"- {r['kind']} {r['dev']} {r['detail']}")
    out.append("")

    for phase, (a, b) in sp.items():
        pr = [r for r in rows if r["phase"] == phase]
        out.append(f"## {phase} ({(b - a) / 1e7:.1f} s)")
        for q, ans in answers.get(phase, []):
            out.append(f"- answer: {q} → **{ans}**")
        keys = defaultdict(list)
        for r in pr:
            if r["kind"] == "key" and " down" in f" {r['detail']}":
                keys[r["dev"]].append(r["detail"].split(" make=")[0].replace(" down", ""))
        for dev, ks in keys.items():
            seq = []
            for k in ks:
                if not seq or seq[-1][0] != k:
                    seq.append([k, 1])
                else:
                    seq[-1][1] += 1
            out.append(f"- keys `{dev}`: " + " ".join(k if n == 1 else f"{k}×{n}" for k, n in seq))
        hid = defaultdict(list)
        for r in pr:
            if r["kind"].startswith("hid"):
                hid[(r["dev"], r["src"])].append(r)
        wheel = [(sec(r["filetime"]), int(r["detail"].split("delta=")[1].split()[0])) for r in pr if r["kind"] in ("wheel", "hwheel")]
        scroll = [(sec(r["filetime"]), int(r["detail"].split("=")[1])) for r in pr if r["kind"] == "scroll"]
        for (dev, src), rs in sorted(hid.items()):
            if TP in dev:
                if src == "raw":
                    out += touchpad([(sec(r["filetime"]), tokens(r["detail"])) for r in rs], t0, wheel, scroll)
                continue
            ids = Counter(r["kind"].split()[-1] for r in rs)
            pressed, values = OrderedDict(), defaultdict(OrderedDict)
            for r in rs:
                for (link, u), v in tokens(r["detail"]).items():
                    if v is True:
                        pressed[u] = 1
                    else:
                        values[u][v] = 1
            seq = [f"{sec(r['filetime']):.2f}:{r['detail'] or '(none)'}" for r in rs][:24]
            out.append(f"- HID `{dev}` via {src}: reports {dict(ids)}; usages pressed {list(pressed) or '-'}; "
                       + "; ".join(f"{u} values {list(v)[:12]}" for u, v in values.items()))
            out.append(f"  - sequence: {' | '.join(seq)}{' …' if len(rs) > 24 else ''}")
        if wheel:
            out.append(f"- WH_MOUSE_LL wheel/hwheel: {len(wheel)} events, delta sum {sum(d for _, d in wheel)}")
        other = Counter(r["kind"] + " " + r["detail"] for r in pr if r["src"] == "ll" and r["kind"] not in ("wheel", "hwheel"))
        if other:
            out.append(f"- WH_MOUSE_LL buttons: {dict(other)}")
        rm = Counter(r["dev"] + " " + r["detail"] for r in pr if r["kind"] == "mouse")
        if rm:
            out.append(f"- raw mouse button/wheel records: {dict(rm)}")
        if scroll:
            out.append(f"- list scroll: {len(scroll)} changes, top {scroll[0][1]} → {scroll[-1][1]}")
        for r in pr:
            if r["src"] == "poll":
                out.append(f"- poll {sec(r['filetime']):.2f} s `{r['dev']}` {r['kind']}: {r['detail']} ({r['hex']})")
        for r in pr:
            if r["kind"] == "counts":
                out.append(f"- {r['detail']}")
        fd = feature_diff(run, phase)
        if fd:
            out.append("- feature/GET_INPUT snapshot changes:")
            out += fd
        if reqs.get(phase):
            out.append("- HID requests (ETW, own GETs excluded):")
            for (rq, rid, proc, coll), n in sorted(reqs[phase].items()):
                out.append(f"  - {n}× {rq} id {rid} by {proc}: {coll}")
        out.append("")
    text = "\n".join(out) + "\n"
    open(os.path.join(run, "summary.md"), "w", encoding="utf-8").write(text)
    print(text)


if __name__ == "__main__":
    main()
