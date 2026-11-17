#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
"""le-legacy-oob.py -- LE legacy OOB pairing through the kernel management socket.

Needs a kernel with "Bluetooth: SMP: Add LE legacy OOB pairing" and root (CAP_NET_ADMIN).
bluetoothd keeps running; it receives the same New Long Term Key / New IRK events.

  le-legacy-oob.py add-tk ADDR {random|public} TK    store the Temporary Key for a peer
  le-legacy-oob.py clear  ADDR {random|public}       drop the OOB data of a peer
  le-legacy-oob.py pair   ADDR {random|public}       Pair Device (NoInputNoOutput), wait for the result
  le-legacy-oob.py c1 ...                            offline: which TK byte order gives a captured confirm value
  le-legacy-oob.py selftest                          offline: c1 against the Core Specification sample data

TK is 32 hex digits, "-" (read from stdin) or @FILE. Its bytes go to the kernel in the order given:
the first byte is the least significant octet of the TK, as in the Security Manager TK Value (AD type
0x10) of an OOB data block. Write the same bytes, in the same order, that Windows writes to the cover.

Options: -i INDEX (controller, default 0), --io N (pair: IO capability, default 3), --timeout S (pair, default 30).

c1 takes what a trace of the pairing shows (btmon, tshark); addresses as printed (AA:BB:..), the rest
as hex in wire order:
  c1 --tk TK --preq 7B --prsp 7B --ia ADDR --iat {0|1} --ra ADDR --rat {0|1} --rand 16B --confirm 16B
--rand/--confirm are the Pairing Random and Pairing Confirm of the same device.
"""
import argparse
import ctypes
import os
import socket
import struct
import sys
import time

AF_BLUETOOTH = 31
BTPROTO_HCI = 1
HCI_DEV_NONE = 0xFFFF
HCI_CHANNEL_CONTROL = 3

OP_PAIR_DEVICE = 0x0019
OP_ADD_REMOTE_OOB_DATA = 0x0021
EV_CMD_COMPLETE = 0x0001
EV_CMD_STATUS = 0x0002
EVENTS = {
    0x000A: "New Long Term Key",
    0x000B: "Device Connected",
    0x000C: "Device Disconnected",
    0x000D: "Connect Failed",
    0x000F: "User Confirmation Request",
    0x0010: "User Passkey Request",
    0x0011: "Authentication Failed",
    0x0018: "New Identity Resolving Key",
    0x0019: "New Signature Resolving Key",
}
ADDR_TYPES = {"public": 1, "random": 2}


def die(msg):
    sys.exit(f"le-legacy-oob: {msg}")


def addr_bytes(text):
    parts = text.split(":")
    if len(parts) != 6:
        die("address must be AA:BB:CC:DD:EE:FF")
    return bytes(int(p, 16) for p in reversed(parts))


def read_tk(arg):
    if arg == "-":
        arg = sys.stdin.readline()
    elif arg.startswith("@"):
        with open(arg[1:]) as f:
            arg = f.read()
    tk = bytes.fromhex("".join(arg.split()).replace(":", ""))
    if len(tk) != 16:
        die("TK must be 16 bytes")
    return tk


class sockaddr_hci(ctypes.Structure):
    _fields_ = [("family", ctypes.c_ushort), ("dev", ctypes.c_ushort), ("channel", ctypes.c_ushort)]


def mgmt_open():
    s = socket.socket(AF_BLUETOOTH, socket.SOCK_RAW, BTPROTO_HCI)
    sa = sockaddr_hci(AF_BLUETOOTH, HCI_DEV_NONE, HCI_CHANNEL_CONTROL)
    libc = ctypes.CDLL(None, use_errno=True)
    if libc.bind(s.fileno(), ctypes.byref(sa), ctypes.sizeof(sa)) != 0:
        die("bind to the management channel: " + os.strerror(ctypes.get_errno()))
    return s


def mgmt_cmd(s, index, opcode, params, timeout, verbose=False):
    """Send a command and return (status, reply parameters) of its Command Complete/Status."""
    s.send(struct.pack("<HHH", opcode, index, len(params)) + params)
    end = time.monotonic() + timeout
    while True:
        left = end - time.monotonic()
        if left <= 0:
            die(f"no reply to command 0x{opcode:04x} within {timeout:g} s")
        s.settimeout(left)
        try:
            pkt = s.recv(1024)
        except socket.timeout:
            continue
        ev, idx, _ = struct.unpack_from("<HHH", pkt)
        body = pkt[6:]
        if idx != index:
            continue
        if ev in (EV_CMD_COMPLETE, EV_CMD_STATUS):
            op, status = struct.unpack_from("<HB", body)
            if op == opcode:
                return status, body[3:]
        elif verbose and ev in EVENTS:
            extra = ""
            if ev == 0x0011:
                extra = f" status 0x{body[7]:02x}"
            elif ev == 0x000A:
                # store_hint, then mgmt_ltk_info: addr 7, type 1, central 1, enc_size 1, ...
                extra = f" store_hint {body[0]} key type {body[8]} enc_size {body[10]}"
            elif ev == 0x0018:
                extra = f" store_hint {body[0]}"
            print(f"event: {EVENTS[ev]}{extra}", flush=True)


def cmd_add_tk(args, tk):
    addr = addr_bytes(args.addr) + bytes([ADDR_TYPES[args.type]])
    # Extended form: addr, hash192, rand192, hash256, rand256. hash192 carries the TK.
    params = addr + tk + bytes(48)
    status, _ = mgmt_cmd(mgmt_open(), args.index, OP_ADD_REMOTE_OOB_DATA, params, 5)
    if status == 0x0D and any(tk):
        die("Add Remote OOB Data: Invalid Parameters (kernel without LE legacy OOB support?)")
    if status:
        die(f"Add Remote OOB Data: status 0x{status:02x}")
    print("TK stored" if any(tk) else "OOB data cleared")


def cmd_pair(args):
    params = addr_bytes(args.addr) + bytes([ADDR_TYPES[args.type], args.io])
    status, _ = mgmt_cmd(mgmt_open(), args.index, OP_PAIR_DEVICE, params, args.timeout, verbose=True)
    if status:
        die(f"Pair Device: status 0x{status:02x}")
    print("paired")


# ---- offline: confirm value of LE legacy pairing (c1) ---------------------------------------------
def _xtime(a):
    return ((a << 1) ^ 0x1B) & 0xFF if a & 0x80 else a << 1


def _sbox():
    # Multiplicative inverse in GF(2^8) followed by the affine transform.
    exp, log, x = [0] * 255, [0] * 256, 1
    for i in range(255):
        exp[i], log[x] = x, i
        x ^= _xtime(x)
    box = []
    for v in range(256):
        q = exp[(255 - log[v]) % 255] if v else 0
        r = q
        for _ in range(4):
            q = ((q << 1) | (q >> 7)) & 0xFF
            r ^= q
        box.append(r ^ 0x63)
    return box


SBOX = _sbox()


def aes_ecb(key, block):
    """AES-128 encryption of one block (no dependency; only used by the offline checks)."""
    w = [list(key[i:i + 4]) for i in range(0, 16, 4)]
    rcon = 1
    for i in range(4, 44):
        t = list(w[i - 1])
        if i % 4 == 0:
            t = [SBOX[b] for b in t[1:] + t[:1]]
            t[0] ^= rcon
            rcon = _xtime(rcon)
        w.append([a ^ b for a, b in zip(w[i - 4], t)])
    rk = [sum(w[r * 4:r * 4 + 4], []) for r in range(11)]
    st = [a ^ b for a, b in zip(block, rk[0])]
    for r in range(1, 11):
        st = [SBOX[b] for b in st]
        st = [st[(i + 4 * (i % 4)) % 16] for i in range(16)]
        if r < 10:
            out = []
            for c in range(0, 16, 4):
                a = st[c:c + 4]
                t = a[0] ^ a[1] ^ a[2] ^ a[3]
                out += [a[i] ^ t ^ _xtime(a[i] ^ a[(i + 1) % 4]) for i in range(4)]
            st = out
        st = [a ^ b for a, b in zip(st, rk[r])]
    return bytes(st)


def smp_e(key_le, data_le):
    """Security function e on little-endian byte strings, as net/bluetooth/smp.c holds them."""
    return aes_ecb(key_le[::-1], data_le[::-1])[::-1]


def smp_c1(tk, rand, preq, prsp, iat, ia, rat, ra):
    """All arguments in wire order (least significant octet first); ia/ra as 6 bytes LSB first."""
    p1 = bytes([iat, rat]) + preq + prsp
    p2 = ra + ia + bytes(4)
    x = smp_e(tk, bytes(a ^ b for a, b in zip(rand, p1)))
    return smp_e(tk, bytes(a ^ b for a, b in zip(x, p2)))


def cmd_selftest():
    # Core Specification, Security Manager, sample data of the confirm value generation function
    # (printed there most significant octet first).
    got = smp_c1(
        bytes(16),
        bytes.fromhex("5783D52156AD6F0E6388274EC6702EE0")[::-1],
        bytes.fromhex("07071000000101")[::-1],
        bytes.fromhex("05000800000302")[::-1],
        1, bytes.fromhex("A1A2A3A4A5A6")[::-1],
        0, bytes.fromhex("B1B2B3B4B5B6")[::-1],
    )
    want = bytes.fromhex("1E1E3FEF878988EAD2A74DC5BEF13B86")[::-1]
    print("c1 sample data:", "ok" if got == want else "MISMATCH")
    return 0 if got == want else 1


def cmd_c1(args, tk):
    def hx(v, n):
        b = bytes.fromhex(v.replace(":", "").replace(" ", ""))
        if len(b) != n:
            die(f"expected {n} bytes: {v}")
        return b
    preq, prsp = hx(args.preq, 7), hx(args.prsp, 7)
    rand, confirm = hx(args.rand, 16), hx(args.confirm, 16)
    ia, ra = addr_bytes(args.ia), addr_bytes(args.ra)
    hit = False
    for name, key in (("as given", tk), ("reversed", tk[::-1]), ("zero (Just Works)", bytes(16))):
        ok = smp_c1(key, rand, preq, prsp, args.iat, ia, args.rat, ra) == confirm
        hit |= ok
        print(f"TK {name}: {'MATCH' if ok else 'no match'}")
    return 0 if hit else 1


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("-i", "--index", type=int, default=0)
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("add-tk")
    p.add_argument("addr"); p.add_argument("type", choices=ADDR_TYPES); p.add_argument("tk")
    p = sub.add_parser("clear")
    p.add_argument("addr"); p.add_argument("type", choices=ADDR_TYPES)
    p = sub.add_parser("pair")
    p.add_argument("addr"); p.add_argument("type", choices=ADDR_TYPES)
    p.add_argument("--io", type=int, default=3)
    p.add_argument("--timeout", type=float, default=30)
    p = sub.add_parser("c1")
    for name in ("tk", "preq", "prsp", "ia", "ra", "rand", "confirm"):
        p.add_argument("--" + name, required=True)
    p.add_argument("--iat", type=int, choices=(0, 1), required=True)
    p.add_argument("--rat", type=int, choices=(0, 1), required=True)
    sub.add_parser("selftest")
    args = ap.parse_args()

    if args.cmd == "add-tk":
        tk = read_tk(args.tk)
        if not any(tk):
            die("an all-zero TK means no TK")
        cmd_add_tk(args, tk)
    elif args.cmd == "clear":
        cmd_add_tk(args, bytes(16))
    elif args.cmd == "pair":
        cmd_pair(args)
    elif args.cmd == "c1":
        return cmd_c1(args, read_tk(args.tk))
    elif args.cmd == "selftest":
        return cmd_selftest()
    return 0


if __name__ == "__main__":
    sys.exit(main())
