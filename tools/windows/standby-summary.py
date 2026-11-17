# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
"""Summarize one standby-trace.ps1 run offline: trace.etl (decoded with etwdecode.exe, TDH) + sleepstudy.xml + run.txt.

Usage: python -I -X utf8 tools/windows/standby-summary.py private/windows/standby/<run> [--max-rows 1500]

Writes <run>/trace.tsv (all events, decoded) and <run>/summary.md:
  - provider coverage: event count per expected provider; a missing provider means the session never enabled it
  - standby window: Kernel-Power 506/507/566 (from the trace, else from run.txt)
  - timeline: Kernel-Power standby/session events, Kernel-Acpi (_DSM/LPS0/SAPD/SSH AML methods, notifications),
    Kernel-Pep and Intel PEP (constraints, D-state, platform idle), PDC activators and phases, Surface drivers
    (SSH display on/off and SAM sleep commands, SAPD/SMF notifications), SleepStudy/UMPS session markers
  - PDC activators active while in standby
  - the sleepstudy session(s) of the run: duration, SW/HW DRIPS %, phases, top blockers, DRIPS buckets, energy
The raw files carry the machine name and device instance paths; copy only model-level facts into docs.
"""
import argparse
import collections
import csv
import datetime as dt
import os
import re
import subprocess
import sys
import xml.etree.ElementTree as ET

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))

# guid -> short name; the order is the coverage table order
PROVIDERS = {
    "331c3b3a-2005-44c2-ac5e-77220c37d6b4": "Kernel-Power",
    "c514638f-7723-485b-bcfc-96565d735d4a": "Kernel-Acpi",
    "5412704e-b2e1-4624-8ffd-55777b8f7373": "Kernel-Pep",
    "9b82887e-0d74-4a2e-85ff-521f93c9d4a0": "IntelPep",
    "a6bf0deb-3659-40ad-9f81-e25af62ce3c7": "PDC",
    "d37687e7-8bf0-4d11-b589-a7abe080756a": "SleepStudy",
    "ce8dee0b-d539-4000-b0f8-77bed049c590": "UserModePowerService",
    "aba759c2-2f7b-41c2-8cfc-71c370e17276": "Surface SAPD",
    "8ddb06d0-a54b-4014-b119-5a09fb550d18": "Surface SSH",
    "0ec12c6e-794a-4b5e-8e27-6d3dc98bbd38": "Surface SMF Core",
    "a8d5fcb2-ff94-4166-920d-3338902e77e0": "Surface SMF Client",
    "32760e2c-4ac2-40eb-9972-1c69c12ed757": "Surface SMF CPU",
    "44f35d0a-a3fa-4248-a7cb-22fe08b3288d": "Surface PowerState",
    "23ac50b3-680e-7ad3-a2b3-d5dbef945870": "Surface PowerTrackerCore",
    "69cd60e3-430f-4da4-b1b3-e3bdaf945875": "Surface HotPlug",
    "2fea7205-b0b1-41ca-8609-5a1d16f3132f": "Surface HidMini",
    "dcf09792-3c77-4d1c-8b99-a8a089841155": "Surface Battery",
    "b87caa6f-37a7-4f09-8dc4-d15572c5904e": "iaLPSS2 UART2",
    "937ad94e-ca8d-4b8e-8143-3fce4accb8cb": "IntelTHCBase",
}
# Kernel-Power ids kept in the timeline (standby, session, sleep, power source, connectivity, wake source)
KP_IDS = {42, 105, 107, 172, 187, 506, 507, 508, 566, 567, 1, 2, 131, 132, 133}
KP_TASKS = {"DevicePowerState"}   # D-state transitions (token only; names are in DeviceRegistration)
AML_RE = re.compile(r"AmlMethodName=[^;]*(_DSM|_PS[0-3]|_PR[0-3]|\._ON|\._OFF|_DSW|_PSW|\._[LEQ][0-9A-F]{2}|PEPD|SAPD|FAN|_FST|_FSL)\b")
PDC_RE = re.compile(r"Phase(?!.*Rundown)|SpmScenarioStateChange|PDCV2ActivationClient(Activate|Deactivate)|Notif(?!.*Rundown)|Drips")
NEAR_S = 10   # seconds around a standby window in which chatty events are kept


def build_decoder():
    exe = os.path.join(REPO, "out", "standby", "etwdecode.exe")
    src = os.path.join(HERE, "etwdecode.cs")
    if not os.path.exists(exe) or os.path.getmtime(src) > os.path.getmtime(exe):
        os.makedirs(os.path.dirname(exe), exist_ok=True)
        csc = os.path.join(os.environ["WINDIR"], r"Microsoft.NET\Framework64\v4.0.30319\csc.exe")
        subprocess.check_call([csc, "/nologo", "/optimize+", "/unsafe", "/platform:x64", "/out:" + exe, src])
    return exe


def decode(run):
    etl = os.path.join(run, "trace.etl")
    tsv = os.path.join(run, "trace.tsv")
    if os.path.exists(etl) and (not os.path.exists(tsv) or os.path.getmtime(etl) > os.path.getmtime(tsv)):
        subprocess.check_call([build_decoder(), etl, tsv])
    rows = []
    if os.path.exists(tsv):
        with open(tsv, encoding="utf-8", newline="") as f:
            for r in csv.DictReader(f, delimiter="\t", quoting=csv.QUOTE_NONE):
                r["t"] = parse_time(r["time"])
                rows.append(r)
    return rows


def parse_time(s):
    # 2026-11-14T02:40:46.6155095Z (7 fraction digits)
    base, frac = s.rstrip("Z").split(".")
    return dt.datetime.strptime(base, "%Y-%m-%dT%H:%M:%S").replace(
        microsecond=int(frac[:6]), tzinfo=dt.timezone.utc)


def read_text_any(path):
    """run.txt mixes UTF-8 lines and UTF-16 lines (Tee-Object -Append); drop NULs and BOMs."""
    b = open(path, "rb").read()
    return b.replace(b"\x00", b"").replace(b"\xef\xbb\xbf", b"").replace(b"\xff\xfe", b"").decode("utf-8", "replace").replace("\r", "")


def props(r):
    p = r.get("props") or ""
    p = re.sub(r"\b\w*Length=\d+ ; ", "", p)
    p = re.sub(r"ActivationHandle=0x[0-9A-F]+ ; |PdcVersion=\d+ ; ", "", p)
    return p


def short(r):
    g = r["provider"].lower()
    return PROVIDERS.get(g) or (r["provider_name"] or g).replace("Microsoft-Windows-", "").replace("Microsoft-Surface-", "")


def keep(r, near):
    """timeline filter; returns a label or None. near = within NEAR_S of a standby window (or no window known)."""
    g = r["provider"].lower()
    name = PROVIDERS.get(g)
    task = r["task"] or r["event"] or ""
    if "Rundown" in task or "ConstraintInfo" in task or "PcieFeatures" in task:
        return None   # capture-state output: tables below
    if name == "Kernel-Power":
        if int(r["id"]) in KP_IDS:
            return "power"
        return "dstate" if task in KP_TASKS and near else None
    if name == "Kernel-Acpi":
        if "AmlMethod" in task or "FrequentAml" in task:
            return "acpi" if AML_RE.search(r["props"]) else None
        return "acpi"
    if name in ("Kernel-Pep", "IntelPep"):
        return "pep"
    if name == "PDC":
        if "PDCV2ActivationClient" in task:
            return "pdc" if near else None
        return "pdc" if PDC_RE.search(task) else None
    if name == "Surface PowerTrackerCore":
        return "surface" if near and re.search(r"Fan|PowerOutput", task) else None
    if name and name.startswith("Surface"):
        return "surface"
    if name == "iaLPSS2 UART2":
        return "uart" if r["id"] in ("17", "18", "19", "20") and near else None
    if name in ("SleepStudy", "UserModePowerService"):
        return "session" if re.search(r"SleepSession|SmartUserPresence|Phase|Drips", task) else None
    return None


def pairs(hexstr):
    """PlatformStateDependency array: (platform state, D-state) u32 pairs"""
    b = bytes.fromhex(hexstr)
    out = []
    for i in range(0, len(b) - 7, 8):
        ps, ds = int.from_bytes(b[i:i + 4], "little"), int.from_bytes(b[i + 4:i + 8], "little")
        if ps > 16 or ds > 4:
            break
        out.append(f"PS{ps}:D{ds}")
    return " ".join(out)


def rundown_tables(rows):
    md = []
    dev = collections.OrderedDict()
    for r in rows:
        if r["task"] == "PepDeviceRundown":
            m = re.search(r"DeviceName=([^;]*) ;.*PlatformStateDependency=rest:([0-9a-f]*)", r["props"])
            if m:
                dev[m.group(1).strip()] = pairs(m.group(2))
    if dev:
        md += ["## PEP device constraints (Kernel-Pep PepDeviceRundown)", "",
               "Pairs are (platform idle state, deepest D-state the device must reach). Devices with D0 everywhere are folded.", "",
               "| device | dependency |", "|---|---|"]
        d0 = []
        for k, v in dev.items():
            if re.fullmatch(r"(PS\d+:D0 ?)*", v):
                d0.append(k)
            else:
                md.append(f"| {k} | {v} |")
        md += ["", f"D0 only ({len(d0)}): " + ", ".join(d0), ""]
    ip = collections.OrderedDict()
    for r in rows:
        if PROVIDERS.get(r["provider"].lower()) == "IntelPep" and ("ConstraintInfo" in r["task"] or "PcieFeatures" in r["task"]):
            p = re.sub(r"PartA_PrivTags=\d+ ; |SleepStudySessionInstanceId=\d+ ; ", "", r["props"])
            ip[(r["task"], p)] = 1
    if ip:
        md += ["## Intel PEP PCIe constraints (capture state)", "", "| event | data |", "|---|---|"]
        for (task, p) in ip:
            md.append(f"| {task} | {p.replace('|', '/')} |")
        md.append("")
    last = collections.OrderedDict()
    for r in rows:
        if r["task"] in ("PdcPhaseAccountingRundown", "FanRundown", "SpmScenarioRundown"):
            m = re.search(r"(Phase|FanBiosName|Scenario)=([^;]*)", r["props"])
            last[(r["task"], m.group(2).strip() if m else "")] = (r["t"], r["props"])
    if last:
        md += ["## Other rundowns (last value: PDC phase accounting, fan, SPM scenarios)", "",
               "| time | event | data |", "|---|---|---|"]
        for (task, _), (t, p) in last.items():
            md.append(f"| {fmt_t(t)} | {task} | {p.replace('|', '/')} |")
        md.append("")
    return md


def standby_window(rows, run_txt):
    """[(enter, exit)] from Kernel-Power 506/507 in the trace, else from run.txt lines '506=<iso> ... 507=<iso>'."""
    out = []
    kp = [r for r in rows if r["provider"].lower() == "331c3b3a-2005-44c2-ac5e-77220c37d6b4" and r["id"] in ("506", "507")]
    enter = None
    for r in kp:
        if r["id"] == "506":
            enter = r["t"]
        elif enter:
            out.append((enter, r["t"], "trace"))
            enter = None
    if out:
        return out
    for m in re.finditer(r"506=(\S+).*?507=(\S+)", run_txt):
        try:
            a = dt.datetime.fromisoformat(m.group(1)[:26] + m.group(1)[27:] if len(m.group(1)) > 33 else m.group(1))
            b = dt.datetime.fromisoformat(m.group(2)[:26] + m.group(2)[27:] if len(m.group(2)) > 33 else m.group(2))
            out.append((a.astimezone(dt.timezone.utc), b.astimezone(dt.timezone.utc), "run.txt"))
        except ValueError:
            pass
    return out


def fmt_t(t):
    return t.strftime("%H:%M:%S.%f")[:-3]


def sleepstudy(path, windows):
    ns = {"s": "http://schemas.microsoft.com/sleepstudy/2012"}
    tree = ET.parse(path)
    md = []
    insts = tree.getroot().findall(".//s:ScenarioInstances/s:ScenarioInstance", ns)
    picked = []
    for si in insts:
        if si.get("Type") != "Sleep":
            continue
        start = dt.datetime.strptime(si.get("TimeStamp"), "%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=dt.timezone.utc)
        dur = int(si.get("Duration", "0")) / 1e7
        if not windows or any(abs((start - a).total_seconds()) < 5 for a, _, _ in windows):
            picked.append((si, start, dur))
    if windows:
        picked = [p for p in picked if p[2] > 30] or picked
    for si, start, dur in picked:
        lp = int(si.get("LowPowerStateTime", "0")) / 1e7
        hw = int(si.get("HwLowPowerStateTime", "0")) / 1e7
        md.append(f"### sleepstudy session {si.get('Id')} ({fmt_t(start)} UTC, {dur:.1f} s)")
        md.append("")
        md.append("| item | value |")
        md.append("|---|---|")
        md.append(f"| enter / exit reason | {si.get('CsEnterReason')} / {si.get('CsExitReason')} |")
        md.append(f"| AC | {si.get('Ac')} |")
        md.append(f"| SW DRIPS (LowPowerStateTime) | {lp:.1f} s = {100 * lp / dur:.1f}% |" if dur else "| SW DRIPS | - |")
        md.append(f"| HW DRIPS (HwLowPowerStateTime) | {hw:.1f} s = {100 * hw / dur:.1f}% |" if dur else "| HW DRIPS | - |")
        md.append(f"| energy drain / discharge (mWh) | {si.get('EnergyDrain')} / {si.get('Discharge')} |")
        md.append(f"| capacity entry -> exit (mWh) | {si.get('EntryRemainingCapacity')} -> {si.get('ExitRemainingCapacity')} of {si.get('FullChargeCapacity')} |")
        md.append(f"| exit latency (ms), of which screen on | {si.get('CsExitLatencyInMs')}, {si.get('CsExitTimeSpentInScreenOn')} |")
        md.append(f"| PEP pre-vetoes, directed DRIPS transitions | {si.get('PreVetoCount')}, {si.get('DirectedDripsTransitionCount')} |")
        for rec in si.findall(".//s:EnergyMeterRecord", ns):
            md.append(f"| {rec.get('RailId')} | {rec.get('EnergyConsumption')} mJ, {rec.get('PowerConsumption')} mW |")
        md.append("")
        md.append("| phase | % of session |")
        md.append("|---|---|")
        for ph in si.findall("s:Phases/s:Phase", ns):
            md.append(f"| {ph.get('Name')} | {ph.get('ScenarioActiveTimePercent')} |")
        md.append("")
        md.append("| top blocker | type | % of session |")
        md.append("|---|---|---|")
        for b in si.findall("s:TopBlockers/s:TopBlocker", ns):
            md.append(f"| {b.get('Name')} | {b.get('Type')} | {b.get('ScenarioActiveTimePercent')} |")
        for sect in ("Activators", "Devices", "Processors", "PepPreVetoes", "SocSubsystems"):
            for b in si.findall(f"s:{sect}/*", ns):
                if b.get("Name"):
                    md.append(f"| {b.get('Name')} | {sect} | {b.get('ScenarioActiveTimePercent')} |")
        md.append("")
        buckets = [f"{b.get('Interval')}: {b.get('TotalTimePercent')}%" for b in si.findall("s:DripsBuckets/s:DripsBucket", ns)
                   if b.get("TotalTimePercent") not in (None, "0")]
        md.append("DRIPS interval buckets (share of DRIPS time): " + (", ".join(buckets) or "none"))
        md.append("")
    if not picked:
        md.append("no sleepstudy Sleep session matches the standby window")
        md.append("")
    return md


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("run", help="private/windows/standby/<run> directory")
    ap.add_argument("--max-rows", type=int, default=1500)
    a = ap.parse_args()
    run = os.path.abspath(a.run)
    rows = decode(run)
    run_txt = read_text_any(os.path.join(run, "run.txt")) if os.path.exists(os.path.join(run, "run.txt")) else ""
    windows = standby_window(rows, run_txt)

    md = ["# Modern Standby trace summary", ""]
    if run_txt:
        md += ["```", run_txt.strip(), "```", ""]

    md += ["## Provider coverage", "", "| provider | events | first | last |", "|---|---|---|---|"]
    by = collections.defaultdict(list)
    for r in rows:
        by[r["provider"].lower()].append(r)
    for g, name in PROVIDERS.items():
        ev = by.get(g, [])
        md.append(f"| {name} | {len(ev)} | {fmt_t(ev[0]['t']) if ev else '-'} | {fmt_t(ev[-1]['t']) if ev else '-'} |")
    other = sum(len(v) for g, v in by.items() if g not in PROVIDERS)
    md += ["", f"other providers (trace header, system config): {other} events", ""]
    missing = [n for g, n in PROVIDERS.items() if not by.get(g)]
    if missing:
        md += [f"**Not in the trace:** {', '.join(missing)}. A provider with 0 events was either silent or never enabled "
               "by the session; Kernel-Power is never silent across a standby cycle, so 0 there means the capture missed it.", ""]

    md += ["## Standby window", ""]
    for a_, b_, src in windows:
        md.append(f"- {fmt_t(a_)} -> {fmt_t(b_)} UTC ({(b_ - a_).total_seconds():.1f} s, from {src})")
    if not windows:
        md.append("- no 506/507 pair found")
    md.append("")

    md += ["## Event counts (provider | task | event id)", "", "| count | provider | task / event | id |", "|---|---|---|---|"]
    cnt = collections.Counter((short(r), r["task"] or r["event"] or "-", r["id"]) for r in rows)
    for (p, t, i), c in cnt.most_common(60):
        md.append(f"| {c} | {p} | {t} | {i} |")
    md.append("")

    md += ["## Timeline", "",
           "Consecutive identical rows are collapsed. `in` = inside a standby window.", "",
           "| time (UTC) | in | kind | provider | task / event (id) | data |", "|---|---|---|---|---|---|"]
    prev = None
    n = 0
    for r in rows:
        near = not windows or any((a_ - dt.timedelta(seconds=NEAR_S)) <= r["t"] <= (b_ + dt.timedelta(seconds=NEAR_S))
                                  for a_, b_, _ in windows)
        k = keep(r, near)
        if not k:
            continue
        p = props(r)
        sig = (short(r), r["task"], r["id"], p)
        if sig == prev:
            continue
        prev = sig
        inside = any(a_ <= r["t"] <= b_ for a_, b_, _ in windows)
        if len(p) > 220:
            p = p[:220] + "..."
        md.append(f"| {fmt_t(r['t'])} | {'in' if inside else ''} | {k} | {short(r)} | {r['task'] or r['event']} ({r['id']}) | {p.replace('|', '/')} |")
        n += 1
        if n >= a.max_rows:
            md.append(f"| ... | | | | | truncated at {a.max_rows} rows, see trace.tsv |")
            break
    md.append("")

    if windows:
        md += ["## PDC activators while in standby", "", "| activator (task / subtask) | activations | total active s |", "|---|---|---|"]
        acts = collections.defaultdict(lambda: [0, 0.0])
        for r in rows:
            if PROVIDERS.get(r["provider"].lower()) != "PDC" or "Deactivate" not in (r["task"] or ""):
                continue
            if not any(a_ <= r["t"] <= b_ for a_, b_, _ in windows):
                continue
            m = re.search(r"TaskName=([^;]*) ;.*?SubTaskName=([^;]*) ;", r["props"])
            d = re.search(r"ActivationDuration=(\d+)", r["props"])
            key = f"{m.group(1).strip()} / {m.group(2).strip()}" if m else "?"
            acts[key][0] += 1
            acts[key][1] += int(d.group(1)) / 1000 if d else 0
        for k, (c, s) in sorted(acts.items(), key=lambda x: -x[1][1]):
            md.append(f"| {k.replace('|', '/')} | {c} | {s:.3f} |")
        md.append("")

    md += rundown_tables(rows)

    ss = os.path.join(run, "sleepstudy.xml")
    if os.path.exists(ss):
        md += ["## sleepstudy", ""] + sleepstudy(ss, windows)

    out = os.path.join(run, "summary.md")
    with open(out, "w", encoding="utf-8") as f:
        f.write("\n".join(md) + "\n")
    print(f"summary: {out} ({len(rows)} events, {n} timeline rows, missing providers: {len(missing)})")


if __name__ == "__main__":
    sys.exit(main())
