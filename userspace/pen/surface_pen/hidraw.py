# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
"""Linux hidraw access: sysfs enumeration, feature get/set ioctls, output writes."""

import glob
import os

from . import hiddesc

SYS_HIDRAW = "/sys/class/hidraw"
BUS_BLUETOOTH = 0x05


def _ioc(direction, nr, size):
    return (direction << 30) | (size << 16) | (ord("H") << 8) | nr


def HIDIOCGFEATURE(n):
    return _ioc(3, 0x07, n)


def HIDIOCSFEATURE(n):
    return _ioc(3, 0x06, n)


class Node:
    def __init__(self, name, sys_root=SYS_HIDRAW):
        self.name = name
        self.sys = os.path.join(sys_root, name)
        self.dev = "/dev/" + name
        self.uevent = {}
        try:
            with open(os.path.join(self.sys, "device", "uevent")) as f:
                for line in f:
                    k, _, v = line.strip().partition("=")
                    self.uevent[k] = v
        except OSError:
            pass
        with open(os.path.join(self.sys, "device", "report_descriptor"), "rb") as f:
            self.raw_descriptor = f.read()
        self.descriptor = hiddesc.parse(self.raw_descriptor)

    @property
    def ids(self):
        """(bus, vendor, product) from HID_ID=bbbb:vvvvvvvv:pppppppp."""
        try:
            b, v, p = self.uevent["HID_ID"].split(":")
            return int(b, 16), int(v, 16), int(p, 16)
        except (KeyError, ValueError):
            return None

    @property
    def hid_name(self):
        return self.uevent.get("HID_NAME", "")

    def __repr__(self):
        ids = self.ids
        s = "%04x:%04x:%04x" % ids if ids else "?"
        return "%s (%s %s)" % (self.dev, s, self.hid_name)


def nodes(sys_root=SYS_HIDRAW):
    out = []
    for p in sorted(glob.glob(os.path.join(sys_root, "hidraw*"))):
        try:
            out.append(Node(os.path.basename(p), sys_root))
        except OSError:
            continue
    return out


class Device:
    def __init__(self, path, write=False):
        flags = (os.O_RDWR if write else os.O_RDONLY) | os.O_NONBLOCK
        self.fd = os.open(path, flags)

    def close(self):
        if self.fd >= 0:
            os.close(self.fd)
            self.fd = -1

    def __enter__(self):
        return self

    def __exit__(self, *a):
        self.close()

    def get_feature(self, report_id, size):
        import fcntl
        buf = bytearray(size)
        buf[0] = report_id
        n = fcntl.ioctl(self.fd, HIDIOCGFEATURE(size), buf, True)
        return bytes(buf[:n])

    def set_feature(self, data):
        import fcntl
        buf = bytearray(data)
        fcntl.ioctl(self.fd, HIDIOCSFEATURE(len(buf)), buf, True)

    def write_output(self, data):
        return os.write(self.fd, bytes(data))

    def read(self, size=16384):
        return os.read(self.fd, size)


def format_addr(b, lsb_first=True):
    """6 raw bytes -> 'AA:BB:CC:DD:EE:FF'. Bluetooth HCI order is LSB first."""
    b = bytes(b)
    return ":".join("%02X" % x for x in (b[::-1] if lsb_first else b))


def mask_addr(s, show=False):
    """Hide all but the last octet of a printed BD address."""
    return s if show or not s else "XX:XX:XX:XX:XX:" + s[-2:]
