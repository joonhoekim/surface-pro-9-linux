# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
"""Host -> digitizer pen feedback (THC feature report 9, usage FF00:C9).

The pen plays the continuous haptic waveforms (ink, pencil, ...) only while the digitizer has a valid pen
speed from the host: the speed is computed by the host, handed back to the digitizer in this report and
passed on to the pen. The digitizer keeps the last value it got. docs/research/gaps/pen-vendor-exchange.md.

Messages are 64 bytes with the report id; all start with 8e <type>.
"""
import os
import random
import struct
import time

from . import connect, haptic, hiddesc, hidraw

REPORT_ID = 9
REPORT_LEN = 64
DIGITIZER_IDS = (0x045E, 0x0C52)
FFF4_FHID_REPORT = 115          # FFF4:0021 on the same device: the FastHostId the firmware has (fe ff: none yet)

T_DISPLAY = 0xA1
T_PEN = 0xA5

SPEED_INVALID = 0xFF
SPEED_MAX = 63
ID_SLOT_NONE = 0xFF             # byte 37: an id slot, ff for a pen the host has no id for
ATTR_BASE = 0x4C                # byte 38: bits 2, 3 and 6 always set; bit 1 is contact
ATTR_CONTACT = 0x02
MODE_HOVER = 3                  # byte 43: 3 in hover and for the eraser end, 1 in tip contact
MODE_CONTACT = 1
ALTERNATING = (2, 4)            # byte 44: alternates from frame to frame
FLAGS = 0x0352                  # bytes 47-48 without a pen id
FLAG_SPEED = 0x0400             # bytes 47-48: the speed is valid

EV_SYN, EV_KEY, EV_ABS = 0, 1, 3
ABS_X, ABS_Y = 0, 1
BTN_TOOL_PEN, BTN_TOOL_RUBBER, BTN_TOUCH = 0x140, 0x141, 0x14A
EVENT = struct.Struct("llHHi")


def display_states(fhid, pen_id=0, display_state=1, hinge=0x190):
    """8e a1: display state, hinge angle and the FastHostId. The firmware echoes it in feature 17."""
    b = bytearray(REPORT_LEN)
    b[0:3] = bytes([REPORT_ID, 0x8E, T_DISPLAY])
    b[3] = display_state
    struct.pack_into("<I", b, 5, hinge)
    struct.pack_into("<I", b, 37, pen_id)
    struct.pack_into("<H", b, 41, fhid)
    return bytes(b)


def pen_feedback(seq, fhid, speed=SPEED_INVALID, contact=False, eraser=False):
    """8e a5: the pen feedback frame the Windows host sends for a pen it has no id for (measured), as
    sp9-helperd sends it (docs/research/gaps/pen-vendor-exchange.md 1.8).

    The speed goes out only for tip contact; hover and the eraser end carry "no speed".
    """
    tip = contact and not eraser
    if not tip:
        speed = SPEED_INVALID
    b = bytearray(REPORT_LEN)
    b[0:3] = bytes([REPORT_ID, 0x8E, T_PEN])
    b[3] = seq & 0xFF
    b[4] = 0x02
    b[5] = 0x01
    b[7:9] = bytes([0x90, 0x01])
    b[37] = ID_SLOT_NONE
    b[38] = ATTR_BASE | (ATTR_CONTACT if contact else 0)
    struct.pack_into("<H", b, 40, fhid)
    b[42] = 1
    b[43] = MODE_CONTACT if tip else MODE_HOVER
    b[44] = ALTERNATING[seq & 1]
    b[45] = min(speed, SPEED_MAX) if speed != SPEED_INVALID else SPEED_INVALID
    flags = FLAGS | (FLAG_SPEED if speed != SPEED_INVALID else 0)
    struct.pack_into("<H", b, 47, flags)
    return bytes(b)


def quantize(v_mm_s, full_scale_mm_s):
    """Pen speed as the 6-bit value of the report."""
    if v_mm_s <= 0:
        return 0
    return int(round(min(v_mm_s / full_scale_mm_s, 1.0) * SPEED_MAX))


class SpeedEstimator:
    """Speed from stylus positions: pairs of samples 14..32 ms apart,
    smoothed over the last three estimates with weights 0.5, 0.33, 0.17."""

    MIN_DT, MAX_DT = 0.014, 0.032
    WEIGHTS = (0.5, 0.33, 0.17)

    def __init__(self):
        self.samples = []      # (t, x_mm, y_mm), newest last
        self.estimates = []    # mm/s, newest first

    def reset(self):
        self.samples.clear()
        self.estimates.clear()

    def add(self, t, x_mm, y_mm):
        self.samples.append((t, x_mm, y_mm))
        while self.samples and t - self.samples[0][0] > self.MAX_DT:
            self.samples.pop(0)
        for t0, x0, y0 in self.samples:
            dt = t - t0
            if dt >= self.MIN_DT:
                v = ((x_mm - x0) ** 2 + (y_mm - y0) ** 2) ** 0.5 / dt
                self.estimates.insert(0, v)
                del self.estimates[len(self.WEIGHTS):]
                break
        return self.value()

    def value(self):
        if not self.estimates:
            return 0.0
        w = self.WEIGHTS[:len(self.estimates)]
        return sum(a * b for a, b in zip(self.estimates, w)) / sum(w)


def find_digitizer(sys_root=hidraw.SYS_HIDRAW):
    for n in hidraw.nodes(sys_root):
        ids = n.ids
        if ids and ids[1:] == DIGITIZER_IDS and n.descriptor.report(hiddesc.FEATURE, REPORT_ID):
            return n
    return None


def load_fhid(path, value=None):
    """The 10-bit FastHostId: given, or kept in `path` (made up once and kept)."""
    if value is not None:
        return value & 0x3FF
    try:
        with open(path) as f:
            return int(f.read().strip(), 0) & 0x3FF
    except (OSError, ValueError):
        pass
    v = random.SystemRandom().randrange(1, 0x400)
    try:
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "w") as f:
            f.write("0x%04x\n" % v)
    except OSError as e:
        connect.log("cannot keep the FastHostId in %s: %s" % (path, e))
    return v


def abs_resolution(fd, axis):
    """Units per mm of an absolute axis (EVIOCGABS)."""
    import fcntl
    buf = bytearray(24)
    fcntl.ioctl(fd, 0x80184540 + axis, buf, True)
    res = struct.unpack("6i", buf)[5]
    return res or 1


class Daemon:
    KEEPALIVE_S = 1.0          # resend an unchanged value while the pen is in range
    STILL_S = 0.06             # no position for this long while in range: the pen stands still
    MIN_INTERVAL_S = 0.007     # the Windows host writes Feature 9 about 130 times a second
    DISPLAY_EVERY_S = 30.0

    def __init__(self, args):
        self.args = args
        self.fhid = load_fhid(args.state, args.fast_host_id)
        self.seq = 0
        self.dev = None
        self.speed = SpeedEstimator()
        self.sent = None
        self.sent_at = 0.0
        self.display_at = 0.0
        self.in_range = False
        self.tip = False
        self.rubber = False
        self.ink = None
        self.pen_haptics = None

    def open_digitizer(self):
        node = find_digitizer()
        if node is None:
            return False
        self.dev = hidraw.Device(node.dev, write=True)
        connect.log("digitizer %s, FastHostId 0x%04x" % (node.dev, self.fhid))
        return True

    def send(self, data):
        try:
            self.dev.set_feature(data)
            return True
        except OSError as e:
            connect.log("feature %d: %s" % (REPORT_ID, e))
            return False

    def send_display(self, now):
        if self.send(display_states(self.fhid)):
            self.display_at = now
            try:
                got = self.dev.get_feature(FFF4_FHID_REPORT, 3)
                if got[1:3] != struct.pack("<H", self.fhid):
                    connect.log("digitizer reports FastHostId %s" % got[1:3].hex(" "))
            except OSError:
                pass

    def send_feedback(self, now, q, force=False):
        state = (q, self.tip, self.rubber)
        if not force and state == self.sent and now - self.sent_at < self.KEEPALIVE_S:
            return
        if now - self.sent_at < self.MIN_INTERVAL_S:
            return
        if self.send(pen_feedback(self.seq, self.fhid, q, self.tip, self.rubber)):
            self.seq += 1
            self.sent, self.sent_at = state, now

    def ink_update(self):
        """Start the inking waveform on contact and stop it on release, like Windows applications do."""
        if not self.args.ink_haptics:
            return
        try:
            if self.pen_haptics is None:
                if not haptic.find_nodes():
                    return
                self.pen_haptics = haptic.PenHaptics()
                self.ink = haptic.InkFeedback(self.pen_haptics, intensity=self.args.ink_intensity)
            self.ink.update(self.tip, "eraser" if self.rubber else "ink")
        except OSError:
            # The pen went to sleep; look for its node again at the next contact.
            if self.pen_haptics is not None:
                self.pen_haptics.close()
            self.pen_haptics = self.ink = None

    def run(self):
        import select
        while not self.open_digitizer():
            time.sleep(connect.EVDEV_RETRY_S)
        self.send_display(time.monotonic())
        while True:
            path = connect.find_stylus_evdev()
            if path is None:
                time.sleep(connect.EVDEV_RETRY_S)
                continue
            try:
                fd = os.open(path, os.O_RDONLY | os.O_NONBLOCK)
            except OSError:
                time.sleep(connect.EVDEV_RETRY_S)
                continue
            connect.log("stylus " + path)
            try:
                rx, ry = abs_resolution(fd, ABS_X), abs_resolution(fd, ABS_Y)
                x = y = None
                moved = False
                last_pos = 0.0
                while True:
                    ready = select.select([fd], [], [], self.STILL_S if self.in_range else None)[0]
                    now = time.monotonic()
                    if not ready:
                        if self.in_range and now - last_pos >= self.STILL_S:
                            self.speed.reset()
                            self.send_feedback(now, 0)
                        continue
                    data = os.read(fd, EVENT.size * 64)
                    for off in range(0, len(data) - EVENT.size + 1, EVENT.size):
                        _, _, etype, code, value = EVENT.unpack_from(data, off)
                        if etype == EV_ABS and code == ABS_X:
                            x, moved = value, True
                        elif etype == EV_ABS and code == ABS_Y:
                            y, moved = value, True
                        elif etype == EV_KEY and code in (BTN_TOOL_PEN, BTN_TOOL_RUBBER):
                            self.in_range = bool(value)
                            self.rubber = code == BTN_TOOL_RUBBER and bool(value)
                            self.speed.reset()
                            if value and now - self.display_at > self.DISPLAY_EVERY_S:
                                self.send_display(now)
                        elif etype == EV_KEY and code == BTN_TOUCH:
                            self.tip = bool(value)
                            self.ink_update()
                        elif etype == EV_SYN and self.in_range:
                            if moved and x is not None and y is not None:
                                v = self.speed.add(now, x / rx, y / ry)
                                last_pos = now
                            else:
                                v = self.speed.value()
                            moved = False
                            self.send_feedback(now, quantize(v * self.args.speed_gain, self.args.full_scale))
            except OSError as e:
                connect.log("stylus %s: %s" % (path, e))
            finally:
                os.close(fd)
                self.in_range = self.tip = False
            time.sleep(1.0)


def main(argv=None):
    import argparse
    ap = argparse.ArgumentParser(description="Feed pen speed back to the Surface digitizer so the Slim Pen 2 plays its continuous haptic waveforms")
    ap.add_argument("--fast-host-id", type=lambda s: int(s, 0), help="10-bit host id (default: kept in --state, made up once)")
    ap.add_argument("--state", default="/var/lib/surface-pen/fast-host-id")
    ap.add_argument("--full-scale", type=float, default=300.0,
                    help="pen speed in mm/s that maps to the largest value (default 300: where the Windows host's value reaches 63, measured)")
    ap.add_argument("--speed-gain", type=float, default=1.0,
                    help="multiplies the pen speed before it is sent; the pen vibrates harder at higher speeds (default 1.0)")
    ap.add_argument("--ink-haptics", action="store_true", help="start the inking waveform on contact and stop it on release")
    ap.add_argument("--ink-intensity", type=int, default=haptic.INK_INTENSITY,
                    help="intensity of the inking waveform, 1..255 (default %d, what Windows sends)" % haptic.INK_INTENSITY)
    args = ap.parse_args(argv)
    Daemon(args).run()
    return 0
