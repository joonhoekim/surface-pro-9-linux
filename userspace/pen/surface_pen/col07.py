# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
"""THC digitizer collection FFF4:0001 (pen BLE address adaptation), as seen on Windows as Col07."""

import hashlib
from dataclasses import dataclass

from . import hiddesc, hidraw

P = lambda uid: hiddesc.usage(0xFFF4, uid)
TLC = P(0x0001)
PEN_ID = hiddesc.usage(0xFF0F, 0x0050)
PEN_ADDR = P(0x0002)
HOST_ADDR = P(0x0003)
FLAG_0008 = P(0x0008)
FLAG_0009 = P(0x0009)
FLAG_0010 = P(0x0010)
FLAG_0011 = P(0x0011)
FLAG_0012 = P(0x0012)
FLAG_0021 = P(0x0021)
FLAG_0022 = P(0x0022)

INPUT_IDS = (84, 110)
FEATURE_HOST_ADDR = 86
FEATURE_AB_CAPABILITY = 112
FEATURE_115 = 115

DIGITIZER_IDS = (0x045E, 0x0C52)


# The pen has a second address for the host it is coupled with through the cover slot. Windows'
# SurfacePenBleLcAddrAdaptationDriver derives it; the constant is that driver's.
_COUPLED_CONST = bytes.fromhex("f876af012e1c2840918f6f605e6b1fd6")
_ADDR_MASK = (1 << 46) - 1
_ADDR_STATIC = 0xC00000000000


def coupled_address(pen_id, host_addr=None):
    """Static random address the pen uses toward the host it is coupled with (out-of-band, cover slot).

    pen_id: the four FF0F:0050 bytes as they are in the input report. host_addr: 'AA:BB:..' of the adapter,
    as written to Feature 86; None for the variant without a host (input report 84 or flag 0012 clear).
    Returns 'AA:BB:CC:DD:EE:FF'.
    """
    h = hashlib.sha1(_COUPLED_CONST + bytes(pen_id)[::-1]).digest()
    q = (int.from_bytes(h[:8], "little") & _ADDR_MASK) | _ADDR_STATIC
    q = ~q if host_addr is None else q ^ int(host_addr.replace(":", ""), 16)
    q = (q & _ADDR_MASK) | _ADDR_STATIC
    return ":".join("%02X" % b for b in q.to_bytes(6, "big"))


def find_nodes(sys_root=hidraw.SYS_HIDRAW):
    return [n for n in hidraw.nodes(sys_root) if n.descriptor.has_application(TLC)]


@dataclass
class Sighting:
    report_id: int
    pen_id: bytes
    pen_addr: bytes
    flags: dict

    def addr(self, lsb_first=True):
        return hidraw.format_addr(self.pen_addr, lsb_first)

    def describe(self, show=False, lsb_first=True):
        fl = " ".join("%04x=%d" % (u & 0xFFFF, v) for u, v in sorted(self.flags.items()))
        pid = self.pen_id.hex() if show else "%d bytes" % len(self.pen_id)
        a = "-" if not any(self.pen_addr) else hidraw.mask_addr(self.addr(lsb_first), show)
        return "report %d pen_id %s addr %s flags %s" % (self.report_id, pid, a, fl)


def parse_input(desc, data):
    if not data or data[0] not in INPUT_IDS:
        return None
    rid, vals = hiddesc.unpack(desc, hiddesc.INPUT, data)
    pid = vals.get(PEN_ID, [])
    addr = vals.get(PEN_ADDR, [])
    flags = {u: v for u, v in vals.items() if u not in (PEN_ID, PEN_ADDR)}
    return Sighting(rid, bytes(pid if isinstance(pid, list) else [pid]),
                    bytes(addr if isinstance(addr, list) else [addr]), flags)


def host_addr_feature(desc, addr, lsb_first=True, base=None, flag=None):
    """SET_FEATURE 86 payload carrying the host BD address ('AA:BB:...')."""
    b = bytes(int(x, 16) for x in addr.split(":"))
    if lsb_first:
        b = b[::-1]
    vals = {HOST_ADDR: b}
    if flag is not None:
        vals[FLAG_0009] = flag
    return hiddesc.pack(desc, hiddesc.FEATURE, FEATURE_HOST_ADDR, vals, base)


def ab_capability_feature(desc, usages, base=None):
    """SET_FEATURE 112 payload with the given flag usages set to 1."""
    return hiddesc.pack(desc, hiddesc.FEATURE, FEATURE_AB_CAPABILITY, {u: 1 for u in usages}, base)


def read_features(dev, desc):
    out = {}
    for rid in (FEATURE_HOST_ADDR, FEATURE_AB_CAPABILITY, FEATURE_115):
        if not desc.report(hiddesc.FEATURE, rid):
            continue
        try:
            out[rid] = dev.get_feature(rid, desc.report_size(hiddesc.FEATURE, rid))
        except OSError as e:
            out[rid] = e
    return out
