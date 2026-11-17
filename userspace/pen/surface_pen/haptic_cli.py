# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
"""pen-haptic: Slim Pen 2 haptics over its BLE HID hidraw node. Without a command it only reads."""

import argparse
import sys

from . import haptic, hiddesc, hidraw


def discover(args):
    nodes = [hidraw.Node(args.node.replace("/dev/", ""))] if args.node else haptic.find_nodes()
    if not nodes:
        print("no Slim Pen 2 hidraw node with a Simple Haptic Controller (pen asleep or not paired?)")
        return 1
    for n in nodes:
        lay = haptic.Layout(n.descriptor)
        print("%r" % n)
        print("  output report %d (%d B):" % (lay.output_id, lay.output_size))
        for f in n.descriptor.report(hiddesc.OUTPUT, lay.output_id):
            u = f.usage_at(0)
            print("    0x%02x @bit %d %dx%d logical %d..%d physical %d..%d exp %d" % (
                u & 0xFFFF, f.bit_offset, f.size, f.count, f.logical_min, f.logical_max,
                f.physical_min, f.physical_max, f.unit_exp))
        try:
            with hidraw.Device(n.dev) as dev:
                for rid in (lay.feature_id, lay.list_id):
                    if rid is None:
                        continue
                    data = dev.get_feature(rid, n.descriptor.report_size(hiddesc.FEATURE, rid))
                    print("  feature %d: %s" % (rid, data.hex()))
                    if rid == lay.list_id:
                        for o, (wid, dur) in sorted(lay.parse_waveform_list(data).items()):
                            print("    ordinal %2d: waveform 0x%04x %-24s duration %d" % (
                                o, wid, haptic.WAVEFORMS.get(wid, "vendor" if wid >= 0x2000 else "?"), dur))
        except OSError as e:
            print("  feature read: %s" % e)
    return 0


def main(argv=None):
    ap = argparse.ArgumentParser(prog="pen-haptic", description=__doc__)
    ap.add_argument("--node", help="hidraw node (default: auto)")
    sub = ap.add_subparsers(dest="cmd")
    sub.add_parser("discover")
    for name in ("pulse", "buzz", "stop"):
        s = sub.add_parser(name)
        if name != "stop":
            s.add_argument("--intensity", type=int, default=128, help="1..255 (logical range of the report)")
            s.add_argument("--ms", type=int, default=30 if name == "pulse" else 300)
            s.add_argument("--waveform", type=int, help="override the waveform ordinal (Manual Trigger)")
        s.add_argument("--dry-run", action="store_true", help="print the report instead of writing it")
    args = ap.parse_args(argv)
    if args.cmd in (None, "discover"):
        return discover(args)
    if args.dry_run:
        nodes = [hidraw.Node(args.node.replace("/dev/", ""))] if args.node else haptic.find_nodes()
        if not nodes:
            print("no pen node; dry run needs the descriptor")
            return 1
        lay = haptic.Layout(nodes[0].descriptor)
        if args.cmd == "stop":
            data = lay.pack_trigger(haptic.ORDINAL_STOP, haptic.STOP_INTENSITY)
        else:
            o = args.waveform or haptic.ordinal_for_kind({}, args.cmd)
            data = lay.pack_trigger(o, args.intensity, cutoff_ms=min(args.ms, 2540))
        print(data.hex())
        return 0
    node = hidraw.Node(args.node.replace("/dev/", "")) if args.node else None
    h = haptic.PenHaptics(node)
    try:
        if args.cmd == "stop":
            h.stop()
        elif args.waveform:
            h.trigger(args.waveform, args.intensity, cutoff_ms=min(args.ms, 2540))
        elif args.cmd == "pulse":
            h.pulse(args.intensity, args.ms)
        else:
            h.buzz(args.intensity, args.ms)
    finally:
        h.close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
