// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
//! HID report descriptors and hidraw access. Report layouts are always computed from the
//! device's descriptor (as userspace/pen/surface_pen/hiddesc.py does), never hard-coded.

use crate::sys::Root;
use std::collections::HashMap;
use std::io;
use std::sync::{Arc, Mutex};

pub const SYS_HIDRAW: &str = "/sys/class/hidraw";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Input,
    Output,
    Feature,
}

/// Extended usage: page << 16 | id.
pub const fn usage(page: u16, id: u16) -> u32 {
    ((page as u32) << 16) | id as u32
}

#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub kind: Kind,
    pub report_id: u8,
    /// bit offset in the report after the report ID byte
    pub bit_offset: u32,
    pub size: u32,
    pub count: u32,
    pub usages: Vec<u32>,
    pub logical_min: i64,
    pub logical_max: i64,
    pub physical_min: i64,
    pub physical_max: i64,
    pub unit_exp: i32,
    pub constant: bool,
    /// usages of the enclosing collections, outermost first
    pub collections: Vec<u32>,
}

impl Field {
    pub fn usage_at(&self, k: usize) -> u32 {
        self.usages.get(k).or(self.usages.last()).copied().unwrap_or(0)
    }

    pub fn has_usage(&self, u: u32) -> bool {
        self.usages.contains(&u)
    }

    /// Value `k` of this field in a report (`data` starts with the report ID byte when the
    /// descriptor uses IDs; `skip` says how many bytes precede the fields).
    pub fn read(&self, data: &[u8], skip: usize, k: u32) -> Option<i64> {
        let start = self.bit_offset + k * self.size;
        let mut v: u64 = 0;
        for b in 0..self.size {
            let bit = (start + b) as usize;
            let byte = *data.get(skip + bit / 8)?;
            v |= (((byte >> (bit % 8)) & 1) as u64) << b;
        }
        if self.logical_min < 0 && self.size < 64 && v & (1 << (self.size - 1)) != 0 {
            Some(v as i64 - (1i64 << self.size))
        } else {
            Some(v as i64)
        }
    }

    fn write(&self, buf: &mut [u8], skip: usize, k: u32, value: i64) {
        let start = self.bit_offset + k * self.size;
        let v = value as u64;
        for b in 0..self.size {
            let bit = (start + b) as usize;
            let idx = skip + bit / 8;
            if idx >= buf.len() {
                return;
            }
            if (v >> b) & 1 != 0 {
                buf[idx] |= 1 << (bit % 8);
            } else {
                buf[idx] &= !(1 << (bit % 8));
            }
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Descriptor {
    pub fields: Vec<Field>,
    /// usages of the application collections
    pub applications: Vec<u32>,
    pub uses_ids: bool,
    bits: HashMap<(Kind, u8), u32>,
}

#[derive(Clone, Default)]
struct Globals {
    page: u16,
    lmin: i64,
    lmax_raw: (u32, usize),
    pmin: i64,
    pmax: i64,
    exp: i32,
    size: u32,
    count: u32,
    id: u8,
}

fn signed(v: u32, n: usize) -> i64 {
    match n {
        1 => v as u8 as i8 as i64,
        2 => v as u16 as i16 as i64,
        4 => v as i32 as i64,
        _ => 0,
    }
}

pub fn parse(d: &[u8]) -> Result<Descriptor, String> {
    let mut out = Descriptor::default();
    let mut g = Globals::default();
    let mut stack: Vec<Globals> = Vec::new();
    let mut usages: Vec<u32> = Vec::new();
    let mut umin: Option<u32> = None;
    let mut colls: Vec<u32> = Vec::new();
    let mut i = 0;
    while i < d.len() {
        let b = d[i];
        if b == 0xFE {
            let n = *d.get(i + 1).ok_or("truncated long item")? as usize;
            i += 3 + n;
            continue;
        }
        let n = [0, 1, 2, 4][(b & 3) as usize];
        let data = d.get(i + 1..i + 1 + n).ok_or("truncated item")?;
        let v = data.iter().rev().fold(0u32, |a, x| (a << 8) | *x as u32);
        let full = |v: u32, page: u16| if n == 4 { v } else { usage(page, v as u16) };
        match ((b >> 2) & 3, b >> 4) {
            (0, tag @ (8 | 9 | 0xB)) => {
                let kind = match tag {
                    8 => Kind::Input,
                    9 => Kind::Output,
                    _ => Kind::Feature,
                };
                let lmin = g.lmin;
                let mut lmax = signed(g.lmax_raw.0, g.lmax_raw.1);
                if lmax < lmin {
                    lmax = g.lmax_raw.0 as i64;
                }
                let key = (kind, g.id);
                let off = *out.bits.get(&key).unwrap_or(&0);
                out.fields.push(Field {
                    kind,
                    report_id: g.id,
                    bit_offset: off,
                    size: g.size,
                    count: g.count,
                    usages: std::mem::take(&mut usages),
                    logical_min: lmin,
                    logical_max: lmax,
                    physical_min: g.pmin,
                    physical_max: g.pmax,
                    unit_exp: g.exp,
                    constant: v & 1 != 0,
                    collections: colls.clone(),
                });
                out.bits.insert(key, off + g.size * g.count);
                umin = None;
            }
            (0, 0xA) => {
                let u = usages.first().copied().unwrap_or(0);
                if v == 1 {
                    out.applications.push(u);
                }
                colls.push(u);
                usages.clear();
                umin = None;
            }
            (0, 0xC) => {
                colls.pop();
            }
            (1, 0) => g.page = v as u16,
            (1, 1) => g.lmin = signed(v, n),
            (1, 2) => g.lmax_raw = (v, n),
            (1, 3) => g.pmin = signed(v, n),
            (1, 4) => g.pmax = signed(v, n),
            (1, 5) => g.exp = if v > 7 { v as i32 - 16 } else { v as i32 },
            (1, 7) => g.size = v,
            (1, 8) => {
                g.id = v as u8;
                out.uses_ids = true;
            }
            (1, 9) => g.count = v,
            (1, 0xA) => stack.push(g.clone()),
            (1, 0xB) => g = stack.pop().ok_or("pop without push")?,
            (2, 0) => usages.push(full(v, g.page)),
            (2, 1) => umin = Some(full(v, g.page)),
            (2, 2) => {
                let lo = umin.take().ok_or("usage maximum without minimum")?;
                let hi = full(v, g.page);
                if hi >= lo && hi - lo < 4096 {
                    usages.extend(lo..=hi);
                }
            }
            _ => {}
        }
        i += 1 + n;
    }
    Ok(out)
}

impl Descriptor {
    /// The first field of `kind` with `usage`, optionally inside a collection with `within`.
    pub fn find(&self, kind: Kind, u: u32, within: Option<u32>) -> Option<&Field> {
        self.fields.iter().find(|f| f.kind == kind && f.has_usage(u) && within.is_none_or(|w| f.collections.contains(&w)))
    }

    pub fn has_application(&self, u: u32) -> bool {
        self.applications.contains(&u)
    }

    pub fn fields_of(&self, kind: Kind, id: u8) -> impl Iterator<Item = &Field> {
        self.fields.iter().filter(move |f| f.kind == kind && f.report_id == id)
    }

    /// Bytes before the fields (the report ID).
    pub fn skip(&self) -> usize {
        usize::from(self.uses_ids)
    }

    /// Report length in bytes, report ID included.
    pub fn report_len(&self, kind: Kind, id: u8) -> usize {
        let bits = *self.bits.get(&(kind, id)).unwrap_or(&0) as usize;
        bits.div_ceil(8) + self.skip()
    }

    /// Build a report: every listed usage gets its value (first slot of its field), the rest
    /// is zero. Values must be inside the field's logical range (0 is always accepted, as
    /// "unset").
    pub fn pack(&self, kind: Kind, id: u8, values: &[(u32, i64)]) -> Result<Vec<u8>, String> {
        let mut buf = vec![0u8; self.report_len(kind, id)];
        if self.uses_ids {
            buf[0] = id;
        }
        for &(u, v) in values {
            let f = self.fields_of(kind, id).find(|f| f.has_usage(u)).ok_or_else(|| format!("usage {u:#010x} not in report {id}"))?;
            if v != 0 && !(f.logical_min..=f.logical_max).contains(&v) {
                return Err(format!("value {v} outside {}..{} for usage {u:#010x}", f.logical_min, f.logical_max));
            }
            let k = f.usages.iter().position(|x| *x == u).unwrap_or(0) as u32;
            f.write(&mut buf, self.skip(), k.min(f.count.saturating_sub(1)), v);
        }
        Ok(buf)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct HidNode {
    /// "hidraw3"
    pub name: String,
    pub bus: u16,
    pub vendor: u16,
    pub product: u16,
    pub hid_name: String,
    pub descriptor: Vec<u8>,
}

pub trait HidBackend: Send + Sync {
    fn nodes(&self) -> Vec<HidNode>;
    fn get_feature(&self, node: &str, id: u8, len: usize) -> io::Result<Vec<u8>>;
    fn write_output(&self, node: &str, data: &[u8]) -> io::Result<()>;
}

/// HID_ID=bbbb:vvvvvvvv:pppppppp
pub fn parse_hid_id(s: &str) -> Option<(u16, u16, u16)> {
    let mut it = s.split(':').map(|p| u32::from_str_radix(p, 16).ok());
    Some((it.next()?? as u16, it.next()?? as u16, it.next()?? as u16))
}

pub struct Hidraw {
    root: Root,
}

impl Hidraw {
    pub fn new(root: Root) -> Self {
        Hidraw { root }
    }

    #[cfg(target_os = "linux")]
    fn open(&self, node: &str, write: bool) -> io::Result<std::fs::File> {
        if node.contains('/') || !node.starts_with("hidraw") {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "bad hidraw name"));
        }
        std::fs::OpenOptions::new().read(true).write(write).open(self.root.p(&format!("/dev/{node}")))
    }
}

impl HidBackend for Hidraw {
    fn nodes(&self) -> Vec<HidNode> {
        self.root
            .list(SYS_HIDRAW)
            .into_iter()
            .filter(|n| n.starts_with("hidraw"))
            .filter_map(|n| {
                let dev = format!("{SYS_HIDRAW}/{n}/device");
                let uevent = self.root.read_opt(&format!("{dev}/uevent")).unwrap_or_default();
                let field = |k: &str| uevent.lines().find_map(|l| l.strip_prefix(k)?.strip_prefix('=')).unwrap_or("").to_string();
                let (bus, vendor, product) = parse_hid_id(&field("HID_ID")).unwrap_or((0, 0, 0));
                let descriptor = std::fs::read(self.root.p(&format!("{dev}/report_descriptor"))).ok()?;
                Some(HidNode { hid_name: field("HID_NAME"), name: n, bus, vendor, product, descriptor })
            })
            .collect()
    }

    #[cfg(target_os = "linux")]
    fn get_feature(&self, node: &str, id: u8, len: usize) -> io::Result<Vec<u8>> {
        use std::os::fd::AsRawFd;
        let f = self.open(node, false)?;
        let mut buf = vec![0u8; len.max(1)];
        buf[0] = id;
        // HIDIOCGFEATURE(len) = _IOC(_IOC_READ|_IOC_WRITE, 'H', 0x07, len)
        let req = (3u32 << 30) | ((buf.len() as u32) << 16) | ((b'H' as u32) << 8) | 0x07;
        // SAFETY: buf is valid for buf.len() bytes, which is the size encoded in req.
        let n = unsafe { libc::ioctl(f.as_raw_fd(), req as _, buf.as_mut_ptr()) };
        if n < 0 {
            return Err(io::Error::last_os_error());
        }
        buf.truncate(n as usize);
        Ok(buf)
    }

    #[cfg(target_os = "linux")]
    fn write_output(&self, node: &str, data: &[u8]) -> io::Result<()> {
        use std::io::Write;
        self.open(node, true)?.write_all(data)
    }

    #[cfg(not(target_os = "linux"))]
    fn get_feature(&self, _node: &str, _id: u8, _len: usize) -> io::Result<Vec<u8>> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "hidraw needs Linux"))
    }

    #[cfg(not(target_os = "linux"))]
    fn write_output(&self, _node: &str, _data: &[u8]) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "hidraw needs Linux"))
    }
}

#[derive(Clone, Debug, Default)]
pub struct FakeHidState {
    pub nodes: Vec<HidNode>,
    /// (node, report id) -> feature report (report ID byte first)
    pub features: HashMap<(String, u8), Vec<u8>>,
    pub writes: Vec<(String, Vec<u8>)>,
}

#[derive(Clone, Default)]
pub struct FakeHid(pub Arc<Mutex<FakeHidState>>);

impl HidBackend for FakeHid {
    fn nodes(&self) -> Vec<HidNode> {
        self.0.lock().unwrap().nodes.clone()
    }
    fn get_feature(&self, node: &str, id: u8, len: usize) -> io::Result<Vec<u8>> {
        let s = self.0.lock().unwrap();
        let mut v = s.features.get(&(node.to_string(), id)).cloned().ok_or_else(|| io::Error::other("feature not readable"))?;
        v.truncate(len);
        Ok(v)
    }
    fn write_output(&self, node: &str, data: &[u8]) -> io::Result<()> {
        let mut s = self.0.lock().unwrap();
        if !s.nodes.iter().any(|n| n.name == node) {
            return Err(io::Error::new(io::ErrorKind::NotFound, "no such node"));
        }
        s.writes.push((node.to_string(), data.to_vec()));
        Ok(())
    }
}

/// Small descriptor builder for tests and fakes.
pub mod build {
    pub fn item(tag_type: u8, v: u32) -> Vec<u8> {
        if v <= 0xFF {
            vec![tag_type | 1, v as u8]
        } else if v <= 0xFFFF {
            vec![tag_type | 2, v as u8, (v >> 8) as u8]
        } else {
            let b = v.to_le_bytes();
            vec![tag_type | 3, b[0], b[1], b[2], b[3]]
        }
    }
    pub fn page(p: u16) -> Vec<u8> {
        item(0x04, p as u32)
    }
    pub fn usage(u: u16) -> Vec<u8> {
        item(0x08, u as u32)
    }
    pub fn lmin(v: u32) -> Vec<u8> {
        item(0x14, v)
    }
    pub fn lmax(v: u32) -> Vec<u8> {
        item(0x24, v)
    }
    pub fn size(v: u32) -> Vec<u8> {
        item(0x74, v)
    }
    pub fn count(v: u32) -> Vec<u8> {
        item(0x94, v)
    }
    pub fn id(v: u8) -> Vec<u8> {
        item(0x84, v as u32)
    }
    pub fn collection(t: u8) -> Vec<u8> {
        vec![0xA1, t]
    }
    pub fn end() -> Vec<u8> {
        vec![0xC0]
    }
    pub fn input(f: u8) -> Vec<u8> {
        vec![0x81, f]
    }
    pub fn output(f: u8) -> Vec<u8> {
        vec![0x91, f]
    }
    pub fn feature(f: u8) -> Vec<u8> {
        vec![0xB1, f]
    }
    pub fn cat(parts: &[Vec<u8>]) -> Vec<u8> {
        parts.concat()
    }
}

#[cfg(test)]
mod tests {
    use super::build::*;
    use super::{parse, HidBackend, Hidraw, Kind, SYS_HIDRAW};
    use crate::sys::FakeTree;

    #[test]
    fn parse_and_pack() {
        let d = cat(&[
            page(0x0C),
            usage(0x01),
            collection(1),
            id(3),
            lmin(0),
            lmax(0x3FF),
            size(16),
            count(1),
            usage(0x1),
            input(0),
            id(9),
            lmin(0),
            lmax(255), // 1-byte 0xFF: must read as 255, not -1
            size(8),
            count(2),
            usage(0x20),
            usage(0x21),
            output(2),
            end(),
        ]);
        let p = parse(&d).unwrap();
        assert!(p.uses_ids);
        assert!(p.has_application(usage_ext(0x0C, 0x01)));
        let f = p.find(Kind::Output, usage_ext(0x0C, 0x21), None).unwrap();
        assert_eq!((f.logical_max, f.count, f.bit_offset), (255, 2, 0));
        assert_eq!(p.report_len(Kind::Output, 9), 3);
        assert_eq!(p.pack(Kind::Output, 9, &[(usage_ext(0x0C, 0x21), 200)]).unwrap(), vec![9, 0, 200]);
        assert!(p.pack(Kind::Output, 9, &[(usage_ext(0x0C, 0x99), 1)]).is_err());
        let fi = p.find(Kind::Input, usage_ext(0x0C, 0x01), None).unwrap();
        assert_eq!(fi.read(&[3, 0x34, 0x02], 1, 0), Some(0x234));
    }

    fn usage_ext(p: u16, u: u16) -> u32 {
        super::usage(p, u)
    }

    #[test]
    fn sysfs_nodes() {
        let t = FakeTree::new("hidraw");
        t.file(&format!("{SYS_HIDRAW}/hidraw2/device/uevent"), "HID_ID=0005:0000045E:00000C0F\nHID_NAME=Surface Pen\n")
            .file(&format!("{SYS_HIDRAW}/hidraw2/device/report_descriptor"), [0x05u8, 0x0D]);
        let n = Hidraw::new(t.root.clone()).nodes();
        assert_eq!(n.len(), 1);
        assert_eq!((n[0].bus, n[0].vendor, n[0].product), (5, 0x045E, 0x0C0F));
        assert_eq!(n[0].hid_name, "Surface Pen");
    }
}
