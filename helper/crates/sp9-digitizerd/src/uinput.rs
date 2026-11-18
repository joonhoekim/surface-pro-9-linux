// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
//! The virtual input devices, through /dev/uinput.

use sp9_digitizer::output::{Device, DeviceSpec, Setup, Sink, EV_SYN, SYN_REPORT};
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;

const IOC_WRITE: libc::c_ulong = 1;

const fn ioc(dir: libc::c_ulong, nr: u8, size: usize) -> libc::c_ulong {
    (dir << 30) | ((size as libc::c_ulong) << 16) | ((b'U' as libc::c_ulong) << 8) | nr as libc::c_ulong
}

const UINPUT_MAX_NAME_SIZE: usize = 80;

#[repr(C)]
struct UinputSetup {
    bustype: u16,
    vendor: u16,
    product: u16,
    version: u16,
    name: [u8; UINPUT_MAX_NAME_SIZE],
    ff_effects_max: u32,
}

#[repr(C)]
#[derive(Default)]
struct UinputAbsSetup {
    code: u16,
    value: i32,
    minimum: i32,
    maximum: i32,
    fuzz: i32,
    flat: i32,
    resolution: i32,
}

const INT: usize = std::mem::size_of::<libc::c_int>();
const UI_DEV_CREATE: libc::c_ulong = ioc(0, 1, 0);
const UI_DEV_DESTROY: libc::c_ulong = ioc(0, 2, 0);
const UI_DEV_SETUP: libc::c_ulong = ioc(IOC_WRITE, 3, std::mem::size_of::<UinputSetup>());
const UI_ABS_SETUP: libc::c_ulong = ioc(IOC_WRITE, 4, std::mem::size_of::<UinputAbsSetup>());
const UI_SET_EVBIT: libc::c_ulong = ioc(IOC_WRITE, 100, INT);
const UI_SET_KEYBIT: libc::c_ulong = ioc(IOC_WRITE, 101, INT);
const UI_SET_PROPBIT: libc::c_ulong = ioc(IOC_WRITE, 110, INT);

/// The size of `struct input_event` and the offset of its type field (after the timestamp).
const EVENT_SIZE: usize = std::mem::size_of::<libc::input_event>();
const EVENT_TYPE: usize = EVENT_SIZE - 8;

pub struct UinputDevice {
    file: File,
    /// The events of the packet that is being put together.
    packet: Vec<u8>,
}

impl UinputDevice {
    pub fn create(spec: &DeviceSpec) -> io::Result<UinputDevice> {
        let file = OpenOptions::new().write(true).custom_flags(libc::O_NONBLOCK).open("/dev/uinput")?;
        let fd = file.as_raw_fd();

        let set = |request: libc::c_ulong, value: u16| -> io::Result<()> {
            // SAFETY: these requests take an integer argument.
            if unsafe { libc::ioctl(fd, request as _, libc::c_int::from(value)) } < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        };

        for step in &spec.setup {
            match *step {
                Setup::Event(v) => set(UI_SET_EVBIT, v)?,
                Setup::Key(v) => set(UI_SET_KEYBIT, v)?,
                Setup::Property(v) => set(UI_SET_PROPBIT, v)?,
                Setup::Abs { code, min, max, resolution } => {
                    let abs = UinputAbsSetup { code, minimum: min, maximum: max, resolution, ..UinputAbsSetup::default() };
                    // SAFETY: the request reads a struct of this layout.
                    if unsafe { libc::ioctl(fd, UI_ABS_SETUP as _, &abs) } < 0 {
                        return Err(io::Error::last_os_error());
                    }
                }
            }
        }

        let mut setup = UinputSetup {
            bustype: spec.bus,
            vendor: spec.vendor,
            product: spec.product,
            version: spec.version,
            name: [0; UINPUT_MAX_NAME_SIZE],
            ff_effects_max: 0,
        };
        let name = spec.name.as_bytes();
        let len = name.len().min(UINPUT_MAX_NAME_SIZE - 1);
        setup.name[..len].copy_from_slice(&name[..len]);

        // SAFETY: the request reads a struct of this layout; the second one takes no argument.
        if unsafe { libc::ioctl(fd, UI_DEV_SETUP as _, &setup) } < 0 || unsafe { libc::ioctl(fd, UI_DEV_CREATE as _) } < 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(UinputDevice { file, packet: Vec::with_capacity(64 * EVENT_SIZE) })
    }

    fn push(&mut self, kind: u16, code: u16, value: i32) {
        let start = self.packet.len();
        self.packet.resize(start + EVENT_SIZE, 0);
        let event = &mut self.packet[start + EVENT_TYPE..];
        event[0..2].copy_from_slice(&kind.to_ne_bytes());
        event[2..4].copy_from_slice(&code.to_ne_bytes());
        event[4..8].copy_from_slice(&value.to_ne_bytes());
    }

    /// Ends the packet and writes it.
    fn flush(&mut self) -> io::Result<()> {
        self.push(EV_SYN, SYN_REPORT, 0);
        let result = self.file.write_all(&self.packet);
        self.packet.clear();
        result
    }
}

impl Drop for UinputDevice {
    fn drop(&mut self) {
        // SAFETY: the request takes no argument.
        unsafe { libc::ioctl(self.file.as_raw_fd(), UI_DEV_DESTROY as _) };
    }
}

/// The sink that writes to the devices. A failed write is kept until [`Uinput::take_error`].
pub struct Uinput {
    touchscreen: Option<UinputDevice>,
    stylus: Option<UinputDevice>,
    error: Option<io::Error>,
}

impl Uinput {
    pub fn create(specs: &[DeviceSpec]) -> io::Result<Uinput> {
        let mut uinput = Uinput { touchscreen: None, stylus: None, error: None };
        for spec in specs {
            let device = UinputDevice::create(spec)?;
            match spec.device {
                Device::Touchscreen => uinput.touchscreen = Some(device),
                Device::Stylus => uinput.stylus = Some(device),
            }
        }
        Ok(uinput)
    }

    fn device(&mut self, device: Device) -> Option<&mut UinputDevice> {
        match device {
            Device::Touchscreen => self.touchscreen.as_mut(),
            Device::Stylus => self.stylus.as_mut(),
        }
    }

    pub fn take_error(&mut self) -> Option<io::Error> {
        self.error.take()
    }
}

impl Sink for Uinput {
    fn event(&mut self, device: Device, kind: u16, code: u16, value: i32) {
        if let Some(d) = self.device(device) {
            d.push(kind, code, value);
        }
    }

    fn sync(&mut self, device: Device) {
        if let Some(Err(e)) = self.device(device).map(UinputDevice::flush) {
            self.error.get_or_insert(e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_numbers_and_layouts() {
        assert_eq!(std::mem::size_of::<UinputSetup>(), 92);
        assert_eq!(std::mem::size_of::<UinputAbsSetup>(), 28);
        assert_eq!(UI_DEV_CREATE, 0x5501);
        assert_eq!(UI_DEV_DESTROY, 0x5502);
        assert_eq!(UI_DEV_SETUP, 0x405C_5503);
        assert_eq!(UI_ABS_SETUP, 0x401C_5504);
        assert_eq!(UI_SET_EVBIT, 0x4004_5564);
        assert_eq!(UI_SET_KEYBIT, 0x4004_5565);
        assert_eq!(UI_SET_PROPBIT, 0x4004_556E);
        assert_eq!(EVENT_SIZE, 24);
    }
}
