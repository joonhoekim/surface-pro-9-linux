#!/usr/bin/env python3
"""Statistics of raw Bayer frames written by cam-raw.sh (16-bit LE, 10 bits).

usage: cam-raw-stats.py WIDTH HEIGHT FILE... [--order GRBG] [--black N] [--columns] [--png OUT.png]

Per file: mean of the four Bayer sites, share of clipped samples, and two sharpness figures of the green
plane in the centre half of the frame (Tenengrad and Laplacian variance, both divided by mean² so that
brightness cancels). --columns lists columns and rows whose mean departs from their neighbours (a defect
in the raw data shows here, one made later by the ISP does not). --png writes a half-size preview.
"""
import argparse
import numpy as np

ap = argparse.ArgumentParser()
ap.add_argument("width", type=int)
ap.add_argument("height", type=int)
ap.add_argument("files", nargs="+")
ap.add_argument("--order", default="GRBG")
ap.add_argument("--black", type=float, default=0.0)
ap.add_argument("--columns", action="store_true")
ap.add_argument("--png")
a = ap.parse_args()


def outliers(p, k=8):
    pad = np.pad(p, k, mode="edge")
    med = np.array([np.median(np.r_[pad[i:i + k - 1], pad[i + k + 2:i + 2 * k + 1]]) for i in range(p.size)])
    d = p - med
    s = np.median(np.abs(d)) * 1.4826 + 1e-6
    idx = [int(i) for i in np.argsort(-np.abs(d))[:6] if abs(d[i]) > 6 * s]
    return s, [(i, round(float(d[i]), 2)) for i in sorted(idx)]


for f in a.files:
    d = np.fromfile(f, dtype="<u2")
    d = d[-a.width * a.height:].reshape(a.height, a.width).astype(np.float32)  # the last frame in the file
    site = {}
    for i, c in enumerate(a.order):
        name = c if c != "G" else ("G1" if "G1" not in site else "G2")
        site[name] = d[i // 2::2, i % 2::2]
    clip = float((d >= 1020).mean())
    g = (site["G1"] + site["G2"]) / 2 - a.black
    h, w = g.shape
    c = g[h // 4:3 * h // 4, w // 4:3 * w // 4]
    m = max(float(c.mean()), 1e-3)
    gx = c[1:-1, 2:] - c[1:-1, :-2]
    gy = c[2:, 1:-1] - c[:-2, 1:-1]
    lap = c[1:-1, 2:] + c[1:-1, :-2] + c[2:, 1:-1] + c[:-2, 1:-1] - 4 * c[1:-1, 1:-1]
    print(f"{f}: " + " ".join(f"{k}={v.mean() - a.black:.1f}" for k, v in site.items())
          + f" clip={clip:.4f} tenengrad={float((gx**2 + gy**2).mean()) / m**2:.5f} lapvar={float(lap.var()) / m**2:.5f}")
    if a.columns:
        for k, v in site.items():
            s, o = outliers(v.mean(axis=0))
            s2, o2 = outliers(v.mean(axis=1))
            print(f"  {k}: column sigma {s:.2f} outliers {o}; row sigma {s2:.2f} outliers {o2}")
    if a.png:
        from PIL import Image
        r, b = site["R"] - a.black, site["B"] - a.black
        rgb = np.stack([r * g.mean() / max(r.mean(), 1), g, b * g.mean() / max(b.mean(), 1)], axis=-1)
        rgb = np.clip(rgb / max(np.percentile(rgb, 99.5), 1), 0, 1) ** (1 / 2.2)
        Image.fromarray((rgb * 255).astype(np.uint8)).save(a.png)
