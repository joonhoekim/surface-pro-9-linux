# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
"""BlueZ D-Bus access (dbus-next, asyncio)."""

BUS = "org.bluez"
DEVICE = "org.bluez.Device1"
ADAPTER = "org.bluez.Adapter1"
PROPS = "org.freedesktop.DBus.Properties"

PEN_MODALIAS = ("usb:v045Ep0C0F", "bluetooth:v045Ep0C0F")
PEN_NAMES = ("Surface Slim Pen 2",)


def is_pen(props):
    mod = props.get("Modalias", "")
    name = props.get("Name", "") or props.get("Alias", "")
    return mod.upper().startswith(tuple(m.upper() for m in PEN_MODALIAS)) or name in PEN_NAMES


class BlueZ:
    def __init__(self):
        self.bus = None

    async def connect(self):
        from dbus_next.aio import MessageBus
        from dbus_next import BusType
        self.bus = await MessageBus(bus_type=BusType.SYSTEM).connect()
        intro = await self.bus.introspect(BUS, "/")
        obj = self.bus.get_proxy_object(BUS, "/", intro)
        self.om = obj.get_interface("org.freedesktop.DBus.ObjectManager")
        return self

    async def objects(self):
        raw = await self.om.call_get_managed_objects()
        return {p: {i: {k: v.value for k, v in props.items()} for i, props in ifaces.items()}
                for p, ifaces in raw.items()}

    async def adapters(self):
        return {p: o[ADAPTER] for p, o in (await self.objects()).items() if ADAPTER in o}

    async def pens(self):
        return {p: o[DEVICE] for p, o in (await self.objects()).items()
                if DEVICE in o and is_pen(o[DEVICE])}

    async def _iface(self, path, iface):
        intro = await self.bus.introspect(BUS, path)
        return self.bus.get_proxy_object(BUS, path, intro).get_interface(iface)

    async def set_trusted(self, path):
        from dbus_next import Variant
        props = await self._iface(path, PROPS)
        await props.call_set(DEVICE, "Trusted", Variant("b", True))

    async def connect_device(self, path):
        dev = await self._iface(path, DEVICE)
        await dev.call_connect()

    async def pair(self, path):
        dev = await self._iface(path, DEVICE)
        await dev.call_pair()
