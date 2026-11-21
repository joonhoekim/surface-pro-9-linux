#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
"""Copy the Slim Pen 2 bond Windows made (out-of-band coupling in the cover slot) into BlueZ.

Windows and Linux use the same adapter address, so the pen cannot tell them apart: with the same LTK it
treats Linux as the host it was coupled with. Reads the Windows SYSTEM hive offline (the partition is
mounted read-only for the copy and unmounted again) and writes /var/lib/bluetooth/<adapter>/<pen>/info.

    sudo tools/linux/pen-bond-from-windows.py                 # show what would be written (keys masked)
    sudo tools/linux/pen-bond-from-windows.py --install       # write it and restart bluetooth
    sudo tools/linux/pen-bond-from-windows.py --hive SYSTEM   # a hive already copied out

Windows must have been restarted or shut down after the coupling (not hibernated), and the pen must not be
paired again on Linux in between: a new pairing replaces the key in the pen.

The bond is under the pen's coupling address toward this host. sp9-helperd learns the pen's part of that
address from it (the address XOR the host address), so coupling through the cover works on Linux afterwards.

Conversion (checked against an HCI capture of the Windows pairing and against a BlueZ bond):
LTK as is, EDIV decimal, ERand read as a little-endian integer, IRK byte-reversed.
Needs hivexregedit (`nix shell nixpkgs#hivex -c ...` is used when it is not on PATH).
"""
import argparse, glob, os, re, shutil, subprocess, sys, tempfile

KEYS = r"\Services\BTHPORT\Parameters\Keys"


def run(cmd, **kw):
    return subprocess.run(cmd, capture_output=True, text=True, **kw)


def find_windows_partition():
    """NTFS partitions of internal disks. Removable and hot-plugged ones (a USB stick) are left out: their
    hive would be taken as the Windows bond, and their file system is mounted by root."""
    out = run(["lsblk", "-rno", "PATH,FSTYPE,RM,HOTPLUG"]).stdout
    return [f[0] for f in (l.split() for l in out.splitlines()) if f[1:] == ["ntfs", "0", "0"]]


def copy_hive(dev, dest):
    mnt = tempfile.mkdtemp(prefix="sp9-win-", dir="/run")
    for fstype in ("ntfs3", "ntfs-3g"):
        if run(["mount", "-o", "ro", "-t", fstype, dev, mnt]).returncode == 0:
            break
    else:
        os.rmdir(mnt)
        return False
    try:
        src = os.path.join(mnt, "Windows/System32/config/SYSTEM")
        if not os.path.exists(src):
            return False
        shutil.copy(src, dest)
        return True
    finally:
        run(["umount", mnt])
        os.rmdir(mnt)


def export(hive, path):
    cmd = ["hivexregedit", "--export", "--prefix", "H", hive, path]
    if not shutil.which("hivexregedit"):
        cmd = ["nix", "shell", "nixpkgs#hivex", "-c"] + cmd
    r = run(cmd)
    if r.returncode:
        sys.exit("hivexregedit: " + r.stderr.strip()[:300])
    return r.stdout.replace("\\\n  ", "")


def parse(reg):
    """{adapter: {device: {name: bytes|int}}} with addresses as 12 hex digits."""
    res, cur = {}, None
    for line in reg.splitlines():
        m = re.match(r"\[H.*\\Keys\\([0-9a-f]{12})(?:\\([0-9a-f]{12}))?\]$", line, re.I)
        if m:
            cur = res.setdefault(m.group(1).lower(), {}).setdefault(m.group(2).lower(), {}) if m.group(2) else None
            continue
        if line.startswith("["):
            cur = None
        m = re.match(r'"([^"]+)"=(?:hex(?:\([0-9a-f]+\))?:([0-9a-f,]*)|dword:([0-9a-f]{8}))$', line, re.I)
        if m and cur is not None:
            cur[m.group(1)] = int(m.group(3), 16) if m.group(3) else bytes.fromhex(m.group(2).replace(",", ""))
    return res


def colons(a):
    return ":".join(a[i:i + 2] for i in range(0, 12, 2)).upper()


def mask(addr, show):
    return addr if show else "xx:xx:xx:xx:xx:" + addr[-2:]


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--hive", help="SYSTEM hive file (default: copy it from the Windows partition)")
    ap.add_argument("--device", help="Windows partition (default: the only NTFS partition with Windows on it)")
    ap.add_argument("--pen", help="pen coupling address as Windows stores it (default: the only LE bond in the hive)")
    ap.add_argument("--install", action="store_true", help="write the BlueZ info file and restart bluetooth")
    ap.add_argument("--show", action="store_true", help="print addresses and keys in full")
    a = ap.parse_args()

    tmp = None
    hive = a.hive
    if not hive:
        if os.geteuid():
            sys.exit("run as root (mounts the Windows partition read-only)")
        tmp = tempfile.mkdtemp(prefix="sp9-hive-")
        hive = os.path.join(tmp, "SYSTEM")
        devs = [a.device] if a.device else find_windows_partition()
        found = [d for d in devs if copy_hive(d, hive)]
        if not found:
            sys.exit("no Windows SYSTEM hive found on " + ", ".join(devs or ["(no internal NTFS partition)"]))
        if len(found) > 1:
            sys.exit("Windows SYSTEM hives on " + ", ".join(found) + ": choose one with --device")
    try:
        sel = export(hive, r"\Select")
        m = re.search(r'"Current"=dword:([0-9a-f]{8})', sel)
        cs = "ControlSet%03d" % (int(m.group(1), 16) if m else 1)
        bonds = parse(export(hive, "\\" + cs + KEYS))
    finally:
        if tmp:
            shutil.rmtree(tmp)

    adapters = [os.path.basename(p) for p in glob.glob("/var/lib/bluetooth/??:??:??:??:??:??")]
    cands = []
    for ad, devs in bonds.items():
        for dev, v in devs.items():
            if isinstance(v.get("LTK"), bytes) and "EDIV" in v:
                cands.append((ad, dev, v))
    # The bond is under the pen's coupling address, not the pairing-mode address BlueZ may already know.
    if a.pen:
        cands = [c for c in cands if c[1] == a.pen.replace(":", "").lower()]
    if len(cands) != 1:
        sys.exit(f"{len(cands)} LE bonds match in the hive ({cs}); pass --pen")
    ad, dev, v = cands[0]
    adapter, pen_addr = colons(ad), colons(dev)

    ltk, irk = v["LTK"], v.get("IRK")
    rand = int.from_bytes(v["ERand"], "little")
    auth = 1 if v.get("AuthReq", 0) & 0x04 else 0
    addr_type = "static" if v.get("AddressType", 1) == 1 else "public"
    sec = lambda b: b.hex().upper() if a.show else "<%d B>" % len(b)
    print(f"hive {cs}: adapter {mask(adapter, a.show)} pen {mask(pen_addr, a.show)} ({addr_type})")
    print(f"  LTK {sec(ltk)} size {v.get('KeyLength', 16)} EDIV {v['EDIV'] if a.show else '<n>'} "
          f"Rand {rand if a.show else '<n>'} authenticated {auth} (AuthReq 0x{v.get('AuthReq', 0):02x}) IRK {sec(irk) if irk else 'none'}")

    path = f"/var/lib/bluetooth/{adapter}/{pen_addr}/info"
    old = open(path).read() if os.path.exists(path) else ""
    if old:
        m = re.search(r"\[LongTermKey\][^\[]*?\nKey=([0-9A-F]+)", old)
        print("  BlueZ has a bond for this pen:", "same LTK" if m and m.group(1) == ltk.hex().upper() else "different LTK (will be replaced)")
    keep = {}
    for name, body in re.findall(r"^\[([^\]]+)\]\n((?:(?!\[).*\n?)*)", old, re.M):
        keep[name] = body.strip("\n")
    # Services= makes BlueZ probe HID over GATT at load and arm auto-connect for a device it has never connected to.
    general = keep.get("General") or (f"Name=Surface Slim Pen 2\nAppearance=0x03c7\nAddressType={addr_type}\nSupportedTechnologies=LE;\n"
                                      "Trusted=true\nBlocked=false\nWakeAllowed=true\n"
                                      "Services=00001800-0000-1000-8000-00805f9b34fb;00001801-0000-1000-8000-00805f9b34fb;"
                                      "0000180a-0000-1000-8000-00805f9b34fb;0000180f-0000-1000-8000-00805f9b34fb;00001812-0000-1000-8000-00805f9b34fb;")
    if "Trusted=" not in general:
        general += "\nTrusted=true"
    general = re.sub(r"Trusted=\w+", "Trusted=true", general)
    out = [f"[General]\n{general}\n"]
    if irk:
        out.append(f"[IdentityResolvingKey]\nKey={irk[::-1].hex().upper()}\n")
    out.append(f"[LongTermKey]\nKey={ltk.hex().upper()}\nAuthenticated={auth}\nEncSize={v.get('KeyLength', 16)}\nEDiv={v['EDIV']}\nRand={rand}\n")
    for name in ("DeviceID", "ConnectionParameters"):
        if name in keep:
            out.append(f"[{name}]\n{keep[name]}\n")
    text = "\n".join(out)

    if not a.install:
        print("  not written (pass --install). Sections:", ", ".join(re.findall(r"^\[([^\]]+)\]", text, re.M)))
        return
    if os.geteuid():
        sys.exit("run as root to install")
    if adapter not in adapters:
        sys.exit(f"BlueZ has no adapter directory for {mask(adapter, a.show)}")
    run(["systemctl", "stop", "bluetooth"])
    try:
        os.makedirs(os.path.dirname(path), mode=0o700, exist_ok=True)
        # the first state this tool replaced; a second run keeps it
        if old and not os.path.exists(path + ".before-windows-bond"):
            shutil.copy(path, path + ".before-windows-bond")
        with open(path, "w") as f:
            f.write(text)
        os.chmod(path, 0o600)
    finally:
        run(["systemctl", "start", "bluetooth"])
    print("  written:", path if a.show else path.replace(adapter, mask(adapter, False)).replace(pen_addr, mask(pen_addr, False)))


if __name__ == "__main__":
    main()
