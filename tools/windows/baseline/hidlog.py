# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
"""Log raw HID input reports from Surface collections through hidapi (read-only: never writes or sends feature/output reports).

Usage: python -I hidlog.py --out DIR [--select thc,ble,kip] [--seconds N]
Stops when DIR/stop exists (or after --seconds). Writes:
  DIR/hid-collections.txt   every matching collection, whether it opened, and why not
  DIR/hid-<pid>-<page>-<usage>.bin   records: <d t (unix s)><H len><bytes>
Windows opens digitizer/keyboard collections exclusively, so those fail here by design; the pointer/raw-input probe covers them.
Collections with restrictive ACLs (THC Col04 heatmap, Col07 pen bonding) open only elevated.
"""
import argparse
import os
import struct
import sys
import threading
import time

import hid

PIDS = {"thc": [0x0C52], "ble": [0x0C0F], "kip": [0x09B0]}


def reader(dev, path, stop, stats, key):
    with open(path, "ab") as f:
        while not stop.is_set():
            try:
                r = dev.read(8192, timeout_ms=200)
            except Exception as e:  # device went away (pen asleep)
                stats[key]["error"] = str(e)
                break
            if r:
                b = bytes(r)
                f.write(struct.pack("<dH", time.time(), len(b)) + b)
                stats[key]["n"] += 1
    dev.close()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", required=True)
    ap.add_argument("--select", default="thc,ble,kip")
    ap.add_argument("--seconds", type=float, default=0)
    a = ap.parse_args()
    os.makedirs(a.out, exist_ok=True)
    pids = [p for s in a.select.split(",") for p in PIDS.get(s.strip(), [])]
    stop = threading.Event()
    stats, threads, lines = {}, [], []
    elevated = False
    try:
        import ctypes
        elevated = bool(ctypes.windll.shell32.IsUserAnAdmin())
    except Exception:
        pass
    lines.append(f"elevated {int(elevated)}")
    seen = set()
    for pid in pids:
        for d in hid.enumerate(0x045E, pid):
            if d["path"] in seen:
                continue
            seen.add(d["path"])
            key = f"{pid:04x}-{d['usage_page']:04x}-{d['usage']:04x}"
            if key in stats:
                key += f"-{len(stats)}"
            dev = hid.device()
            try:
                dev.open_path(d["path"])
                dev.set_nonblocking(0)
                # A first read distinguishes "opened but read access denied" (exclusive digitizer TLCs) from a usable handle.
                first = dev.read(8192, timeout_ms=50)
            except Exception as e:
                lines.append(f"{key} open_fail {e}")
                try:
                    dev.close()
                except Exception:
                    pass
                continue
            path = os.path.join(a.out, f"hid-{key}.bin")
            stats[key] = {"n": 0}
            if first:
                with open(path, "ab") as f:
                    b = bytes(first)
                    f.write(struct.pack("<dH", time.time(), len(b)) + b)
                stats[key]["n"] = 1
            lines.append(f"{key} open_ok")
            t = threading.Thread(target=reader, args=(dev, path, stop, stats, key), daemon=True)
            t.start()
            threads.append(t)
    if not pids or not seen:
        lines.append("no matching collections (pen asleep or not paired?)")
    with open(os.path.join(a.out, "hid-collections.txt"), "w", encoding="utf-8") as f:
        f.write("\n".join(lines) + "\n")
    t0 = time.time()
    stopfile = os.path.join(a.out, "stop")
    while not os.path.exists(stopfile) and not (a.seconds and time.time() - t0 > a.seconds):
        time.sleep(0.2)
    stop.set()
    for t in threads:
        t.join(2)
    with open(os.path.join(a.out, "hid-collections.txt"), "a", encoding="utf-8") as f:
        for k, v in stats.items():
            f.write(f"{k} reports {v['n']}" + (f" error {v['error']}" if "error" in v else "") + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
