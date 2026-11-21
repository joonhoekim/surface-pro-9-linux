# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
"""Summarize a tools/windows/perf-modes.ps1 run.

usage: python summarize.py <raw run dir> <out dir (hw/windows/perf)>

Writes <out>/<ac|battery>-<mode>.md per mode and (re)writes <out>/README.md with a comparison table built from every
<power>-<mode>.md summary in <out> (each carries a machine-readable line "<!-- row: ... -->"). No device-unique data.
"""
import csv
import json
import os
import re
import sys
from statistics import mean

MODE_NAMES = {
    'efficiency': ('Best Power Efficiency', '961cc777-2547-4f9d-8174-7d86181b8a7a'),
    'balanced': ('Balanced (no overlay)', '00000000-0000-0000-0000-000000000000'),
    'better': ('Better Performance (Surface AC default)', '3af9b8d9-7c97-431d-ad78-34a8bfea439f'),
    'best': ('Best Performance', 'ded574b5-45a0-4f42-8737-46345c09c238'),
}
ORDER = list(MODE_NAMES)
P_CPUS = range(0, 4)    # i5-1235U: 2 P-cores (4 threads), 8 E-cores
E_CPUS = range(4, 12)


def fnum(s):
    try:
        return float(s)
    except (TypeError, ValueError):
        return None


def read_csv(path):
    if not os.path.exists(path):
        return []
    with open(path, encoding='utf-8-sig', newline='') as f:
        return list(csv.DictReader(f))


def avg(xs):
    xs = [x for x in xs if x is not None]
    return mean(xs) if xs else None


def mx(xs):
    xs = [x for x in xs if x is not None]
    return max(xs) if xs else None


def mn(xs):
    xs = [x for x in xs if x is not None]
    return min(xs) if xs else None


def f(x, nd=1):
    return '-' if x is None else f'{x:.{nd}f}'


FILETIME_EPOCH = 11644473600
KEY_NODES = ['I_SLIDER', 'I_STANDBY', 'I_MS_NOTIFY', 'O_BATT_MODE', 'O_CPU_AVG', 'O_CPU_MAX', 'O_CPU_PEAK', 'O_CPU_AVG_TIME', 'O_SYS_AVG',
             'O_SYS_MAX', 'O_SYS_PEAK', 'O_CPU_FMIN', 'O_CPU_FPEAK', 'O_WTMF_PARK', 'O_FAN1_PROFILE', 'O_FAN1_ACTIVE_RPM',
             'O_FAN_CS_REQUEST', 'O_FAN1_SPL_MAX', 'I_CPU_TEMP', 'I_CPU_TEMP_AVG', 'I_CPU_PWR', 'I_SYS_PWR', 'I_PROCHOT_STATUS',
             'I_PSU_SIZE', 'I_IPL_TOP', 'I_RTS_SAM_5', 'I_VTS_SAM_9', 'VN1', 'VN2', 'VN21', 'VN98']


def load_nodes(raw):
    """SurfacePowerTrackerCore ETW (tracerpt CSV): one event per SMF node and update, fields id, category, name, value."""
    path = os.path.join(raw, 'etw.csv')
    nodes = []
    if not os.path.exists(path):
        return nodes
    with open(path, encoding='utf-8-sig', errors='replace', newline='') as fi:
        for r in csv.reader(fi, skipinitialspace=True):
            if len(r) < 23 or not r[0].strip().endswith('SurfacePowerTrackerCore'):
                continue
            try:
                t = int(r[16]) / 1e7 - FILETIME_EPOCH
                v = int(r[22])
                if v >= 1 << 63:
                    v -= 1 << 64
                nodes.append((t, int(r[19]), r[20].strip(), r[21].strip(), v))
            except ValueError:
                pass
    return nodes


def main(raw, out):
    meta = dict(l.strip().split(' ', 1) for l in open(os.path.join(raw, 'meta.txt'), encoding='utf-8-sig') if ' ' in l.strip())
    power = meta['power']
    markers = read_csv(os.path.join(raw, 'markers.csv'))
    msr = read_csv(os.path.join(raw, 'msr.csv'))
    cnt = read_csv(os.path.join(raw, 'counters.csv'))
    smf = read_csv(os.path.join(raw, 'smf.csv'))
    for r in smf:
        r['t'] = float(r['t']) / 1000.0
    nodes = load_nodes(raw)
    if nodes:
        with open(os.path.join(raw, 'nodes.csv'), 'w', encoding='utf-8', newline='') as fo:
            w = csv.writer(fo)
            w.writerow(['t', 'id', 'category', 'name', 'value'])
            w.writerows(nodes)
    for rows in (msr, cnt):
        for r in rows:
            r['t'] = float(r['t'])

    # phases: (mode, phase, t0, t1)
    phases = []
    for i, m in enumerate(markers):
        if m['mode'] in MODE_NAMES and i + 1 < len(markers):
            phases.append((m['mode'], m['phase'], float(m['t']), float(markers[i + 1]['t'])))

    tz_cols = [c for c in (cnt[0].keys() if cnt else []) if 'high precision temperature' in c.lower()]
    pl_cols = [c for c in (cnt[0].keys() if cnt else []) if 'passive limit' in c.lower()]
    perf_total = next((c for c in (cnt[0].keys() if cnt else []) if c.lower() == 'processor information(_total)\\% processor performance'), None)
    freq = next((c for c in (cnt[0].keys() if cnt else []) if c.lower().endswith('\\processor frequency')), None)
    util = next((c for c in (cnt[0].keys() if cnt else []) if c.lower().endswith('% processor utility')), None)

    def sl(rows, t0, t1):
        return [r for r in rows if t0 <= r['t'] < t1]

    def smf_state(t):
        """Last value of every channel at time t."""
        st = {}
        for r in smf:
            if r['t'] <= t:
                st[int(r['channel'])] = int(r['value'])
        return st

    def smf_in(t0, t1):
        ch = {}
        for r in smf:
            if t0 <= r['t'] < t1:
                ch.setdefault(int(r['channel']), []).append(int(r['value']))
        return ch

    def node_at(name, t):
        v = None
        for n in nodes:
            if n[3] == name and n[0] <= t:
                v = n[4]
        return v

    def node_in(name, t0, t1):
        return [n[4] for n in nodes if n[3] == name and t0 <= n[0] < t1]

    def tz_c(r, c):
        v = fnum(r.get(c))
        return None if not v else v / 10.0 - 273.15   # tenths of K

    def phase_stats(rows_m, rows_c):
        s = {}
        s['pkg_w'] = avg([fnum(r['pkg_w']) for r in rows_m])
        s['pkg_w_max'] = mx([fnum(r['pkg_w']) for r in rows_m])
        s['pp0_w'] = avg([fnum(r['pp0_w']) for r in rows_m])
        s['psys_w'] = avg([fnum(r.get('psys_w')) for r in rows_m])
        s['pkg_temp_max'] = mx([fnum(r['pkg_temp_c']) for r in rows_m])
        s['pkg_temp'] = avg([fnum(r['pkg_temp_c']) for r in rows_m])
        s['ratio_p'] = avg([fnum(r.get(f'ratio{c}')) for r in rows_m for c in P_CPUS])
        s['ratio_e'] = avg([fnum(r.get(f'ratio{c}')) for r in rows_m for c in E_CPUS])
        s['pl1'] = sorted({r['pl1_w'] for r in rows_m})
        s['pl2'] = sorted({r['pl2_w'] for r in rows_m})
        s['tau'] = sorted({r['pl1_tau_s'] for r in rows_m})
        s['limit_reasons'] = sorted({r['limit_reasons'] for r in rows_m if r['limit_reasons'] not in ('0000', '')})
        if perf_total and freq:
            s['clk'] = avg([fnum(r[perf_total]) * fnum(r[freq]) / 100 for r in rows_c if fnum(r[perf_total]) is not None and fnum(r[freq])])
        else:
            s['clk'] = None
        s['util'] = avg([fnum(r[util]) for r in rows_c]) if util else None
        s['tz_max'] = mx([tz_c(r, c) for r in rows_c for c in tz_cols if 'tpol' not in c.lower()])
        s['tz_hot'] = None
        if tz_cols and rows_c:
            last = rows_c[-1]
            per = [(c, tz_c(last, c)) for c in tz_cols if 'tpol' not in c.lower()]
            per = [p for p in per if p[1] is not None]
            if per:
                c, v = max(per, key=lambda p: p[1])
                s['tz_hot'] = (re.search(r'\((.*?)\)', c).group(1), v)
        s['passive_min'] = mn([fnum(r[c]) for r in rows_c for c in pl_cols])
        s['discharge'] = avg([fnum(r.get('discharge_rate_mw')) for r in rows_c])
        s['charge'] = avg([fnum(r.get('charge_rate_mw')) for r in rows_c])
        return s

    os.makedirs(out, exist_ok=True)
    for mode in ORDER:
        ph = {p: (t0, t1) for (m, p, t0, t1) in phases if m == mode}
        if 'load' not in ph:
            continue
        stats = {p: phase_stats(sl(msr, *ph[p]), sl(cnt, *ph[p])) for p in ('idle', 'load', 'cool') if p in ph}
        lt0, lt1 = ph['load']
        load_m = sl(msr, lt0, lt1)
        first10 = phase_stats(sl(msr, lt0, lt0 + 10), sl(cnt, lt0, lt0 + 10))
        last30 = phase_stats(sl(msr, lt1 - 30, lt1), sl(cnt, lt1 - 30, lt1))
        # time until package power first falls below the midpoint between the burst peak and the sustained level
        tau_obs = None
        peak, sus = first10['pkg_w_max'], last30['pkg_w']
        if peak and sus and peak - sus > 3:
            thr = (peak + sus) / 2
            for r in load_m:
                if r['t'] > lt0 + 2 and fnum(r['pkg_w']) is not None and fnum(r['pkg_w']) < thr:
                    tau_obs = r['t'] - lt0
                    break
        smf_switch = smf_state(ph['idle'][0]) if 'idle' in ph else {}
        smf_load = smf_in(lt0, lt1)
        name, guid = MODE_NAMES[mode]

        timeline = []
        t = lt0
        while t < lt1:
            mm = sl(msr, t, t + 10)
            cc = sl(cnt, t, t + 10)
            s = phase_stats(mm, cc)
            timeline.append((int(t - lt0), s))
            t += 10

        row = {
            'power': power, 'mode': mode,
            'idle_w': stats.get('idle', {}).get('pkg_w'),
            'burst_w': first10['pkg_w_max'], 'sus_w': last30['pkg_w'], 'load_w': stats['load']['pkg_w'],
            'sus_clk': last30['clk'], 'sus_ratio_p': last30['ratio_p'], 'sus_ratio_e': last30['ratio_e'],
            'temp_max': stats['load']['pkg_temp_max'], 'tz_max': stats['load']['tz_max'],
            'tau_obs': tau_obs,
            'msr_pl1': ','.join(stats['load']['pl1']), 'msr_pl2': ','.join(stats['load']['pl2']),
            'smf': {str(k): v for k, v in sorted(smf_switch.items())},
            'smf_load_updates': {str(k): [min(v), max(v), len(v)] for k, v in sorted(smf_load.items())},
            'util': stats['load']['util'],
        }

        L = []
        L.append(f'# Power mode {name} ({power.upper()})\n')
        L.append(f'`tools/windows/perf-modes.ps1` output. Overlay `{guid}`. Load {meta.get("threads", "?")} threads, '
                 f'load QoS {meta.get("load_qos", "?")}. Phases: {meta.get("idle", "")}.\n')
        L.append('## Phase summary\n')
        L.append('| Phase | Package W avg/max | Core (PP0) W | Effective clock MHz | P-core/E-core ratio | Package °C avg/max | SAM thermal zone max °C | Util % |')
        L.append('|---|---|---|---|---|---|---|---|')
        for p, lbl in (('idle', 'Idle'), ('load', 'Load'), ('cool', 'Cool-down')):
            if p in stats:
                s = stats[p]
                L.append(f'| {lbl} | {f(s["pkg_w"])}/{f(s["pkg_w_max"])} | {f(s["pp0_w"])} | {f(s["clk"], 0)} | {f(s["ratio_p"])}/{f(s["ratio_e"])} | '
                         f'{f(s["pkg_temp"])}/{f(s["pkg_temp_max"])} | {f(s["tz_max"])} | {f(s["util"], 0)} |')
        L.append(f'| Load, first 10 s | {f(first10["pkg_w"])}/{f(first10["pkg_w_max"])} | {f(first10["pp0_w"])} | {f(first10["clk"], 0)} | '
                 f'{f(first10["ratio_p"])}/{f(first10["ratio_e"])} | {f(first10["pkg_temp"])}/{f(first10["pkg_temp_max"])} | {f(first10["tz_max"])} | {f(first10["util"], 0)} |')
        L.append(f'| Load, last 30 s | {f(last30["pkg_w"])}/{f(last30["pkg_w_max"])} | {f(last30["pp0_w"])} | {f(last30["clk"], 0)} | '
                 f'{f(last30["ratio_p"])}/{f(last30["ratio_e"])} | {f(last30["pkg_temp"])}/{f(last30["pkg_temp_max"])} | {f(last30["tz_max"])} | {f(last30["util"], 0)} |')
        L.append('')
        L.append('## Power limits\n')
        L.append(f'- MSR 0x610 (observed under load): PL1 {row["msr_pl1"]} W, PL2 {row["msr_pl2"]} W, PL1 tau {",".join(stats["load"]["tau"])} s')
        L.append(f'- Observed burst-to-sustained switch (first drop below the midpoint between peak and sustained): {f(tau_obs, 0)} s')
        L.append('- SMF CPU channels (last value at the start of idle after the mode switch, mW): ' + (', '.join(f'ch{k}={v}' for k, v in sorted(smf_switch.items())) or 'none'))
        L.append('- SMF updates under load (min/max/count): ' + (', '.join(f'ch{k}={v[0]}..{v[1]} x{v[2]}' for k, v in row['smf_load_updates'].items()) or 'none'))
        lr = sorted(set(sum((stats[p]['limit_reasons'] for p in stats), [])))
        L.append('- CORE_PERF_LIMIT_REASONS (0x64F) non-zero values: ' + (', '.join(lr) or 'none (PawnIO may block this MSR)'))
        pmin = stats['load']['passive_min']
        L.append(f'- ACPI thermal zone % Passive Limit min: {f(pmin, 0)}')
        if stats['load'].get('tz_hot'):
            L.append(f'- Hottest SAM thermal zone at end of load: {stats["load"]["tz_hot"][0]} {f(stats["load"]["tz_hot"][1])} °C')
        L.append('')
        if nodes:
            for p_, (a_, b_) in ph.items():
                ms = set(node_in('I_MS_NOTIFY', a_, b_))
                if ms - {4}:
                    L.append(f'Note: screen-off or standby (`I_MS_NOTIFY` {sorted(ms)}, 4 is normal) occurred during the {p_} phase. Values from this phase are not used.\n')
            L.append('## SMF nodes (SurfacePowerTrackerCore ETW)\n')
            L.append('Raw driver values (power mW, time ms, temperature 0.1 K, fan rpm). "After switch" is the last value at the start of idle after the mode switch.\n')
            L.append('| Node | After switch | Idle | Load min..max | End of load | End of cool-down |')
            L.append('|---|---|---|---|---|---|')
            it0 = ph['idle'][0] if 'idle' in ph else lt0
            rng = lambda xs: '-' if not xs else (str(xs[0]) if min(xs) == max(xs) else f'{min(xs)}..{max(xs)}')
            for k in KEY_NODES:
                a = node_at(k, it0)
                idle = node_in(k, *ph['idle']) if 'idle' in ph else []
                ld = node_in(k, lt0, lt1)
                if a is None and not ld:
                    continue
                ce = node_at(k, ph['cool'][1]) if 'cool' in ph else None
                L.append(f'| `{k}` | {a} | {rng(idle)} | {rng(ld)} | {node_at(k, lt1)} | {ce} |')
            L.append('')
            row['nodes'] = {k: node_at(k, it0) for k in KEY_NODES}
            ld_rpm = node_in('O_FAN1_ACTIVE_RPM', lt0, lt1)
            row['fan_load_max'] = max(ld_rpm) if ld_rpm else None
            co = node_in('O_FAN1_ACTIVE_RPM', *ph['cool']) if 'cool' in ph else []
            row['fan_cool_end'] = co[-1] if co else None
        L.append('## Load in 10 s steps\n')
        L.append('| s | Package W | Effective clock MHz | P/E ratio | Package °C | SAM max °C |')
        L.append('|---|---|---|---|---|---|')
        for t, s in timeline:
            L.append(f'| {t} | {f(s["pkg_w"])} | {f(s["clk"], 0)} | {f(s["ratio_p"])}/{f(s["ratio_e"])} | {f(s["pkg_temp"])} | {f(s["tz_max"])} |')
        L.append('')
        if nodes:
            L.append('| s | SMF PL1/PL2 (W) | Fan rpm (O_FAN1_ACTIVE_RPM) | I_CPU_TEMP °C |')
            L.append('|---|---|---|---|')
            t = lt0
            while t < lt1:
                pl1, pl2 = node_at('O_CPU_AVG', t + 10), node_at('O_CPU_MAX', t + 10)
                rpm, ct = node_at('O_FAN1_ACTIVE_RPM', t + 10), node_at('I_CPU_TEMP', t + 10)
                L.append(f'| {int(t - lt0)} | {pl1 and pl1 / 1000}/{pl2 and pl2 / 1000} | {rpm} | {f(ct / 10 - 273.15) if ct else "-"} |')
                t += 10
            L.append('')
        L.append(f'<!-- row: {json.dumps(row, ensure_ascii=False)} -->')
        with open(os.path.join(out, f'{power}-{mode}.md'), 'w', encoding='utf-8', newline='\n') as fo:
            fo.write('\n'.join(L) + '\n')

    # comparison table from every summary in out/
    rows = []
    for fn in sorted(os.listdir(out)):
        m = re.match(r'(ac|battery)-(\w+)\.md$', fn)
        if not m:
            continue
        txt = open(os.path.join(out, fn), encoding='utf-8').read()
        j = re.search(r'<!-- row: (.*) -->', txt)
        if j:
            rows.append(json.loads(j.group(1)))
    rows.sort(key=lambda r: (r['power'], ORDER.index(r['mode']) if r['mode'] in ORDER else 99))
    L = ['# CPU power and thermals per Windows power mode (perf-modes.ps1)\n',
         'Measured with `tools/windows/perf-modes.ps1`. Per mode: idle, all-core load, cool-down, 1 s interval. Per-mode data is in `<ac|battery>-<mode>.md`.',
         'Interpretation and Linux design: [docs/research/control/power-thermal.md](../../../docs/research/control/power-thermal.md).\n',
         '| Power | Mode | I_SLIDER | SMF PL1/PL2/PEAK (W), tau s | Fan profile | Fan rpm load max / cool-down end | Idle W | Load peak W | Sustained W (last 30 s) | Sustained effective clock MHz | P/E ratio | Package max °C | SAM max °C | Switch s | MSR 0x610 PL1/PL2 (W) |',
         '|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|']
    for r in rows:
        s = r['smf']
        nd = r.get('nodes') or {}
        w = lambda k: '-' if nd.get(k) is None else f'{nd[k] / 1000:g}'
        smfs = f'{w("O_CPU_AVG")}/{w("O_CPU_MAX")}/{w("O_CPU_PEAK")}, {w("O_CPU_AVG_TIME")}'
        L.append(f'| {r["power"].upper()} | {MODE_NAMES.get(r["mode"], (r["mode"],))[0]} | {nd.get("I_SLIDER", "-")} | {smfs} | {nd.get("O_FAN1_PROFILE", "-")} | '
                 f'{r.get("fan_load_max", "-")} / {r.get("fan_cool_end", "-")} | {f(r["idle_w"])} | {f(r["burst_w"])} | {f(r["sus_w"])} | '
                 f'{f(r["sus_clk"], 0)} | {f(r["sus_ratio_p"])}/{f(r["sus_ratio_e"])} | {f(r["temp_max"])} | {f(r["tz_max"])} | {f(r["tau_obs"], 0)} | {r["msr_pl1"]}/{r["msr_pl2"]} |')
    L.append('')
    L.append('"Switch s" is the time from load start until package power first drops below the midpoint between the burst peak and the sustained value (observed tau, estimate).')
    L.append('SMF values (I_SLIDER, PL, fan profile) are SurfacePowerTrackerCore ETW node values at the start of idle after the mode switch. Fan rpm is `O_FAN1_ACTIVE_RPM`.')
    with open(os.path.join(out, 'README.md'), 'w', encoding='utf-8', newline='\n') as fo:
        fo.write('\n'.join(L) + '\n')


if __name__ == '__main__':
    main(sys.argv[1], sys.argv[2])
