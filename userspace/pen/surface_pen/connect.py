# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
"""surface-pen-connect: keep the Slim Pen 2 trusted and reconnect it when the digitizer sees it."""

import argparse
import asyncio
import glob
import os
import struct
import sys
import time

from . import bluez, col07, hiddesc, hidraw

EV_KEY = 0x01
BTN_TOOL_PEN = 0x140
BTN_TOOL_RUBBER = 0x141
BTN_TOUCH = 0x14A
EVENT = struct.Struct("llHHi")
STYLUS_NAME = "IPTSD Virtual Stylus"
# How often `run` looks for the stylus evdev while it has none. iptsd runs as a udev-started
# template instance, so nothing orders this helper after it, and a restarted iptsd creates a
# new uinput node under another event number.
EVDEV_RETRY_S = 5.0


def log(*a):
    print(*a, flush=True)


def find_stylus_evdev(sys_root="/sys/class/input"):
    for p in sorted(glob.glob(os.path.join(sys_root, "event*"))):
        try:
            with open(os.path.join(p, "device", "name")) as f:
                if f.read().strip().startswith(STYLUS_NAME):
                    return "/dev/input/" + os.path.basename(p)
        except OSError:
            continue
    return None


def is_pen_seen(ev_type, code, value):
    return ev_type == EV_KEY and code in (BTN_TOOL_PEN, BTN_TOOL_RUBBER, BTN_TOUCH) and value == 1


class Helper:
    def __init__(self, args):
        self.args = args
        self.bz = None
        self.last_attempt = 0.0
        self.busy = False
        self.node = None
        self.lsb = args.addr_order == "lsb"
        self.evdev_fd = None
        self.evdev_missing_logged = False

    async def setup_bluez(self):
        self.bz = await bluez.BlueZ().connect()

    async def ensure_trusted(self):
        for path, p in (await self.bz.pens()).items():
            name = hidraw.mask_addr(p.get("Address", ""), self.args.show_addr)
            log("pen %s paired=%s trusted=%s connected=%s" % (name, p.get("Paired"), p.get("Trusted"), p.get("Connected")))
            if p.get("Paired") and not p.get("Trusted") and not self.args.no_trust:
                await self.bz.set_trusted(path)
                log("  set Trusted")

    async def on_seen(self, source, sighting=None):
        now = time.monotonic()
        if self.busy or now - self.last_attempt < self.args.cooldown:
            return
        self.busy, self.last_attempt = True, now
        try:
            pens = await self.bz.pens()
            if sighting is not None and any(sighting.pen_addr):
                self.match_addr(sighting, pens)
            for path, p in pens.items():
                if p.get("Connected"):
                    continue
                if not p.get("Paired"):
                    if not self.args.pair:
                        log("pen seen (%s) but not paired; pair once with the top button held, or use --pair" % source)
                        continue
                    log("pairing (%s)" % source)
                    await self.bz.pair(path)
                    if not self.args.no_trust:
                        await self.bz.set_trusted(path)
                log("connecting (%s)" % source)
                try:
                    await asyncio.wait_for(self.bz.connect_device(path), self.args.timeout)
                    log("  connected")
                except Exception as e:
                    log("  connect failed: %s" % e)
        finally:
            self.busy = False

    def match_addr(self, s, pens):
        for path, p in pens.items():
            a = p.get("Address", "")
            if a == s.addr(True):
                log("Col07 address matches BlueZ pen (LSB-first)")
            elif a == s.addr(False):
                log("Col07 address matches BlueZ pen (MSB-first)")
            else:
                log("Col07 address does not match BlueZ pen in either byte order")

    def watch_evdev(self, loop, path):
        fd = os.open(path, os.O_RDONLY | os.O_NONBLOCK)

        def ready():
            try:
                data = os.read(fd, EVENT.size * 64)
            except BlockingIOError:
                return
            except OSError as e:
                data, why = b"", str(e)
            else:
                why = "end of file"
            if not data:
                # The node is gone (ENODEV once iptsd removes its uinput device). The fd stays
                # readable, so leaving the reader in place would call this in a busy loop.
                loop.remove_reader(fd)
                os.close(fd)
                self.evdev_fd = None
                log("lost %s (%s)" % (path, why))
                return
            for off in range(0, len(data) - EVENT.size + 1, EVENT.size):
                _, _, t, c, v = EVENT.unpack_from(data, off)
                if is_pen_seen(t, c, v):
                    asyncio.ensure_future(self.on_seen("evdev"))
                    break

        loop.add_reader(fd, ready)
        self.evdev_fd = fd
        log("watching %s" % path)

    def ensure_evdev(self, loop):
        """Watch the stylus evdev if it is not watched; True when it is. Called periodically."""
        if self.evdev_fd is not None:
            return True
        path = self.args.evdev or find_stylus_evdev()
        if path:
            try:
                self.watch_evdev(loop, path)
                self.evdev_missing_logged = False
                return True
            except OSError as e:
                path = "%s: %s" % (path, e)
        if not self.evdev_missing_logged:
            log("no '%s' evdev node yet (%s); looking again every %g s"
                % (STYLUS_NAME, path or "is iptsd running?", EVDEV_RETRY_S))
            self.evdev_missing_logged = True
        return False

    def watch_hidraw(self, loop, node):
        dev = hidraw.Device(node.dev)

        def ready():
            try:
                data = dev.read()
            except BlockingIOError:
                return
            s = col07.parse_input(node.descriptor, data)
            if s is not None:
                if self.args.verbose:
                    log(s.describe(self.args.show_addr, self.lsb))
                asyncio.ensure_future(self.on_seen("col07", s))

        loop.add_reader(dev.fd, ready)
        log("watching %r for reports %s" % (node, col07.INPUT_IDS))


async def write_host_features(args, node):
    bz = await bluez.BlueZ().connect()
    adapters = await bz.adapters()
    if not adapters:
        raise SystemExit("no Bluetooth adapter")
    addr = next(iter(adapters.values()))["Address"]
    desc = node.descriptor
    lsb = args.addr_order == "lsb"
    with hidraw.Device(node.dev, write=True) as dev:
        before = col07.read_features(dev, desc)
        prev = before.get(col07.FEATURE_HOST_ADDR)
        payload = col07.host_addr_feature(desc, addr, lsb, base=prev if isinstance(prev, bytes) else None)
        dev.set_feature(payload)
        log("SET_FEATURE %d sent (host address, %s-first)" % (col07.FEATURE_HOST_ADDR, args.addr_order))
        usages = [int(u, 0) | (0xFFF4 << 16) for u in args.ab_usage]
        if usages:
            base = before.get(col07.FEATURE_AB_CAPABILITY)
            dev.set_feature(col07.ab_capability_feature(desc, usages, base if isinstance(base, bytes) else None))
            log("SET_FEATURE %d sent (usages %s)" % (col07.FEATURE_AB_CAPABILITY, ",".join(args.ab_usage)))
        after = col07.read_features(dev, desc)
        print_features(after, args.show_addr, lsb)


def print_features(feats, show, lsb):
    for rid, v in feats.items():
        if isinstance(v, Exception):
            log("feature %d: %s" % (rid, v))
        elif rid == col07.FEATURE_HOST_ADDR:
            a = hidraw.format_addr(v[1:7], lsb) if any(v[1:7]) else "all zero"
            log("feature %d: host addr %s, tail %s" % (rid, hidraw.mask_addr(a, show) if any(v[1:7]) else a, v[7:].hex()))
        else:
            log("feature %d: %s" % (rid, v.hex()))


def print_layout(desc):
    for kind in (hiddesc.INPUT, hiddesc.OUTPUT, hiddesc.FEATURE):
        for rid in desc.report_ids(kind):
            fs = [f for f in desc.report(kind, rid) if any((u >> 16) in (0xFFF4, 0xFF0F) for u in f.usages)
                  or f.in_collection(col07.TLC)]
            if not fs:
                continue
            parts = []
            for f in fs:
                u = "const" if f.constant else ",".join("%04x:%04x" % (x >> 16, x & 0xFFFF) for x in sorted(set(f.usages)))
                parts.append("%s@%d %dx%d" % (u, f.bit_offset, f.size, f.count))
            log("  %-7s %3d (%d B): %s" % (kind, rid, desc.report_size(kind, rid), "  ".join(parts)))


async def cmd_probe(args):
    nodes = col07.find_nodes()
    if not nodes:
        log("no hidraw node with an FFF4:0001 application collection")
    for n in nodes:
        log("%r" % n)
        print_layout(n.descriptor)
        try:
            with hidraw.Device(n.dev) as dev:
                print_features(col07.read_features(dev, n.descriptor), args.show_addr, args.addr_order == "lsb")
        except OSError as e:
            log("  open: %s" % e)
    log("stylus evdev: %s" % find_stylus_evdev())
    try:
        bz = await bluez.BlueZ().connect()
        pens = await bz.pens()
        if not pens:
            log("BlueZ: no Slim Pen 2 known")
        for p in pens.values():
            log("BlueZ pen %s paired=%s trusted=%s connected=%s modalias=%s" % (
                hidraw.mask_addr(p.get("Address", ""), args.show_addr), p.get("Paired"),
                p.get("Trusted"), p.get("Connected"), p.get("Modalias")))
    except Exception as e:
        log("BlueZ: %s" % e)


async def cmd_run(args, monitor_only=False):
    h = Helper(args)
    loop = asyncio.get_running_loop()
    if not monitor_only:
        await h.setup_bluez()
        await h.ensure_trusted()
        if args.write_host_features:
            nodes = col07.find_nodes()
            if nodes:
                await write_host_features(args, nodes[0])
    else:
        args.verbose = True

        async def noop(source, sighting=None):
            if sighting is None:
                log("pen seen (%s)" % source)
        h.on_seen = noop
    triggers = args.trigger.split(",")
    if "col07" in triggers:
        nodes = col07.find_nodes()
        if nodes:
            h.watch_hidraw(loop, nodes[0])
        else:
            log("no Col07 hidraw node")
    next_trust = time.monotonic() + args.poll
    while True:
        if "evdev" in triggers:
            h.ensure_evdev(loop)
        await asyncio.sleep(EVDEV_RETRY_S)
        if args.poll and not monitor_only and time.monotonic() >= next_trust:
            next_trust = time.monotonic() + args.poll
            await h.ensure_trusted()


def main(argv=None):
    ap = argparse.ArgumentParser(prog="surface-pen-connect", description=__doc__)
    sub = ap.add_subparsers(dest="cmd")
    for name in ("probe", "monitor", "run"):
        s = sub.add_parser(name)
        s.add_argument("--show-addr", action="store_true", help="print full BD addresses")
        s.add_argument("--addr-order", choices=("lsb", "msb"), default="lsb",
                       help="byte order of 6-byte address fields in Col07 reports (guess: lsb)")
        if name != "probe":
            s.add_argument("--trigger", default="evdev,col07", help="evdev, col07 or both (comma separated)")
            s.add_argument("--evdev", help="stylus event node (default: find '%s')" % STYLUS_NAME)
            s.add_argument("--verbose", action="store_true")
            s.add_argument("--cooldown", type=float, default=5.0, help="seconds between connect attempts")
            s.add_argument("--timeout", type=float, default=10.0)
            s.add_argument("--poll", type=float, default=0, help="re-check trust every N seconds")
            s.add_argument("--no-trust", action="store_true", help="do not set Trusted")
            s.add_argument("--pair", action="store_true", help="also Pair() a known but unpaired pen")
            s.add_argument("--write-host-features", action="store_true",
                           help="opt-in: SET_FEATURE 86 (host address) and 112 on the digitizer")
            s.add_argument("--ab-usage", action="append", default=[],
                           help="FFF4 usage to set in feature 112 with --write-host-features, e.g. 0x10")
    args = ap.parse_args(argv)
    if args.cmd is None:
        args = ap.parse_args(["probe"] + (argv or sys.argv[1:]))
    try:
        if args.cmd == "probe":
            asyncio.run(cmd_probe(args))
        else:
            asyncio.run(cmd_run(args, monitor_only=args.cmd == "monitor"))
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
