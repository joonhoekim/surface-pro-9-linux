// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
// Derived from iptsd (src/hid/*.hpp, src/ipts/descriptor.hpp, src/ipts/protocol/descriptor.hpp),
// GPL-2.0-or-later, Copyright (C) the iptsd authors.
//! The parts of the digitizer's HID report descriptor the daemon needs: which input reports
//! carry raw touch data, and the feature reports for the mode switch and the metadata.
//!
//! Reports are collected per collection, as the reference implementation does: a report whose
//! fields are spread over several collections shows up once per collection, each with only the
//! fields declared there. The lookups below (and the report sizes) depend on that.

const PAGE_DIGITIZER: u32 = 0x000D;
const PAGE_VENDOR: u32 = 0xFF00;
const USAGE_TOUCHSCREEN: u32 = 0x04;
const USAGE_TOUCHPAD: u32 = 0x05;
const USAGE_SCAN_TIME: u32 = 0x56;
const USAGE_GESTURE_DATA: u32 = 0x61;
const USAGE_METADATA: u32 = 0x63;
const USAGE_SET_MODE: u32 = 0xC8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReportKind {
    Input,
    Output,
    Feature,
}

#[derive(Clone, Copy, Debug)]
struct Field {
    usage: u32,
    usage_range: Option<(u32, u32)>,
    bits: usize,
}

/// The fields of one report within one collection.
#[derive(Clone, Debug)]
pub struct Report {
    pub kind: ReportKind,
    pub id: Option<u8>,
    fields: Vec<Field>,
}

impl Report {
    /// Size of the fields in bytes, without the report ID.
    pub fn bytes(&self) -> usize {
        self.fields.iter().map(|f| f.bits).sum::<usize>().div_ceil(8)
    }

    fn has_usage(&self, page: u32, id: u32) -> bool {
        let usage = (page << 16) | id;
        self.fields.iter().any(|f| f.usage == usage || f.usage_range.is_some_and(|(min, max)| min <= usage && usage <= max))
    }
}

#[derive(Clone, Debug, Default)]
struct Collection {
    usage: u32,
    reports: Vec<Report>,
    children: Vec<Collection>,
}

impl Collection {
    fn reports<'a>(&'a self, out: &mut Vec<&'a Report>) {
        out.extend(&self.reports);
        for c in &self.children {
            c.reports(out);
        }
    }

    fn any_usage(&self, usage: u32) -> bool {
        self.children.iter().any(|c| c.usage == usage || c.any_usage(usage))
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Global {
    usage_page: u32,
    report_id: Option<u8>,
    report_count: u32,
    report_size: u32,
}

#[derive(Clone, Copy, Debug, Default)]
struct Local {
    usage: u32,
    usage_min: Option<u32>,
    usage_max: Option<u32>,
}

/// A parsed report descriptor.
#[derive(Clone, Debug)]
pub struct Descriptor {
    root: Collection,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Truncated;

impl Descriptor {
    pub fn parse(bytes: &[u8]) -> Result<Descriptor, Truncated> {
        let mut global = Global::default();
        let mut global_stack = Vec::new();
        let mut local = Local::default();
        let mut current = Collection::default();
        let mut parents: Vec<Collection> = Vec::new();

        let mut i = 0;
        while i < bytes.len() {
            let header = bytes[i];
            i += 1;
            let size = [0, 1, 2, 4][usize::from(header & 3)];
            let tag = header >> 2;
            let data = bytes.get(i..i + size).ok_or(Truncated)?;
            i += size;
            let payload = data.iter().rev().fold(0u32, |acc, &b| (acc << 8) | u32::from(b));
            // A usage of four bytes carries its page, a shorter one takes the current page.
            let usage = if size == 4 { payload } else { global.usage_page | payload };

            match tag {
                // Main items: input, output, feature
                0b100000 | 0b100100 | 0b101100 => {
                    let kind = match tag {
                        0b100000 => ReportKind::Input,
                        0b100100 => ReportKind::Output,
                        _ => ReportKind::Feature,
                    };
                    let field = Field {
                        usage: local.usage,
                        usage_range: local.usage_min.zip(local.usage_max),
                        bits: global.report_count as usize * global.report_size as usize,
                    };
                    match current.reports.iter_mut().find(|r| r.id == global.report_id && r.kind == kind) {
                        Some(report) => report.fields.push(field),
                        None => current.reports.push(Report { kind, id: global.report_id, fields: vec![field] }),
                    }
                    local = Local::default();
                }
                // Collection, end collection
                0b101000 => {
                    parents.push(std::mem::take(&mut current));
                    current.usage = local.usage;
                    local = Local::default();
                }
                0b110000 => {
                    let mut parent = parents.pop().ok_or(Truncated)?;
                    parent.children.push(std::mem::take(&mut current));
                    current = parent;
                }
                // Global items
                0b000001 => global.usage_page = payload << 16,
                0b011101 => global.report_size = payload,
                0b100001 => global.report_id = Some(payload as u8),
                0b100101 => global.report_count = payload,
                0b101001 => global_stack.push(global),
                0b101101 => global = global_stack.pop().ok_or(Truncated)?,
                // Local items
                0b000010 => local.usage = usage,
                0b000110 => local.usage_min = Some(usage),
                0b001010 => local.usage_max = Some(usage),
                _ => {}
            }
        }

        // Collections that were not closed are dropped, like in the reference parser.
        Ok(Descriptor { root: current })
    }

    fn reports(&self) -> Vec<&Report> {
        let mut out = Vec::new();
        self.root.reports(&mut out);
        out
    }

    /// The input reports that carry raw touch data (scan time and gesture data usages).
    pub fn touch_data_reports(&self) -> Vec<&Report> {
        self.reports()
            .into_iter()
            .filter(|r| {
                r.kind == ReportKind::Input
                    && r.has_usage(PAGE_DIGITIZER, USAGE_SCAN_TIME)
                    && r.has_usage(PAGE_DIGITIZER, USAGE_GESTURE_DATA)
            })
            .collect()
    }

    /// The one-byte feature report that switches between single-touch and multi-touch mode.
    pub fn mode_report(&self) -> Option<&Report> {
        self.reports().into_iter().find(|r| r.kind == ReportKind::Feature && r.bytes() == 1 && r.has_usage(PAGE_VENDOR, USAGE_SET_MODE))
    }

    /// The feature report that returns the device metadata.
    pub fn metadata_report(&self) -> Option<&Report> {
        self.reports().into_iter().find(|r| r.kind == ReportKind::Feature && r.has_usage(PAGE_DIGITIZER, USAGE_METADATA))
    }

    pub fn is_touchscreen(&self) -> bool {
        self.root.any_usage((PAGE_DIGITIZER << 16) | USAGE_TOUCHSCREEN)
    }

    pub fn is_touchpad(&self) -> bool {
        self.root.any_usage((PAGE_DIGITIZER << 16) | USAGE_TOUCHPAD)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A descriptor in the shape of the digitizer's: a touchscreen collection with a fallback
    /// report, and a vendor collection with two raw data reports, the mode and the metadata.
    pub(crate) fn sample() -> Vec<u8> {
        let mut d = vec![
            0x05, 0x0D, // usage page digitizer
            0x09, 0x04, // usage touchscreen
            0xA1, 0x01, // collection application
            0x85, 0x40, //   report id 0x40
            0x09, 0x56, //   usage scan time
            0x75, 0x10, 0x95, 0x01, 0x81, 0x02, // 16 bit input
            0xC0, 0x06, 0x00, 0xFF, // usage page vendor
            0x09, 0x01, 0xA1, 0x01, // collection
        ];
        for (id, bytes) in [(0x0Bu8, 100u16), (0x1A, 300)] {
            d.extend([0x85, id, 0x05, 0x0D, 0x09, 0x56, 0x75, 0x10, 0x95, 0x01, 0x81, 0x02]);
            d.extend([0x09, 0x61, 0x75, 0x08, 0x96]);
            d.extend(bytes.to_le_bytes());
            d.extend([0x81, 0x03]);
        }
        // mode: one byte feature with a four byte usage
        d.extend([0x85, 0x05, 0x0B, 0xC8, 0x00, 0x00, 0xFF, 0x75, 0x08, 0x95, 0x01, 0xB1, 0x02]);
        // metadata
        d.extend([0x85, 0x06, 0x05, 0x0D, 0x09, 0x63, 0x75, 0x08, 0x95, 0x77, 0xB1, 0x02]);
        d.push(0xC0);
        d
    }

    #[test]
    fn finds_the_reports() {
        let d = Descriptor::parse(&sample()).unwrap();
        assert!(d.is_touchscreen());
        assert!(!d.is_touchpad());
        let touch: Vec<_> = d.touch_data_reports().iter().map(|r| (r.id, r.bytes())).collect();
        assert_eq!(touch, vec![(Some(0x0B), 102), (Some(0x1A), 302)]);
        assert_eq!(d.mode_report().map(|r| r.id), Some(Some(5)));
        assert_eq!(d.metadata_report().map(|r| (r.id, r.bytes())), Some((Some(6), 0x77)));
    }

    #[test]
    fn truncated() {
        let mut d = sample();
        // An item that announces two bytes at the very end.
        d.truncate(d.len() - 1);
        d.push(0x96);
        assert_eq!(Descriptor::parse(&d).err(), Some(Truncated));
        assert_eq!(Descriptor::parse(&[0xC0]).err(), Some(Truncated));
    }
}
