# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
"""Minimal HID report descriptor parser and report packer (HID 1.11, section 6.2.2)."""

from dataclasses import dataclass, field

INPUT, OUTPUT, FEATURE = "input", "output", "feature"
_MAIN = {0x8: INPUT, 0x9: OUTPUT, 0xB: FEATURE}


def usage(page, uid):
    return (page << 16) | uid


@dataclass
class Collection:
    usage: int
    kind: int
    parent: "Collection | None" = None

    def path(self):
        c, out = self, []
        while c is not None:
            out.append(c.usage)
            c = c.parent
        return out[::-1]


@dataclass
class Field:
    kind: str
    report_id: int
    bit_offset: int
    size: int
    count: int
    usages: list
    flags: int
    logical_min: int
    logical_max: int
    physical_min: int
    physical_max: int
    unit: int
    unit_exp: int
    collection: Collection | None

    @property
    def constant(self):
        return bool(self.flags & 0x01)

    @property
    def variable(self):
        return bool(self.flags & 0x02)

    def usage_at(self, i):
        if not self.usages:
            return None
        return self.usages[min(i, len(self.usages) - 1)]

    def in_collection(self, u):
        c = self.collection
        while c is not None:
            if c.usage == u:
                return True
            c = c.parent
        return False


@dataclass
class Descriptor:
    fields: list = field(default_factory=list)
    collections: list = field(default_factory=list)

    def report_ids(self, kind):
        return sorted({f.report_id for f in self.fields if f.kind == kind})

    def report(self, kind, report_id):
        return [f for f in self.fields if f.kind == kind and f.report_id == report_id]

    def report_size(self, kind, report_id):
        """Bytes on the wire including the report ID byte (if IDs are used)."""
        fs = self.report(kind, report_id)
        bits = max((f.bit_offset + f.size * f.count for f in fs), default=0)
        return (1 if report_id else 0) + (bits + 7) // 8

    def find(self, kind, u, report_id=None, within=None):
        for f in self.fields:
            if f.kind != kind or f.constant or u not in f.usages:
                continue
            if report_id is not None and f.report_id != report_id:
                continue
            if within is not None and not f.in_collection(within):
                continue
            return f
        return None

    def has_application(self, u):
        return any(c.usage == u and c.kind == 1 for c in self.collections)


def _sign(v, nbytes):
    bits = 8 * nbytes
    return v - (1 << bits) if nbytes and v & (1 << (bits - 1)) else v


def parse(data):
    d = Descriptor()
    g = dict(page=0, lmin=0, lmax=0, pmin=0, pmax=0, unit=0, exp=0, size=0, count=0, rid=0, _lmax_n=0)
    lmin_raw = (0, 0)
    stack = []
    local_usages, umin, umax = [], None, None
    offsets = {}
    col = None
    i = 0
    while i < len(data):
        b = data[i]
        if b == 0xFE:
            i += 3 + data[i + 1]
            continue
        n = (0, 1, 2, 4)[b & 3]
        raw = int.from_bytes(bytes(data[i + 1:i + 1 + n]), "little")
        i += 1 + n
        tag, typ = b >> 4, (b >> 2) & 3

        def full(v):
            return v if n == 4 else usage(g["page"], v)

        if typ == 0:
            if tag in _MAIN:
                kind = _MAIN[tag]
                us = list(local_usages)
                if umin is not None and umax is not None:
                    us += list(range(umin, umax + 1))
                key = (kind, g["rid"])
                off = offsets.get(key, 0)
                lmin = _sign(*lmin_raw)
                lmax = g["lmax"]
                if lmin < 0:
                    lmax = _sign(lmax, g["_lmax_n"])
                d.fields.append(Field(kind, g["rid"], off, g["size"], g["count"], us, raw,
                                      lmin, lmax, g["pmin"], g["pmax"], g["unit"], g["exp"], col))
                offsets[key] = off + g["size"] * g["count"]
            elif tag == 0xA:
                u = local_usages[0] if local_usages else (umin or 0)
                col = Collection(u, raw, col)
                d.collections.append(col)
            elif tag == 0xC:
                col = col.parent if col else None
            local_usages, umin, umax = [], None, None
        elif typ == 1:
            if tag == 0x0:
                g["page"] = raw
            elif tag == 0x1:
                g["lmin"] = raw
                lmin_raw = (raw, n)
            elif tag == 0x2:
                g["lmax"], g["_lmax_n"] = raw, n
            elif tag == 0x3:
                g["pmin"] = _sign(raw, n)
            elif tag == 0x4:
                g["pmax"] = _sign(raw, n)
            elif tag == 0x5:
                g["exp"] = _sign(raw, n) if raw > 15 else (raw - 16 if raw >= 8 else raw)
            elif tag == 0x6:
                g["unit"] = raw
            elif tag == 0x7:
                g["size"] = raw
            elif tag == 0x8:
                g["rid"] = raw
            elif tag == 0x9:
                g["count"] = raw
            elif tag == 0xA:
                stack.append((dict(g), lmin_raw))
            elif tag == 0xB:
                g, lmin_raw = stack.pop()
        elif typ == 2:
            if tag == 0x0:
                local_usages.append(full(raw))
            elif tag == 0x1:
                umin = full(raw)
            elif tag == 0x2:
                umax = full(raw)
    return d


def _get_bits(buf, off, size):
    v = int.from_bytes(buf, "little")
    return (v >> off) & ((1 << size) - 1)


def unpack(desc, kind, data):
    """Decode a report (first byte = report ID when IDs are used) into {usage: value or [values]}."""
    rid = data[0] if desc.report_ids(kind) != [0] else 0
    body = bytes(data[1:] if rid else data)
    out = {}
    for f in desc.report(kind, rid):
        if f.constant or not f.usages:
            continue
        vals = []
        for k in range(f.count):
            v = _get_bits(body, f.bit_offset + k * f.size, f.size)
            if f.logical_min < 0:
                v = v - (1 << f.size) if v >> (f.size - 1) else v
            vals.append(v)
        if f.variable:
            distinct = len(set(f.usages)) > 1
            if distinct:
                for k, v in enumerate(vals):
                    out[f.usage_at(k)] = v
            else:
                out[f.usages[0]] = vals[0] if f.count == 1 else vals
        else:
            for v in vals:
                if f.logical_min <= v <= f.logical_max and v - f.logical_min < len(f.usages):
                    out[f.usages[v - f.logical_min]] = 1
    return rid, out


def pack(desc, kind, report_id, values, base=None):
    """Encode {usage: int | bytes | list} into a report; unspecified fields keep `base` (or zero)."""
    size = desc.report_size(kind, report_id)
    prefix = 1 if report_id else 0
    body = bytearray(size - prefix)
    if base is not None:
        b = bytes(base)[prefix:prefix + len(body)]
        body[:len(b)] = b
    acc = int.from_bytes(body, "little")
    for f in desc.report(kind, report_id):
        if f.constant or not f.variable:
            continue
        for k in range(f.count):
            u = f.usage_at(k)
            if u not in values:
                continue
            v = values[u]
            if isinstance(v, (bytes, bytearray, list, tuple)):
                if len(set(f.usages)) > 1:
                    raise ValueError("sequence given for a per-usage field")
                v = v[k]
            mask = (1 << f.size) - 1
            off = f.bit_offset + k * f.size
            acc = (acc & ~(mask << off)) | ((int(v) & mask) << off)
    body = acc.to_bytes(len(body), "little")
    return (bytes([report_id]) if report_id else b"") + body
