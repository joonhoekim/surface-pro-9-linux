# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
"""Simple Haptic Controller (HID usage page 0x0E) on the Slim Pen 2 BLE HID device."""

import time

from . import hiddesc, hidraw

PAGE = 0x0E
U = lambda uid: hiddesc.usage(PAGE, uid)
SIMPLE_HAPTIC_CONTROLLER = U(0x01)
WAVEFORM_LIST = U(0x10)
DURATION_LIST = U(0x11)
AUTO_TRIGGER = U(0x20)
MANUAL_TRIGGER = U(0x21)
AUTO_TRIGGER_CONTROL = U(0x22)
INTENSITY = U(0x23)
REPEAT_COUNT = U(0x24)
RETRIGGER_PERIOD = U(0x25)
CUTOFF_TIME = U(0x28)
ORDINAL_PAGE = 0x0A

ORDINAL_NONE = 1
ORDINAL_STOP = 2

WAVEFORMS = {
    0x1001: "none", 0x1002: "stop", 0x1003: "click", 0x1004: "buzz-continuous",
    0x1005: "rumble-continuous", 0x1006: "press", 0x1007: "release", 0x1008: "hover",
    0x1009: "success", 0x100A: "error", 0x100B: "ink-continuous", 0x100C: "pencil-continuous",
    0x100D: "marker-continuous", 0x100E: "chisel-marker-continuous", 0x100F: "brush-continuous",
    0x1010: "eraser-continuous", 0x1011: "sparkle-continuous",
}

PEN_IDS = (0x045E, 0x0C0F)

# Feature 66 as the Slim Pen 2 returned it on Windows (ATT Read of the report characteristic at
# reconnect, docs/research/gaps/pen-ble-trace.md): ordinal -> (waveform, duration ms). 16..22 are 0x1001 (none), 0 ms.
# Used when Feature 66 cannot be read. There is no buzz-continuous (0x1004); ordinal 4 is "press".
KNOWN_WAVEFORMS = {
    3: (0x1003, 45), 4: (0x1006, 60), 5: (0x1007, 60), 6: (0x1008, 35), 7: (0x1009, 145),
    8: (0x100A, 185), 9: (0x100B, 1995), 10: (0x100C, 1995), 11: (0x100D, 1995), 12: (0x100E, 1995),
    13: (0x100F, 1980), 14: (0x1010, 1995), 15: (0x1011, 1980),
}
# Waveform names tried in order for each kind of effect.
KINDS = {
    "pulse": ("click",),
    "buzz": ("buzz-continuous", "ink-continuous"),
    "ink": ("ink-continuous",),
    "eraser": ("eraser-continuous", "ink-continuous"),
}
# Inking start report Windows sends (#1066 capture: 41 00 7f 09 00 00 00): intensity 127, no repeat,
# cutoff or retrigger. The ETW trace shows one write ~10-40 ms after tip down and one (stop) ~10-35 ms
# after tip up, nothing periodic in between (a 9.8 s stroke had no writes).
INK_INTENSITY = 0x7F
STOP_INTENSITY = 0xFE   # Windows stop report (#1066): 41 00 fe 02 00 00 00


class Layout:
    """Report IDs and fields of the haptic collection, taken from the descriptor."""

    def __init__(self, desc):
        self.desc = desc
        f = desc.find(hiddesc.OUTPUT, MANUAL_TRIGGER, within=SIMPLE_HAPTIC_CONTROLLER)
        if f is None:
            raise LookupError("no Simple Haptic Controller output report")
        self.output_id = f.report_id
        self.output_size = desc.report_size(hiddesc.OUTPUT, self.output_id)
        f = desc.find(hiddesc.FEATURE, AUTO_TRIGGER, within=SIMPLE_HAPTIC_CONTROLLER)
        self.feature_id = f.report_id if f else None
        f = desc.find(hiddesc.FEATURE, hiddesc.usage(ORDINAL_PAGE, 3), within=WAVEFORM_LIST)
        self.list_id = f.report_id if f else None

    def field(self, u):
        return self.desc.find(hiddesc.OUTPUT, u, report_id=self.output_id)

    def pack_trigger(self, ordinal, intensity, repeat=0, cutoff_ms=0, retrigger_ms=0):
        vals = {MANUAL_TRIGGER: ordinal, INTENSITY: intensity, REPEAT_COUNT: repeat,
                CUTOFF_TIME: self.ms_to_logical(CUTOFF_TIME, cutoff_ms),
                RETRIGGER_PERIOD: self.ms_to_logical(RETRIGGER_PERIOD, retrigger_ms)}
        for u, v in vals.items():
            f = self.field(u)
            if f is not None and not (min(0, f.logical_min) <= v <= f.logical_max):
                raise ValueError("value %d out of range %d..%d for usage 0x%x"
                                 % (v, f.logical_min, f.logical_max, u & 0xFFFF))
        return hiddesc.pack(self.desc, hiddesc.OUTPUT, self.output_id, vals)

    def ms_to_logical(self, u, ms):
        """Scale milliseconds by the field's physical range and unit exponent (seconds, 10^exp)."""
        f = self.field(u)
        if f is None or not ms:
            return 0
        lspan = f.logical_max - f.logical_min
        pspan = f.physical_max - f.physical_min
        if lspan <= 0 or pspan <= 0:
            return min(int(ms), f.logical_max)
        phys_per_ms = 10 ** (-3 - f.unit_exp)
        v = round(ms * phys_per_ms * lspan / pspan)
        return max(f.logical_min, min(f.logical_max, v))

    def parse_waveform_list(self, data):
        """Feature report -> {ordinal: (waveform_id, duration)}."""
        wf = self.desc.find(hiddesc.FEATURE, hiddesc.usage(ORDINAL_PAGE, 3), within=WAVEFORM_LIST)
        du = self.desc.find(hiddesc.FEATURE, hiddesc.usage(ORDINAL_PAGE, 3), within=DURATION_LIST)
        out = {}
        body = int.from_bytes(bytes(data[1:]), "little")
        for f, slot in ((wf, 0), (du, 1)):
            if f is None:
                continue
            for k in range(f.count):
                o = f.usage_at(k) & 0xFFFF
                v = (body >> (f.bit_offset + k * f.size)) & ((1 << f.size) - 1)
                cur = list(out.get(o, (0, 0)))
                cur[slot] = v
                out[o] = tuple(cur)
        return out


def ordinal_for(waveforms, name, default):
    for o, (wid, _) in sorted(waveforms.items()):
        if WAVEFORMS.get(wid) == name:
            return o
    return default


def ordinal_for_kind(waveforms, kind):
    """First ordinal whose waveform matches the kind's preference list (pen list, then KNOWN_WAVEFORMS)."""
    for table in (waveforms, KNOWN_WAVEFORMS):
        for name in KINDS[kind]:
            o = ordinal_for(table or {}, name, None)
            if o is not None:
                return o
    raise LookupError("no waveform for %r" % kind)


def find_nodes(sys_root=hidraw.SYS_HIDRAW):
    out = []
    for n in hidraw.nodes(sys_root):
        ids = n.ids
        if ids and ids[1:] == PEN_IDS and n.descriptor.has_application(hiddesc.usage(0x0D, 0x20)):
            try:
                Layout(n.descriptor)
            except LookupError:
                continue
            out.append(n)
    return out


class PenHaptics:
    """Library entry point. `trigger()` is cheap enough to call from a pressure loop."""

    def __init__(self, node=None):
        if node is None:
            found = find_nodes()
            if not found:
                raise FileNotFoundError("Slim Pen 2 haptic hidraw node not found (pen asleep or not paired?)")
            node = found[0]
        self.node = node
        self.layout = Layout(node.descriptor)
        self.dev = hidraw.Device(node.dev, write=True)
        self.waveforms = {}
        try:
            self.waveforms = self.read_waveforms()
        except OSError:
            pass

    def close(self):
        self.dev.close()

    def read_waveforms(self):
        if self.layout.list_id is None:
            return {}
        size = self.layout.desc.report_size(hiddesc.FEATURE, self.layout.list_id)
        return self.layout.parse_waveform_list(self.dev.get_feature(self.layout.list_id, size))

    def ordinal(self, kind):
        return ordinal_for_kind(self.waveforms, kind)

    def trigger(self, ordinal, intensity, repeat=0, cutoff_ms=0, retrigger_ms=0):
        self.dev.write_output(self.layout.pack_trigger(ordinal, intensity, repeat, cutoff_ms, retrigger_ms))

    def stop(self):
        self.trigger(ORDINAL_STOP, STOP_INTENSITY)

    def pulse(self, intensity, ms):
        self.trigger(self.ordinal("pulse"), intensity, cutoff_ms=ms)

    def buzz(self, intensity, ms):
        limit = self.layout.field(CUTOFF_TIME)
        max_ms = 2540 if limit is None else (limit.physical_max or 2540)
        self.trigger(self.ordinal("buzz"), intensity, cutoff_ms=ms if ms <= max_ms else 0)
        time.sleep(ms / 1000.0)
        self.stop()


class InkFeedback:
    """Windows-style inking haptics: start a continuous waveform on contact, stop on release.

    Call `update(contact)` from the stylus event loop; it writes only on edges. `haptics` is a
    PenHaptics (or anything with trigger/stop/ordinal)."""

    def __init__(self, haptics, kind="ink", intensity=INK_INTENSITY):
        self.h = haptics
        self.kind = kind
        self.intensity = intensity
        self.active = False

    def update(self, contact, kind=None):
        if contact and not self.active:
            self.h.trigger(self.h.ordinal(kind or self.kind), self.intensity)
        elif not contact and self.active:
            self.h.stop()
        self.active = bool(contact)


def pressure_to_intensity(pressure, max_pressure=4096, floor=0.05, lo=1, hi=255):
    """Map stylus pressure to an intensity value; 0 below `floor` (caller then sends stop)."""
    x = pressure / float(max_pressure)
    if x < floor:
        return 0
    return int(lo + (hi - lo) * min(1.0, x))
