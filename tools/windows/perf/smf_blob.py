# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
"""Decode the Surface SMF policy blob (UEFI variable SMF_DATA_BLOB-a3d05fa9-db5e-490c-956e-a57384a4b70b).

The SMF core driver reads this variable at boot, LZNT1-decompresses it and builds its node graph from the CSV text
(power limits, fan and thermal tables per power-slider state). Works on Linux too:

  linux:   python3 smf_blob.py /sys/firmware/efi/efivars/SMF_DATA_BLOB-a3d05fa9-db5e-490c-956e-a57384a4b70b out.txt
  windows: tools/windows/perf/smf-blob.ps1 (reads the variable, elevated) then this script

An efivarfs file starts with a 4-byte attribute word; it is skipped when present. The blob has a 20-byte header
(5 x u32: id, product, board, format, compressed size) followed by LZNT1 data.
"""
import struct
import sys


def lznt1(src):
    out = bytearray()
    i = 0
    while i + 2 <= len(src):
        hdr = struct.unpack_from('<H', src, i)[0]
        i += 2
        if hdr == 0:
            break
        size = (hdr & 0x0fff) + 1
        chunk = src[i:i + size]
        i += size
        if not hdr & 0x8000:            # stored chunk
            out += chunk
            continue
        start = len(out)
        j = 0
        while j < len(chunk):
            flags = chunk[j]
            j += 1
            for bit in range(8):
                if j >= len(chunk):
                    break
                if not flags & (1 << bit):
                    out.append(chunk[j])
                    j += 1
                    continue
                tok = struct.unpack_from('<H', chunk, j)[0]
                j += 2
                pos = len(out) - start - 1
                shift = 12
                while pos >= 0x10:
                    pos >>= 1
                    shift -= 1
                length = (tok & ((1 << shift) - 1)) + 3
                disp = (tok >> shift) + 1
                for _ in range(length):
                    out.append(out[-disp])
    return bytes(out)


def main(path, out):
    d = open(path, 'rb').read()
    if len(d) > 24 and struct.unpack_from('<I', d, 4)[0] >> 24 == 0x5d and struct.unpack_from('<I', d, 0)[0] < 0x100:
        d = d[4:]                       # efivarfs attribute word
    hdr = struct.unpack_from('<5I', d)
    text = lznt1(d[20:]).decode('latin1')
    with open(out, 'w', encoding='utf-8', newline='\n') as fo:
        fo.write('# header ' + ' '.join(f'0x{x:x}' for x in hdr) + '\n')
        fo.write(text.replace('\r\n', '\n'))
    print(f'header {[hex(x) for x in hdr]}, {len(text)} bytes of text -> {out}')


if __name__ == '__main__':
    main(sys.argv[1], sys.argv[2])
