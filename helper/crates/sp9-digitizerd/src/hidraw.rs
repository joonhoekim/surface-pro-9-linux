// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
//! The hidraw node of the digitizer.

use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::path::Path;

const IOC_WRITE: libc::c_ulong = 1;
const IOC_READ: libc::c_ulong = 2;

const fn ioc(dir: libc::c_ulong, kind: u8, nr: u8, size: usize) -> libc::c_ulong {
    (dir << 30) | ((size as libc::c_ulong) << 16) | ((kind as libc::c_ulong) << 8) | nr as libc::c_ulong
}

/// The largest report descriptor the kernel hands out.
const HID_MAX_DESCRIPTOR_SIZE: usize = 4096;

#[repr(C)]
struct RawInfo {
    bustype: u32,
    vendor: i16,
    product: i16,
}

#[repr(C)]
struct RawDescriptor {
    size: u32,
    value: [u8; HID_MAX_DESCRIPTOR_SIZE],
}

const HIDIOCGRDESCSIZE: libc::c_ulong = ioc(IOC_READ, b'H', 0x01, std::mem::size_of::<libc::c_int>());
const HIDIOCGRDESC: libc::c_ulong = ioc(IOC_READ, b'H', 0x02, std::mem::size_of::<RawDescriptor>());
const HIDIOCGRAWINFO: libc::c_ulong = ioc(IOC_READ, b'H', 0x03, std::mem::size_of::<RawInfo>());

const fn hidiocsfeature(len: usize) -> libc::c_ulong {
    ioc(IOC_READ | IOC_WRITE, b'H', 0x06, len)
}

const fn hidiocgfeature(len: usize) -> libc::c_ulong {
    ioc(IOC_READ | IOC_WRITE, b'H', 0x07, len)
}

pub struct Hidraw {
    file: File,
}

impl Hidraw {
    pub fn open(path: &Path) -> io::Result<Hidraw> {
        Ok(Hidraw { file: OpenOptions::new().read(true).write(true).open(path)? })
    }

    fn ioctl<T>(&self, request: libc::c_ulong, arg: *mut T) -> io::Result<()> {
        // SAFETY: every caller passes a pointer to a live object of the size the request encodes.
        if unsafe { libc::ioctl(self.file.as_raw_fd(), request as _, arg) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Vendor and product ID.
    pub fn ids(&self) -> io::Result<(u16, u16)> {
        let mut info = RawInfo { bustype: 0, vendor: 0, product: 0 };
        self.ioctl(HIDIOCGRAWINFO, &mut info)?;
        Ok((info.vendor as u16, info.product as u16))
    }

    /// The report descriptor.
    pub fn descriptor(&self) -> io::Result<Vec<u8>> {
        let mut size: libc::c_int = 0;
        self.ioctl(HIDIOCGRDESCSIZE, &mut size)?;
        let mut desc = Box::new(RawDescriptor { size: size as u32, value: [0; HID_MAX_DESCRIPTOR_SIZE] });
        self.ioctl(HIDIOCGRDESC, &mut *desc)?;
        Ok(desc.value[..(desc.size as usize).min(HID_MAX_DESCRIPTOR_SIZE)].to_vec())
    }

    /// Reads a feature report into `buf`, whose first byte is the report ID.
    pub fn get_feature(&self, buf: &mut [u8]) -> io::Result<()> {
        self.ioctl(hidiocgfeature(buf.len()), buf.as_mut_ptr())
    }

    /// Writes a feature report, whose first byte is the report ID.
    pub fn set_feature(&self, report: &[u8]) -> io::Result<()> {
        // The kernel only reads the buffer, but the request takes a mutable pointer.
        let mut copy = report.to_vec();
        self.ioctl(hidiocsfeature(copy.len()), copy.as_mut_ptr())
    }

    /// Reads one input report. Blocks; a signal ends the wait with `Interrupted` (reading
    /// through `File` would retry, and the daemon is told to stop that way).
    pub fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        // SAFETY: the buffer is valid for writes of its length.
        let n = unsafe { libc::read(self.file.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
        if n < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(n as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_numbers() {
        assert_eq!(HIDIOCGRDESCSIZE, 0x8004_4801);
        assert_eq!(HIDIOCGRDESC, 0x9004_4802);
        assert_eq!(HIDIOCGRAWINFO, 0x8008_4803);
        assert_eq!(hidiocsfeature(2), 0xC002_4806);
        assert_eq!(hidiocgfeature(120), 0xC078_4807);
    }
}
