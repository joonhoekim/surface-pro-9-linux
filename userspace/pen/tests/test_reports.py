# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
import argparse
import asyncio
import os
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from surface_pen import col07, connect, feedback, haptic, hiddesc, hidraw  # noqa: E402

# Slim Pen 2 BLE report descriptor posted in linux-surface/linux-surface#1066.
PEN_DESC = bytes.fromhex(
    "05010906a1018501050719e029e715002501750195088102750895018101950615012503196d296f8100c006"
    "0bff0a0001a1018530150025ff9501750809009102c00a0101a101150025ff7508953c852a09c682020109c7"
    "920201953c09c8b20301170000008027ffffff7f75209504852c19cc29cf8102852d19d829db810219dc29df"
    "9102852e19e429e7810219e829eb910219ec29efb102852f19f029f3810219f429f7910219f829fbb102c009"
    "53a101150026ff007508950185320a01038202010a000382020195630a0203820201956585330a0104820201"
    "95650a0004920201953c85340a020482020195010a0304920201c0050d0920a1018540950175201700000080"
    "27ffffff7f095bb1037510150127ffff00000991b1030992b10309940998b103050e0901a102854195017508"
    "150126ff000924b102092491020923b10209239102150125160920b10209219102150026fe0066011055fd35"
    "0046ec0909289102751026d00746d007092591020925b1024500854275201742000d002742000d000922b102"
    "0911a102050a75109514150127ffff000019032916b102c0050e0910a102050a16031026ff2f19032916b102"
    "c0c0c0"
)


def item(tag, typ, value=None, size=None):
    if value is None:
        return bytes([(tag << 4) | (typ << 2)])
    if size is None:
        size = 1 if 0 <= value <= 0xFF else 2 if 0 <= value <= 0xFFFF else 4
    code = {1: 1, 2: 2, 4: 3}[size]
    return bytes([(tag << 4) | (typ << 2) | code]) + (value & ((1 << (8 * size)) - 1)).to_bytes(size, "little")


def page(p): return item(0x0, 1, p)
def use(u): return item(0x0, 2, u)
def rid(r): return item(0x8, 1, r)
def rsize(n): return item(0x7, 1, n)
def rcount(n): return item(0x9, 1, n)
def lmin(v): return item(0x1, 1, v)
def lmax(v): return item(0x2, 1, v)
def coll(k): return item(0xA, 0, k)
def end(): return item(0xC, 0)
def inp(f): return item(0x8, 0, f)
def out(f): return item(0x9, 0, f)
def feat(f): return item(0xB, 0, f)


def bytes_field(p, u, n):
    return page(p) + use(u) + rsize(8) + rcount(n) + lmin(0) + lmax(255) + item(0x2, 1, 255, 2)


def flag(u):
    return page(0xFFF4) + use(u) + rsize(1) + rcount(1) + lmin(0) + lmax(1)


def pad(n):
    return rsize(n) + rcount(1)


# Synthetic stand-in for the THC FFF4:0001 collection, built from the Windows capability dump
# (report IDs, usages and sizes). Field order inside each report is a guess.
COL07 = (
    page(0xFFF4) + use(0x01) + coll(1)
    + use(0x07) + coll(2) + rid(84)
    + bytes_field(0xFF0F, 0x50, 4) + inp(2) + bytes_field(0xFFF4, 0x02, 6) + inp(2)
    + flag(0x08) + inp(2) + pad(7) + inp(1) + end()
    + use(0x13) + coll(2) + rid(110)
    + bytes_field(0xFF0F, 0x50, 4) + inp(2) + bytes_field(0xFFF4, 0x02, 6) + inp(2)
    + flag(0x08) + inp(2) + flag(0x11) + inp(2) + flag(0x12) + inp(2) + pad(5) + inp(1) + end()
    + use(0x06) + coll(2) + rid(86)
    + bytes_field(0xFFF4, 0x03, 6) + feat(2) + flag(0x09) + feat(2) + pad(7) + feat(1) + end()
    + use(0x16) + coll(2) + rid(112)
    + flag(0x10) + feat(2) + flag(0x22) + feat(2) + pad(6) + feat(1) + end()
    + rid(115) + bytes_field(0xFFF4, 0x21, 2) + feat(2)
    + end()
)


class HapticReport(unittest.TestCase):
    def setUp(self):
        self.desc = hiddesc.parse(PEN_DESC)
        self.lay = haptic.Layout(self.desc)

    def test_descriptor_length(self):
        self.assertEqual(len(PEN_DESC), 443)

    def test_layout(self):
        self.assertEqual(self.lay.output_id, 65)
        self.assertEqual(self.lay.output_size, 7)
        self.assertEqual(self.lay.feature_id, 65)
        self.assertEqual(self.lay.list_id, 66)
        cut = self.lay.field(haptic.CUTOFF_TIME)
        self.assertEqual((cut.logical_max, cut.physical_max, cut.unit_exp), (254, 2540, -3))
        rt = self.lay.field(haptic.RETRIGGER_PERIOD)
        self.assertEqual((rt.size, rt.logical_max), (16, 2000))

    def test_pack_matches_issue_captures(self):
        # Windows start/stop commands captured in #1066, and the one known to buzz on Linux.
        self.assertEqual(self.lay.pack_trigger(9, 0x7F), bytes([65, 0, 0x7F, 0x09, 0, 0, 0]))
        self.assertEqual(self.lay.pack_trigger(2, 0xFE), bytes([65, 0, 0xFE, 0x02, 0, 0, 0]))
        self.assertEqual(self.lay.pack_trigger(4, 255, repeat=1)[:4], bytes([65, 1, 255, 4]))

    def test_pack_time_fields(self):
        r = self.lay.pack_trigger(3, 100, cutoff_ms=500, retrigger_ms=0x0102)
        self.assertEqual(r, bytes([65, 0, 100, 3, 50, 0x02, 0x01]))

    def test_range_check(self):
        with self.assertRaises(ValueError):
            self.lay.pack_trigger(23, 100)
        with self.assertRaises(ValueError):
            self.lay.pack_trigger(3, 256)

    def test_waveform_list(self):
        size = self.desc.report_size(hiddesc.FEATURE, 66)
        self.assertEqual(size, 1 + 4 + 40 + 40)
        body = (0x000D0042).to_bytes(4, "little")
        body += b"".join((10 * (i + 1)).to_bytes(2, "little") for i in range(20))
        body += b"".join((0x1003 + i).to_bytes(2, "little") for i in range(20))
        wf = self.lay.parse_waveform_list(bytes([66]) + body)
        self.assertEqual(wf[3], (0x1003, 10))
        self.assertEqual(wf[4], (0x1004, 20))
        self.assertEqual(haptic.ordinal_for(wf, "buzz-continuous", 0), 4)
        self.assertEqual(haptic.ordinal_for_kind(wf, "buzz"), 4)
        self.assertEqual(haptic.ordinal_for({}, "click", 3), 3)

    def test_waveform_list_from_windows_trace(self):
        # ATT Read Rsp of Feature 66 at reconnect (Windows ETW, docs/research/gaps/pen-ble-trace.md); HOGP drops the ID.
        body = bytes.fromhex(
            "42000d00" "2d003c003c0023009100b900cb07cb07cb07cb07bc07cb07bc07" + "0000" * 7
            + "031006100710081009100a100b100c100d100e100f1010101110" + "0110" * 7)
        self.assertEqual(len(body), 84)
        wf = self.lay.parse_waveform_list(bytes([66]) + body)
        self.assertEqual({o: v for o, v in wf.items() if v[0] != 0x1001}, haptic.KNOWN_WAVEFORMS)
        self.assertEqual(wf[16], (0x1001, 0))
        self.assertEqual(haptic.ordinal_for_kind(wf, "pulse"), 3)
        self.assertEqual(haptic.ordinal_for_kind(wf, "buzz"), 9)   # no buzz-continuous; 4 would be "press"
        self.assertEqual(haptic.ordinal_for_kind(wf, "eraser"), 14)
        self.assertEqual(haptic.ordinal_for_kind({}, "ink"), 9)

    def test_ink_feedback_edges(self):
        lay = self.lay

        class Fake:
            def __init__(self):
                self.sent = []

            def ordinal(self, kind):
                return haptic.ordinal_for_kind({}, kind)

            def trigger(self, o, i, repeat=0, cutoff_ms=0, retrigger_ms=0):
                self.sent.append(lay.pack_trigger(o, i, repeat, cutoff_ms, retrigger_ms))

            def stop(self):
                self.trigger(haptic.ORDINAL_STOP, haptic.STOP_INTENSITY)

        f = Fake()
        ink = haptic.InkFeedback(f)
        for c in (False, True, True, True, False, False, True):
            ink.update(c)
        start, stop = bytes([65, 0, 0x7F, 9, 0, 0, 0]), bytes([65, 0, 0xFE, 2, 0, 0, 0])
        self.assertEqual(f.sent, [start, stop, start])

    def test_feature65_unpack(self):
        rid, vals = hiddesc.unpack(self.desc, hiddesc.FEATURE, bytes([65, 1, 255, 1, 0, 0]))
        self.assertEqual(rid, 65)
        self.assertEqual(vals[haptic.INTENSITY], 255)
        self.assertEqual(vals[haptic.AUTO_TRIGGER], 1)

    def test_pressure_mapping(self):
        self.assertEqual(haptic.pressure_to_intensity(0), 0)
        self.assertEqual(haptic.pressure_to_intensity(4096), 255)
        self.assertTrue(1 <= haptic.pressure_to_intensity(1000) < 255)


class Col07Report(unittest.TestCase):
    def setUp(self):
        self.desc = hiddesc.parse(COL07)

    def test_sizes_match_windows_caps(self):
        self.assertTrue(self.desc.has_application(col07.TLC))
        self.assertEqual(self.desc.report_size(hiddesc.INPUT, 84), 12)
        self.assertEqual(self.desc.report_size(hiddesc.INPUT, 110), 12)
        self.assertEqual(self.desc.report_size(hiddesc.FEATURE, 86), 8)
        self.assertEqual(self.desc.report_size(hiddesc.FEATURE, 112), 2)
        self.assertEqual(self.desc.report_size(hiddesc.FEATURE, 115), 3)

    def test_parse_input(self):
        addr = bytes([0x66, 0x55, 0x44, 0x33, 0x22, 0x11])
        data = bytes([110]) + b"\x01\x02\x03\x04" + addr + bytes([0b101])
        s = col07.parse_input(self.desc, data)
        self.assertEqual(s.report_id, 110)
        self.assertEqual(s.pen_id, b"\x01\x02\x03\x04")
        self.assertEqual(s.addr(True), "11:22:33:44:55:66")
        self.assertEqual(s.addr(False), "66:55:44:33:22:11")
        self.assertEqual(s.flags, {col07.FLAG_0008: 1, col07.FLAG_0011: 0, col07.FLAG_0012: 1})
        self.assertNotIn("11:22", s.describe())
        self.assertIsNone(col07.parse_input(self.desc, bytes([7, 0, 0])))

    def test_host_features(self):
        p = col07.host_addr_feature(self.desc, "11:22:33:44:55:66", lsb_first=True, flag=1)
        self.assertEqual(p, bytes([86, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11, 0x01]))
        p = col07.host_addr_feature(self.desc, "11:22:33:44:55:66", lsb_first=False)
        self.assertEqual(p[1:7], bytes.fromhex("112233445566"))
        self.assertEqual(col07.ab_capability_feature(self.desc, [col07.FLAG_0010]), bytes([112, 0x01]))
        self.assertEqual(col07.ab_capability_feature(self.desc, [col07.FLAG_0022], base=bytes([112, 0x01])),
                         bytes([112, 0x03]))


class Misc(unittest.TestCase):
    def test_coupled_address(self):
        # H XOR the host address, static random; NOT H without a host. A bond gives H back. (H is made up.)
        def fmt(q):
            return ":".join("%02X" % b for b in q.to_bytes(6, "big"))
        h, host, other_host = 0x2A5A5A5A5A5A, "11:22:33:44:55:66", fmt(0xA0B0C0D0E0F0)
        self.assertEqual(col07.coupled_address(h, host), fmt(0xFB78691E0F3C))
        self.assertEqual(col07.coupled_address(h), fmt(0xD5A5A5A5A5A5))
        self.assertEqual(col07.pen_hash_from_bond(fmt(0xFB78691E0F3C), host), h)
        other = col07.coupled_address(h, other_host)
        self.assertNotEqual(other, fmt(0xFB78691E0F3C))
        self.assertEqual(col07.pen_hash_from_bond(other, other_host), h)
        self.assertIsNone(col07.pen_hash_from_bond(fmt(0x3B78691E0F3C), host))

    def test_digitizer_feedback(self):
        # The frames of pen-vendor-exchange.md 1.8, as sp9-helperd sends them (FastHostId made up).
        head = lambda seq: "098ea5%s0201009001" % seq + "00" * 28
        self.assertEqual(feedback.pen_feedback(0, 0x2A5).hex(),
                         head("00") + "ff4c00a5020103" + "02ff005203" + "00" * 15)
        self.assertEqual(feedback.pen_feedback(1, 0x2A5, 0, contact=True).hex(),
                         head("01") + "ff4e00a5020101" + "0400005207" + "00" * 15)
        self.assertEqual([feedback.pen_feedback(s, 0x2A5)[44] for s in (0, 1, 2, 255)], [2, 4, 2, 4])
        m = feedback.pen_feedback(2, 0x2A5, 40, contact=True)
        self.assertEqual((m[38], m[43], m[45], m[47:49]), (0x4E, 1, 40, bytes([0x52, 0x07])))
        self.assertEqual(feedback.pen_feedback(2, 0x2A5, 200, contact=True)[45], 63)
        # no speed in hover or for the eraser end
        m = feedback.pen_feedback(2, 0x2A5, 40)
        self.assertEqual((m[38], m[43], m[45], m[47:49]), (0x4C, 3, 0xFF, bytes([0x52, 0x03])))
        m = feedback.pen_feedback(2, 0x2A5, 40, contact=True, eraser=True)
        self.assertEqual((m[38], m[43], m[45], m[47:49]), (0x4E, 3, 0xFF, bytes([0x52, 0x03])))
        self.assertEqual(feedback.display_states(0x2A5).hex(),
                         "098ea10100900100" + "00" * 33 + "a502" + "00" * 21)

    def test_pen_speed(self):
        e = feedback.SpeedEstimator()
        v = 0
        for i in range(20):
            v = e.add(i * 0.0077, i * 0.77, 0)      # 100 mm/s
        self.assertAlmostEqual(v, 100.0, delta=0.5)
        self.assertEqual(feedback.quantize(v, 860), 7)
        self.assertEqual(feedback.quantize(5000, 860), 63)
        self.assertEqual(feedback.quantize(0, 860), 0)
        e.reset()
        self.assertEqual(e.value(), 0.0)

    def test_mask(self):
        self.assertEqual(hidraw.mask_addr("11:22:33:44:55:66"), "XX:XX:XX:XX:XX:66")
        self.assertEqual(hidraw.mask_addr("11:22:33:44:55:66", True), "11:22:33:44:55:66")

    def test_ioctl_numbers(self):
        self.assertEqual(hidraw.HIDIOCGFEATURE(8), 0xC0084807)
        self.assertEqual(hidraw.HIDIOCSFEATURE(8), 0xC0084806)

    def test_signed_logical(self):
        d = hiddesc.parse(page(0x0D) + use(0x20) + coll(1) + rid(1) + page(0x0D) + use(0x3D)
                          + item(0x1, 1, -9000, 2) + item(0x2, 1, 9000, 2) + rsize(16) + rcount(1) + inp(2) + end())
        f = d.find(hiddesc.INPUT, hiddesc.usage(0x0D, 0x3D))
        self.assertEqual((f.logical_min, f.logical_max), (-9000, 9000))
        _, v = hiddesc.unpack(d, hiddesc.INPUT, bytes([1]) + (-100 & 0xFFFF).to_bytes(2, "little"))
        self.assertEqual(v[hiddesc.usage(0x0D, 0x3D)], -100)


class EvdevRetry(unittest.TestCase):
    """The stylus evdev may appear after the helper starts and may be replaced later."""

    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.addCleanup(self.dir.cleanup)
        # A FIFO stands in for the event node: readable without blocking, and it reads as end of
        # file once the writer is gone, which the watcher treats like a removed device.
        self.node = os.path.join(self.dir.name, "event22")
        os.mkfifo(self.node)
        self.loop = asyncio.new_event_loop()
        self.addCleanup(self.loop.close)
        self.helper = connect.Helper(argparse.Namespace(addr_order="lsb", evdev=None))
        self.seen = []

        async def on_seen(source, sighting=None):
            self.seen.append(source)
        self.helper.on_seen = on_seen
        self.addCleanup(lambda: self.helper.evdev_fd is None or os.close(self.helper.evdev_fd))

    def spin(self):
        self.loop.run_until_complete(asyncio.sleep(0.05))

    def test_found_later_and_again_after_removal(self):
        pen_down = connect.EVENT.pack(0, 0, connect.EV_KEY, connect.BTN_TOOL_PEN, 1)
        with mock.patch.object(connect, "log"):
            with mock.patch.object(connect, "find_stylus_evdev", return_value=None):
                self.assertFalse(self.helper.ensure_evdev(self.loop))
                self.assertFalse(self.helper.ensure_evdev(self.loop))
            with mock.patch.object(connect, "find_stylus_evdev", return_value=self.node):
                self.assertTrue(self.helper.ensure_evdev(self.loop))
                w = os.open(self.node, os.O_WRONLY)
                os.write(w, pen_down)
                self.spin()
                self.assertEqual(self.seen, ["evdev"])
                # iptsd goes away
                os.close(w)
                self.spin()
                self.assertIsNone(self.helper.evdev_fd)
                # and comes back
                self.assertTrue(self.helper.ensure_evdev(self.loop))
                w = os.open(self.node, os.O_WRONLY)
                os.write(w, pen_down)
                self.spin()
                os.close(w)
                self.assertEqual(self.seen, ["evdev", "evdev"])

    def test_missing_node_is_logged_once(self):
        with mock.patch.object(connect, "log") as log, \
                mock.patch.object(connect, "find_stylus_evdev", return_value=None):
            for _ in range(3):
                self.helper.ensure_evdev(self.loop)
        self.assertEqual(log.call_count, 1)

    def test_unopenable_node_is_retried(self):
        gone = os.path.join(self.dir.name, "event99")
        with mock.patch.object(connect, "log"), \
                mock.patch.object(connect, "find_stylus_evdev", return_value=gone):
            self.assertFalse(self.helper.ensure_evdev(self.loop))
            os.mkfifo(gone)
            self.assertTrue(self.helper.ensure_evdev(self.loop))


if __name__ == "__main__":
    unittest.main()
