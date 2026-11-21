# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# Extract Intel VBT blobs ("$VBT" header + BDB) from a firmware image of your own device, found by
# searching for the "$VBT" signature and cut to the VBT's own size field.
# Usage: python -I vbt-extract.py <firmware.bin> <outdir>
# Prints offset, BDB version, size, checksum and sha256 of each blob and writes <outdir>/vbt<N>.bin.
# Decode the result with intel_vbt_decode (igt-gpu-tools) or compare with Linux /sys/kernel/debug/dri/0/i915_vbt.
import hashlib
import os
import struct
import sys

src, outdir = sys.argv[1], sys.argv[2]
os.makedirs(outdir, exist_ok=True)
data = open(src, 'rb').read()
pos, n = 0, 0
while (pos := data.find(b'$VBT', pos)) >= 0:
    ver, hdr_size, vbt_size, _, _, bdb_off = struct.unpack_from('<HHHBBI', data, pos + 20)
    blob = data[pos:pos + vbt_size]
    bdb_ver = struct.unpack_from('<H', blob, bdb_off + 16)[0] if blob[bdb_off:bdb_off + 4] == b'BIOS' else None
    ok = sum(blob) & 0xff == 0
    print(f'{n}: offset {pos} id {blob[4:20].decode("ascii", "replace").strip()!r} size {vbt_size} '
          f'bdb {bdb_ver} checksum {"ok" if ok else "bad"} sha256 {hashlib.sha256(blob).hexdigest()}')
    with open(os.path.join(outdir, f'vbt{n}.bin'), 'wb') as f:
        f.write(blob)
    n += 1
    pos += 4
