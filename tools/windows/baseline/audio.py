# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
"""Speaker/microphone baseline: play test signals on the internal speakers while recording the internal 2-channel mic.

Usage (python -I audio.py ...):
  list
  sweep  --out DIR [--mode shared|exclusive] [--level -18]     log sweep on L, R: speaker response, mic delay/polarity
  tones  --out DIR [--mode ...] [--freqs 40,100,1000,3700] [--level -18] [--tag vol50]
  record --out DIR [--mode ...] [--seconds 5]                  mic only (person speaks in front of the device)
--mode is the playback path. "shared" goes through the Windows audio engine (Surface render APO active, = what users hear).
"exclusive" plays in WASAPI exclusive mode, which bypasses the render APO (raw codec path, the closest match to Linux without
a filter chain). Capture is always shared mode (Surface capture APO active), see play_rec. Signal level is fixed and moderate;
endpoint volume is managed by baseline.ps1 (set, then restored). Writes WAVs and <name>.json, prints a short summary.
"""
import argparse
import json
import os
import sys
import time
import wave

import numpy as np
import sounddevice as sd

FS = 48000


def pick(kind):
    apis = sd.query_hostapis()
    wasapi = next(i for i, h in enumerate(apis) if "WASAPI" in h["name"])
    best = None
    for i, d in enumerate(sd.query_devices()):
        if d["hostapi"] != wasapi:
            continue
        n = d["max_output_channels"] if kind == "out" else d["max_input_channels"]
        if n < 2 or "Surface" not in d["name"]:
            continue
        if kind == "out" and "Omnisonic" in d["name"]:
            return i, d["name"]
        best = best or (i, d["name"])
    if best is None:
        raise SystemExit(f"no Surface WASAPI {kind} device")
    return best


def write_wav(path, x):
    y = np.clip(x, -1, 1)
    with wave.open(path, "wb") as w:
        w.setnchannels(y.shape[1] if y.ndim > 1 else 1)
        w.setsampwidth(2)
        w.setframerate(FS)
        w.writeframes((y * 32767).astype("<i2").tobytes())


def play_rec(sig, mode, pre=0.3, post=0.6):
    """Play sig (n x 2 float) on the speaker while recording 2 mic channels. Returns n' x 2 float."""
    out_dev, _ = pick("out")
    in_dev, _ = pick("in")
    pex = mode == "exclusive"
    chunks = []

    def cb(indata, frames, t, status):
        chunks.append(indata.copy())

    # Capture is always shared mode: WASAPI exclusive capture through PortAudio re-delivers stale buffers on this device
    # (more samples than wall time, repeated blocks), which destroys the sweep deconvolution.
    with sd.InputStream(device=in_dev, channels=2, samplerate=FS, dtype="float32", callback=cb):
        time.sleep(pre)
        if sig is not None:
            # 0.5 s of leading silence lets the render path and amplifier wake up before the signal starts
            sig = np.concatenate([np.zeros((int(0.5 * FS), 2)), sig])
            s = (sig * 32767).astype(np.int16) if pex else sig.astype(np.float32)
            sd.play(s, FS, device=out_dev, extra_settings=sd.WasapiSettings(exclusive=True) if pex else None, blocking=True)
        time.sleep(post)
    return np.concatenate(chunks).astype(np.float64)


def db(x):
    return float(20 * np.log10(max(x, 1e-12)))


def rms(x):
    return float(np.sqrt(np.mean(np.square(x)))) if len(x) else 0.0


def gcc_phat(a, b, maxlag=96):
    """Lag of b relative to a in samples (positive: b later) and the sign of the correlation peak (polarity)."""
    n = 1 << int(np.ceil(np.log2(len(a) + len(b))))
    A, B = np.fft.rfft(a, n), np.fft.rfft(b, n)
    R = B * np.conj(A)
    phat = np.fft.irfft(R / (np.abs(R) + 1e-12), n)
    raw = np.fft.irfft(R, n)
    idx = np.r_[n - maxlag:n, 0:maxlag + 1]
    lags = np.r_[-maxlag:0, 0:maxlag + 1]
    k = int(np.argmax(np.abs(phat[idx])))
    # polarity from the plain cross-correlation at that lag
    return int(lags[k]), int(np.sign(raw[idx][k]) or 1), float(np.abs(phat[idx][k]))


def mic_pair_stats(x):
    l, r = x[:, 0], x[:, 1]
    lag, sign, conf = gcc_phat(l, r)
    s, d = rms(l + r), rms(l - r)
    return {
        "rms_dbfs": [db(rms(l)), db(rms(r))],
        "peak_dbfs": [db(np.max(np.abs(l))), db(np.max(np.abs(r)))],
        "clipped_samples": int(np.sum(np.abs(x) >= 0.999)),
        "ch_level_diff_db": db(rms(r)) - db(rms(l)),
        "lag_samples": lag, "lag_us": lag / FS * 1e6, "polarity": sign, "gcc_peak": conf,
        "sum_minus_diff_db": db(s) - db(d),
    }


def log_sweep(f1=50.0, f2=20000.0, T=4.0, level_db=-18.0):
    t = np.arange(int(T * FS)) / FS
    L = np.log(f2 / f1)
    s = np.sin(2 * np.pi * f1 * T / L * (np.exp(t * L / T) - 1))
    fade = int(0.02 * FS)
    s[:fade] *= np.linspace(0, 1, fade)
    s[-fade:] *= np.linspace(1, 0, fade)
    inv = s[::-1] * np.exp(-t * L / T)
    return s * 10 ** (level_db / 20), inv


def third_octave(ir):
    n = 1 << 16
    H = np.abs(np.fft.rfft(ir, n))
    f = np.fft.rfftfreq(n, 1 / FS)
    centers = [63, 100, 160, 250, 400, 630, 1000, 1600, 2500, 4000, 6300, 10000, 16000]
    out = {}
    for c in centers:
        m = (f >= c / 2 ** (1 / 6)) & (f < c * 2 ** (1 / 6))
        out[str(c)] = db(np.sqrt(np.mean(H[m] ** 2)))
    ref = out["1000"]
    return {k: round(v - ref, 1) for k, v in out.items()}


def cmd_sweep(a):
    s, inv = log_sweep(level_db=a.level)
    res = {"mode": a.mode, "level_dbfs": a.level, "speaker": {}}
    for name, chans in (("L", (1, 0)), ("R", (0, 1))):
        sig = np.stack([s * chans[0], s * chans[1]], 1)
        x = play_rec(sig, a.mode, post=2.5)
        write_wav(os.path.join(a.out, f"sweep-{a.mode}-{name}.wav"), x)
        n = 1 << int(np.ceil(np.log2(len(x) + len(inv))))
        irs = [np.fft.irfft(np.fft.rfft(x[:, ch], n) * np.fft.rfft(inv, n), n) for ch in range(2)]
        # The linear IR sits len(s)-1 samples into the deconvolution (harmonic IRs come earlier). Search 0..2 s (pre-roll + lead-in + output latency)
        # on the summed envelope, then each mic's own peak within +-1 ms of it.
        base = len(s) - 1
        env = np.abs(irs[0]) + np.abs(irs[1])
        k0 = base + int(np.argmax(env[base:base + int(2.0 * FS)]))
        peaks = []
        for ch in range(2):
            lo = k0 - 48
            peaks.append(lo + int(np.argmax(np.abs(irs[ch][lo:k0 + 49]))))
        noise = np.concatenate([irs[0][base + int(2.1 * FS):base + int(2.4 * FS)], irs[1][base + int(2.1 * FS):base + int(2.4 * FS)]])
        snr = db(np.max(env[k0 - 48:k0 + 49]) / 2) - db(rms(noise) if len(noise) else 1e-12)
        w = slice(min(peaks) - 48, min(peaks) + int(0.02 * FS))
        r = {
            "latency_ms": (k0 - base) / FS * 1e3,
            "ir_snr_db": snr,
            "reliable": bool(snr > 25),
            "mic_level_db": [db(rms(irs[c][w])) for c in range(2)],
            "mic_delay_us": (peaks[1] - peaks[0]) / FS * 1e6,
            "ir_polarity": [int(np.sign(irs[c][peaks[c]])) for c in range(2)],
            "response_db_rel_1k": [third_octave(irs[c][w]) for c in range(2)],
            "pair": mic_pair_stats(x),
        }
        res["speaker"][name] = r
    lv = {k: float(np.mean(v["mic_level_db"])) for k, v in res["speaker"].items()}
    res["speaker_balance_db_R_minus_L"] = lv["R"] - lv["L"]
    return res


def cmd_tones(a):
    res = {"mode": a.mode, "level_dbfs": a.level, "tag": a.tag, "tones": []}
    for f in [float(v) for v in a.freqs.split(",")]:
        for name, chans in (("L", (1, 0)), ("R", (0, 1)), ("LR", (1, 1))):
            t = np.arange(int(a.seconds * FS)) / FS
            s = np.sin(2 * np.pi * f * t) * 10 ** (a.level / 20)
            fade = int(0.01 * FS)
            s[:fade] *= np.linspace(0, 1, fade)
            s[-fade:] *= np.linspace(1, 0, fade)
            x = play_rec(np.stack([s * chans[0], s * chans[1]], 1), a.mode, post=1.0)
            write_wav(os.path.join(a.out, f"tone-{a.mode}-{a.tag}-{int(f)}-{name}.wav"), x)
            # Stream start-up latency varies per call (0.3-1 s), so find the tone by cross-correlation instead of a fixed window.
            mono = x.mean(1)
            n = 1 << int(np.ceil(np.log2(len(mono) + len(s))))
            c = np.fft.irfft(np.fft.rfft(mono, n) * np.conj(np.fft.rfft(s, n)), n)[:len(mono) - len(s)]
            lag = int(np.argmax(np.abs(c))) if len(c) else 0
            seg = mono[lag + int(0.1 * FS):lag + len(s) - int(0.1 * FS)]
            noise = mono[:int(0.25 * FS)]
            win = np.hanning(len(seg))
            X = np.abs(np.fft.rfft(seg * win))
            fr = np.fft.rfftfreq(len(seg), 1 / FS)

            def band(fc):
                m = (fr > fc * 0.97) & (fr < fc * 1.03)
                return float(np.sqrt(np.sum(X[m] ** 2))) if m.any() else 0.0

            fund = band(f)
            harm = [band(f * k) for k in range(2, 6) if f * k < FS / 2 - 500]
            res["tones"].append({
                "freq": f, "speaker": name,
                "mic_level_dbfs": db(rms(seg)),
                "snr_db": db(rms(seg)) - db(rms(noise)),
                "fund_db": db(fund / len(seg)),
                "thd_pct": float(100 * np.sqrt(np.sum(np.square(harm))) / fund) if fund > 0 else None,
                "clipped_samples": int(np.sum(np.abs(x) >= 0.999)),
            })
    return res


def cmd_record(a):
    x = play_rec(None, a.mode, pre=a.seconds, post=0.0)
    write_wav(os.path.join(a.out, f"record-{a.mode}{'-' + a.tag if a.tag else ''}.wav"), x)
    return {"mode": a.mode, "seconds": a.seconds, "tag": a.tag, "pair": mic_pair_stats(x)}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("cmd", choices=["list", "sweep", "tones", "record"])
    ap.add_argument("--out", default=".")
    ap.add_argument("--mode", default="shared", choices=["shared", "exclusive"])
    ap.add_argument("--level", type=float, default=-18.0)
    ap.add_argument("--freqs", default="40,100,1000,3700")
    ap.add_argument("--seconds", type=float, default=1.5)
    ap.add_argument("--tag", default="")
    a = ap.parse_args()
    if a.cmd == "list":
        print("out:", pick("out")[1])
        print("in :", pick("in")[1])
        return 0
    os.makedirs(a.out, exist_ok=True)
    try:
        res = {"sweep": cmd_sweep, "tones": cmd_tones, "record": cmd_record}[a.cmd](a)
    except sd.PortAudioError as e:
        res = {"mode": a.mode, "error": str(e)}
    name = f"{a.cmd}-{a.mode}" + (f"-{a.tag}" if a.tag else "")
    with open(os.path.join(a.out, name + ".json"), "w", encoding="utf-8") as f:
        json.dump(res, f, indent=1, ensure_ascii=False)
    print(json.dumps(res, ensure_ascii=False)[:400])
    return 0


if __name__ == "__main__":
    sys.exit(main())
