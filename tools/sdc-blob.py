# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
"""Surface Display Calibration (SDC) data: decode the per-unit panel calibration the firmware keeps in two UEFI
variables, and derive Linux color settings from it.

    python -I -X utf8 tools/sdc-blob.py MEASUREMENT [CONFIG] [--icc out.icc] [--trc 11|12]

Inputs
  Windows: private/windows/display/uefi-Type_1-*.bin (measurement) and uefi-Type_2-*.bin (configuration),
           dumped by tools/windows/collect-display.ps1.
  Linux:   /sys/firmware/efi/efivars/Type_1-0c3b397f-4162-4ed2-9aaa-5baf400e3aaf (and Type_2-...);
           the 4-byte efivarfs attribute prefix is stripped automatically.

Output: blob/table headers, measured patches (XYZ in nits, xy), the PWM -> relative luminance pairs, the
two tone curves, the configuration fields whose values identify them, the native primaries and a 3x3 linear
matrix that maps linear sRGB (D65) to native panel RGB (usable as DRM CTM between DEGAMMA_LUT and GAMMA_LUT).
--icc writes a matrix/TRC ICC v2.4 display profile of the native panel (for colord / KDE / GNOME).

Variable layout (little endian), as read from the variables themselves; tables are known by number only:
  blob header 28 B: header size, total size, blob id (2 in Type_1, 3 in Type_2), three u32 not used here,
                    table count; then the tables; then 4 trailing bytes (not checked)
  table header 24 B: header size, table size, table id, a u32 not used here, entry size, entry count
  blob 2 tables 3 and 4, entries of 33 B: R,G,B and three more u16, a u8, PWM duty (u32, milli-%),
                    backlight current (u32, uA), CIE X,Y,Z (f32, nits). The patches are the primaries,
                    greys, black and white; table 4 repeats white at two PWM values
  blob 3 table 1:   one 401-byte entry; only the fields in CONFIG_FIELDS are read
  blob 3 table 2:   (u32 PWM duty in milli-%, f64 luminance relative to 100 %) pairs, agreeing with table 4
  blob 3 tables 11 and 12: 256 f64 each, rising from 0 to 1 (tone curves; --trc picks one for the ICC)
"""
import argparse
import math
import struct
import sys

# Fields of blob 3 table 1 whose values identify them: (offset within table data, format, description)
CONFIG_FIELDS = [
    (0x44, "<d", "PWM duty (milli-%), equal to that of the measured patches"),
    (0x4c, "<I", "backlight current (uA), equal to that of the measured patches"),
    (0xa5, "<4d", "piecewise curve (sRGB constants: linear limit, linear slope, offset, exponent)"),
    (0xe9, "<d", "power curve exponent"),
    (0x119, "<3d", "primary red (Y, x, y)"), (0x131, "<3d", "primary green (Y, x, y)"),
    (0x149, "<3d", "primary blue (Y, x, y)"), (0x161, "<3d", "white (Y, x, y)"),
]

SRGB = [[0.4124, 0.3576, 0.1805], [0.2126, 0.7152, 0.0722], [0.0193, 0.1192, 0.9505]]
D50 = [0.9642, 1.0, 0.8249]
BRADFORD = [[0.8951, 0.2664, -0.1614], [-0.7502, 1.7135, 0.0367], [0.0389, -0.0685, 1.0296]]


def load(path):
    d = open(path, "rb").read()
    if len(d) >= 8:
        hs, total = struct.unpack("<II", d[:8])
        if hs != 28 and len(d) > 12 and struct.unpack("<I", d[4:8])[0] == 28:
            d = d[4:]  # efivarfs attribute prefix
    return d


def tables(d):
    h = struct.unpack("<7I", d[:28])
    off, out = h[0], {}
    for _ in range(h[5]):
        th = struct.unpack("<6I", d[off:off + 24])
        out[th[2]] = (th, d[off + th[0]:off + th[1]])
        off += th[1]
    return h, out, d[off:off + 4]


def matmul(a, b):
    return [[sum(a[i][k] * b[k][j] for k in range(3)) for j in range(3)] for i in range(3)]


def matvec(a, v):
    return [sum(a[i][k] * v[k] for k in range(3)) for i in range(3)]


def inv(m):
    (a, b, c), (d, e, f), (g, h, i) = m
    det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g)
    return [[(e * i - f * h) / det, (c * h - b * i) / det, (b * f - c * e) / det],
            [(f * g - d * i) / det, (a * i - c * g) / det, (c * d - a * f) / det],
            [(d * h - e * g) / det, (b * g - a * h) / det, (a * e - b * d) / det]]


def xy(v):
    s = sum(v)
    return (v[0] / s, v[1] / s) if s else (0.0, 0.0)


def cct(x, y):  # McCamy
    n = (x - 0.3320) / (0.1858 - y)
    return 449 * n ** 3 + 3525 * n ** 2 + 6823.3 * n + 5520.33


def measurements(raw, count):
    out = []
    for k in range(count):
        e = raw[33 * k:33 * (k + 1)]
        rgb = struct.unpack("<6H", e[:12])
        pwm, cur = struct.unpack("<II", e[13:21])
        out.append((rgb, e[12], pwm, cur, struct.unpack("<3f", e[21:33])))
    return out


def s15(v):
    return struct.pack(">i", int(round(v * 65536)))


def write_icc(path, prim, white_xyz, trc, desc):
    # chromatic adaptation (Bradford) from the measured white to D50
    wn = [c / white_xyz[1] for c in white_xyz]
    cs, cd = matvec(BRADFORD, wn), matvec(BRADFORD, D50)
    chad = matmul(inv(BRADFORD), matmul([[cd[0] / cs[0], 0, 0], [0, cd[1] / cs[1], 0], [0, 0, cd[2] / cs[2]]], BRADFORD))
    cols = [matvec(chad, [p / white_xyz[1] for p in col]) for col in prim]

    def xyz_tag(v):
        return b"XYZ \0\0\0\0" + b"".join(s15(c) for c in v)

    def text_desc(s):
        a = s.encode("ascii") + b"\0"
        return b"desc\0\0\0\0" + struct.pack(">I", len(a)) + a + b"\0" * 12 + b"\0" * 67

    curve = b"curv\0\0\0\0" + struct.pack(">I", len(trc)) + b"".join(
        struct.pack(">H", max(0, min(65535, int(round(v * 65535))))) for v in trc)
    tags = [(b"desc", text_desc(desc)), (b"cprt", b"text\0\0\0\0No copyright, generated from SDC calibration\0"),
            (b"wtpt", xyz_tag(D50)), (b"rXYZ", xyz_tag(cols[0])), (b"gXYZ", xyz_tag(cols[1])),
            (b"bXYZ", xyz_tag(cols[2])), (b"rTRC", curve), (b"gTRC", curve), (b"bTRC", curve),
            (b"chad", b"sf32\0\0\0\0" + b"".join(s15(chad[r][c]) for r in range(3) for c in range(3))),
            (b"lumi", xyz_tag([0, white_xyz[1], 0]))]
    off = 128 + 4 + 12 * len(tags)
    table, body = struct.pack(">I", len(tags)), b""
    for sig, data in tags:
        while (off + len(body)) % 4:
            body += b"\0"
        table += sig + struct.pack(">II", off + len(body), len(data))
        body += data
    size = 128 + len(table) + len(body)
    hdr = struct.pack(">I4sI4s4s4s", size, b"none", 0x02400000, b"mntr", b"RGB ", b"XYZ ") + b"\0" * 12
    hdr += b"acspMSFT" + b"\0" * 4 + b"\0" * 4 + b"\0" * 4 + b"\0" * 8 + struct.pack(">I", 0)
    hdr += s15(D50[0]) + s15(D50[1]) + s15(D50[2]) + b"\0" * 4 + b"\0" * 16 + b"\0" * 28
    assert len(hdr) == 128
    open(path, "wb").write(hdr + table + body)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("measurement")
    ap.add_argument("config", nargs="?")
    ap.add_argument("--icc")
    ap.add_argument("--trc", choices=("11", "12"), default="11", help="blob 3 tone curve table for the ICC")
    a = ap.parse_args()

    blobs = [load(a.measurement)] + ([load(a.config)] if a.config else [])
    meas, conf = None, None
    for d in blobs:
        h, tbl, crc = tables(d)
        print(f"blob id {h[2]} size {h[1]} tables {h[5]} (header {h[3]} {h[4]} {h[6]}, trailer {crc.hex()})")
        for tid, (th, _) in tbl.items():
            print(f"  table {tid:2} ({th[3]}) entry {th[4]} B x {th[5]}")
        if h[2] == 2:
            meas = tbl
        elif h[2] == 3:
            conf = tbl

    if meas:
        print("\nmeasured patches: RGB / PWM milli-% / current uA / XYZ nits / xy")
        for tid in (3, 4):
            if tid not in meas:
                continue
            th, raw = meas[tid]
            for rgb, ratio, pwm, cur, X in measurements(raw, th[5]):
                x, y = xy(X)
                print(f"  t{tid} {rgb[:3]} {pwm:6} {cur:6}  X {X[0]:8.3f} Y {X[1]:8.3f} Z {X[2]:8.3f}  xy {x:.4f} {y:.4f}")
    if conf:
        print("\nblob 3 table 1 fields")
        th, raw = conf[1]
        for off, fmt, name in CONFIG_FIELDS:
            v = struct.unpack(fmt, raw[off:off + struct.calcsize(fmt)])
            print(f"  +{off:#05x} {name} = {', '.join(f'{x:.6g}' if isinstance(x, float) else str(x) for x in v)}")
        if 2 in conf:
            th, raw = conf[2]
            pts = [struct.unpack("<Id", raw[12 * i:12 * i + 12]) for i in range(th[5])]
            print("  table 2 (PWM milli-% -> relative luminance): " +
                  ", ".join(f"{p}:{r:.4f}" for p, r in pts))
        for tid in (11, 12):
            if tid in conf:
                th, raw = conf[tid]
                v = struct.unpack(f"<{th[5]}d", raw)
                g = math.log(v[128]) / math.log(128 / 255) if v[128] > 0 else float("nan")
                print(f"  table {tid} curve: v[16] {v[16]:.5f} v[64] {v[64]:.5f} v[128] {v[128]:.5f} "
                      f"v[192] {v[192]:.5f}, effective gamma at 50% {g:.3f}")

    if meas and 3 in meas:
        th, raw = meas[3]
        m = {p[0][:3]: p for p in measurements(raw, th[5]) if p[2] == 100000}
        black = m.get((0, 0, 0), (None,) * 4 + ([0, 0, 0],))[4]
        R, G, B, W = (m[k][4] for k in ((255, 0, 0), (0, 255, 0), (0, 0, 255), (255, 255, 255)))
        prim = [[c - b for c, b in zip(P, black)] for P in (R, G, B)]
        wx, wy = xy(W)
        print(f"\nnative panel at 100% PWM: white {W[1]:.1f} nits xy {wx:.4f} {wy:.4f} (~{cct(wx, wy):.0f} K), "
              f"black {black[1]:.3f} nits, contrast {W[1] / black[1]:.0f}:1" if black[1] else "")
        for n, P in zip("RGB", prim):
            print(f"  {n} xy {xy(P)[0]:.4f} {xy(P)[1]:.4f}")
        s = [sum(c) for c in zip(*prim)]
        print(f"  additivity: (R+G+B-3K)/(W-K) Y = {s[1] / (W[1] - black[1]):.4f}")
        wy = W[1] - black[1]
        nat = [[prim[c][r] / wy for c in range(3)] for r in range(3)]  # columns = primaries, white Y = 1
        ctm = matmul(inv(nat), SRGB)
        k = 1 / max(sum(row) for row in ctm)
        ctm = [[v * k for v in row] for row in ctm]
        print(f"  linear sRGB(D65) -> native RGB matrix (scaled so white peaks at 1.0, {k * wy:.0f} nits max at 100% PWM):")
        for row in ctm:
            print("    " + "  ".join(f"{v:9.6f}" for v in row))
        if a.icc:
            trc = None
            if conf and int(a.trc) in conf:
                th, raw = conf[int(a.trc)]
                trc = struct.unpack(f"<{th[5]}d", raw)
            if trc is None:
                trc = [(i / 255) ** 2.2 for i in range(256)]
            write_icc(a.icc, [list(p) for p in prim], [w - b for w, b in zip(W, black)], trc,
                      "Surface panel native (SDC calibration)")
            print(f"\nwrote {a.icc}")


if __name__ == "__main__":
    sys.exit(main())
