// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
//! Slim Pen 2 haptics over its BLE HID hidraw node: the Simple Haptic Controller (page 0x0E)
//! output report (65 on this pen), the same reports userspace/pen/surface_pen/haptic.py writes.
//! Windows-style inking feedback: one start report on tip down (the continuous waveform that
//! is set, brush unless chosen; its ordinal comes from the pen's waveform list), one stop
//! report on tip up, nothing in between (docs/research/gaps/pen-ble-trace.md 3.3). The eraser end
//! gets one click on touch-down instead.

use crate::hid::{self, HidBackend, Kind};
use std::io;

pub const PEN_VENDOR: u16 = 0x045E;
pub const PEN_PRODUCT: u16 = 0x0C0F;
pub const STYLUS: u32 = hid::usage(0x0D, 0x20);
pub const SIMPLE_HAPTIC_CONTROLLER: u32 = hid::usage(0x0E, 0x01);
pub const MANUAL_TRIGGER: u32 = hid::usage(0x0E, 0x21);
pub const INTENSITY: u32 = hid::usage(0x0E, 0x23);
pub const REPEAT_COUNT: u32 = hid::usage(0x0E, 0x24);

pub const ORDINAL_STOP: i64 = 2;
/// Feature 66 of this pen as read on Windows: 3 = click, 9 = ink-continuous.
pub const ORDINAL_CLICK: i64 = 3;
pub const ORDINAL_INK: i64 = 9;
pub const ORDINAL_PAGE: u16 = 0x0A;
/// Simple Haptic Controller: the collection of the waveform list feature (ordinal -> waveform).
pub const WAVEFORM_LIST: u32 = hid::usage(0x0E, 0x10);

/// The continuous waveforms the pen offers for inking. Windows applications pick one per
/// tool: of 230 continuous starts in three captures 172 were brush (OneNote), 50 pencil
/// (Journal), 3 ink. The eraser waveform is not offered: the eraser end plays nothing
/// continuous.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Waveform {
    Ink,
    Pencil,
    Marker,
    ChiselMarker,
    #[default]
    Brush,
    Sparkle,
}

impl Waveform {
    pub const ALL: [Waveform; 6] =
        [Waveform::Ink, Waveform::Pencil, Waveform::Marker, Waveform::ChiselMarker, Waveform::Brush, Waveform::Sparkle];

    pub fn name(self) -> &'static str {
        match self {
            Waveform::Ink => "ink",
            Waveform::Pencil => "pencil",
            Waveform::Marker => "marker",
            Waveform::ChiselMarker => "chisel-marker",
            Waveform::Brush => "brush",
            Waveform::Sparkle => "sparkle",
        }
    }

    pub fn from_name(name: &str) -> Option<Waveform> {
        Self::ALL.into_iter().find(|w| w.name() == name)
    }

    /// The HID waveform usage (page 0x0E): ink-continuous 0x100B ... sparkle-continuous 0x1011.
    pub fn id(self) -> u16 {
        match self {
            Waveform::Ink => 0x100B,
            Waveform::Pencil => 0x100C,
            Waveform::Marker => 0x100D,
            Waveform::ChiselMarker => 0x100E,
            Waveform::Brush => 0x100F,
            Waveform::Sparkle => 0x1011,
        }
    }

    /// The ordinal in this pen's waveform list as Windows read it (Feature 66), for when the
    /// list cannot be read.
    pub fn fixed_ordinal(self) -> i64 {
        match self {
            Waveform::Ink => 9,
            Waveform::Pencil => 10,
            Waveform::Marker => 11,
            Waveform::ChiselMarker => 12,
            Waveform::Brush => 13,
            Waveform::Sparkle => 15,
        }
    }

    /// The ordinal to trigger: from the pen's list (`PenHaptic::waveform_list`), else fixed.
    pub fn ordinal(self, list: &[(i64, u16)]) -> i64 {
        list.iter().find(|(_, id)| *id == self.id()).map(|(o, _)| *o).unwrap_or(self.fixed_ordinal())
    }
}
/// Windows' inking start intensity (#1066 capture `41 00 7f 09 …`) and stop report
/// (`41 00 fe 02 …`).
pub const INK_INTENSITY: u8 = 0x7F;
pub const STOP_INTENSITY: i64 = 0xFE;
/// Windows' "tactile signals" strength setting is this intensity byte and nothing else:
/// minimum 2, default 127, maximum 254.
pub const INTENSITY_RANGE: (u8, u8) = (2, 254);

/// Strength in percent (100 = Windows' default intensity 127) as the intensity byte, kept
/// inside what Windows sends (2..=254, about 2 %..200 %).
pub fn strength_to_intensity(percent: f64) -> Option<u8> {
    let (lo, hi) = INTENSITY_RANGE;
    percent.is_finite().then(|| (INK_INTENSITY as f64 * percent / 100.0).round().clamp(lo as f64, hi as f64) as u8)
}

pub fn intensity_to_strength(intensity: u8) -> f64 {
    intensity as f64 * 100.0 / INK_INTENSITY as f64
}

pub struct PenHaptic {
    pub node: String,
    desc: hid::Descriptor,
    out_id: u8,
}

/// The pen's hidraw node with a haptic output report (only while the pen is connected).
pub fn find(h: &dyn HidBackend) -> Option<PenHaptic> {
    h.nodes().into_iter().filter(|n| n.vendor == PEN_VENDOR && n.product == PEN_PRODUCT).find_map(|n| {
        let desc = hid::parse(&n.descriptor).ok()?;
        if !desc.has_application(STYLUS) {
            return None;
        }
        let out_id = desc.find(Kind::Output, MANUAL_TRIGGER, Some(SIMPLE_HAPTIC_CONTROLLER))?.report_id;
        Some(PenHaptic { node: n.name, desc, out_id })
    })
}

impl PenHaptic {
    /// The trigger report; repeat, cutoff and retrigger stay 0 as Windows sends them.
    pub fn report(&self, ordinal: i64, intensity: i64) -> io::Result<Vec<u8>> {
        self.desc
            .pack(Kind::Output, self.out_id, &[(MANUAL_TRIGGER, ordinal), (INTENSITY, intensity)])
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))
    }

    /// The remembered node still is the pen (hidraw numbers are reused once it is gone).
    pub fn alive(&self, h: &dyn HidBackend) -> bool {
        h.ids(&self.node).is_some_and(|(_, vendor, product)| vendor == PEN_VENDOR && product == PEN_PRODUCT)
    }

    fn write(&self, h: &dyn HidBackend, report: &[u8]) -> io::Result<()> {
        if !self.alive(h) {
            return Err(io::Error::new(io::ErrorKind::NotFound, "the pen's node is gone"));
        }
        h.write_output(&self.node, report)
    }

    pub fn trigger(&self, h: &dyn HidBackend, ordinal: i64, intensity: u8) -> io::Result<()> {
        self.write(h, &self.report(ordinal, intensity.max(1) as i64)?)
    }

    /// The pen's waveform list (ordinal, waveform usage), read from its feature report; empty
    /// when the pen has none or does not answer.
    pub fn waveform_list(&self, h: &dyn HidBackend) -> Vec<(i64, u16)> {
        let Some(f) = self.desc.find(Kind::Feature, hid::usage(ORDINAL_PAGE, 3), Some(WAVEFORM_LIST)) else { return Vec::new() };
        let Ok(data) = h.get_feature(&self.node, f.report_id, self.desc.report_len(Kind::Feature, f.report_id)) else { return Vec::new() };
        (0..f.count)
            .filter_map(|k| Some(((f.usage_at(k as usize) & 0xFFFF) as i64, f.read(&data, self.desc.skip(), k)? as u16)))
            .filter(|(_, id)| *id != 0)
            .collect()
    }

    pub fn stop(&self, h: &dyn HidBackend) -> io::Result<()> {
        self.write(h, &self.report(ORDINAL_STOP, STOP_INTENSITY)?)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InkAction {
    /// start the inking waveform
    Start,
    /// stop it
    Stop,
    /// one click: the eraser end touched down
    Click,
}

/// Edge detector for the inking feedback: feed every tip state, act on the returned edge.
/// The tip end gets a start on touch-down and a stop on lift. The eraser end plays no
/// continuous waveform (on Windows neither), so its touch-down gets one click when
/// `eraser_click` is on, and nothing else.
#[derive(Default)]
pub struct InkFeedback {
    active: bool,
    /// the stroke in progress began with a start report
    started: bool,
}

impl InkFeedback {
    pub fn update(&mut self, contact: bool, rubber: bool, eraser_click: bool) -> Option<InkAction> {
        let r = match (contact, self.active) {
            (true, false) if rubber => eraser_click.then_some(InkAction::Click),
            (true, false) => {
                self.started = true;
                Some(InkAction::Start)
            }
            (false, true) => std::mem::take(&mut self.started).then_some(InkAction::Stop),
            _ => None,
        };
        self.active = contact;
        r
    }

    /// The stylus is gone or the feedback is switched off: the stop a running waveform needs.
    pub fn end(&mut self) -> Option<InkAction> {
        self.active = false;
        std::mem::take(&mut self.started).then_some(InkAction::Stop)
    }
}

/// The Simple Haptic Controller part of the pen descriptor as #1066 posted it (report 65,
/// 7 bytes), inside a Stylus application, for fakes and tests.
pub fn fake_pen_descriptor() -> Vec<u8> {
    use hid::build::*;
    cat(&[
        page(0x0D),
        usage(0x20),
        collection(1),
        page(0x0E),
        usage(0x01),
        collection(2),
        id(65),
        usage(0x24),
        lmin(1),
        lmax(255),
        size(8),
        count(1),
        output(2),
        usage(0x23),
        output(2),
        usage(0x21),
        lmax(22),
        output(2),
        usage(0x28),
        lmin(0),
        lmax(254),
        output(2),
        usage(0x25),
        lmax(2000),
        size(16),
        output(2),
        end(),
        end(),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hid::{FakeHid, HidNode};

    fn pen() -> FakeHid {
        let f = FakeHid::default();
        f.0.lock().unwrap().nodes.push(HidNode {
            name: "hidraw7".into(),
            bus: 5,
            vendor: PEN_VENDOR,
            product: PEN_PRODUCT,
            hid_name: "Surface Pen".into(),
            descriptor: fake_pen_descriptor(),
        });
        f
    }

    #[test]
    fn reports_match_windows_capture() {
        let h = pen();
        let p = find(&h).unwrap();
        // #1066: 41 00 7f 09 00 00 00 (start), 41 00 fe 02 00 00 00 (stop)
        assert_eq!(p.report(ORDINAL_INK, 0x7F).unwrap(), vec![0x41, 0, 0x7F, 9, 0, 0, 0]);
        p.stop(&h).unwrap();
        assert_eq!(h.0.lock().unwrap().writes[0].1, vec![0x41, 0, 0xFE, 2, 0, 0, 0]);
        assert!(p.report(30, 10).is_err());
    }

    /// The pen's descriptor as linux-surface#1066 posted it and the waveform list Windows read
    /// from this pen (Feature 66, docs/research/gaps/pen-ble-trace.md).
    #[test]
    fn waveform_ordinals_come_from_the_pen() {
        const DESC: &str = "05010906a1018501050719e029e715002501750195088102750895018101950615012503196d296f8100c006\
            0bff0a0001a1018530150025ff9501750809009102c00a0101a101150025ff7508953c852a09c682020109c7\
            920201953c09c8b20301170000008027ffffff7f75209504852c19cc29cf8102852d19d829db810219dc29df\
            9102852e19e429e7810219e829eb910219ec29efb102852f19f029f3810219f429f7910219f829fbb102c009\
            53a101150026ff007508950185320a01038202010a000382020195630a0203820201956585330a0104820201\
            95650a0004920201953c85340a020482020195010a0304920201c0050d0920a1018540950175201700000080\
            27ffffff7f095bb1037510150127ffff00000991b1030992b10309940998b103050e0901a102854195017508\
            150126ff000924b102092491020923b10209239102150125160920b10209219102150026fe0066011055fd35\
            0046ec0909289102751026d00746d007092591020925b1024500854275201742000d002742000d000922b102\
            0911a102050a75109514150127ffff000019032916b102c0050e0910a102050a16031026ff2f19032916b102\
            c0c0c0";
        const LIST: &str = "4242000d002d003c003c0023009100b900cb07cb07cb07cb07bc07cb07bc07\
            0000000000000000000000000000031006100710081009100a100b100c100d100e100f1010101110\
            0110011001100110011001100110";
        let unhex = |t: &str| -> Vec<u8> {
            let d: Vec<u8> = t.bytes().filter(u8::is_ascii_hexdigit).collect();
            d.chunks_exact(2).map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap()).collect()
        };
        let h = FakeHid::default();
        {
            let mut s = h.0.lock().unwrap();
            s.nodes.push(HidNode {
                name: "hidraw7".into(),
                bus: 5,
                vendor: PEN_VENDOR,
                product: PEN_PRODUCT,
                hid_name: "Surface Slim Pen 2".into(),
                descriptor: unhex(DESC),
            });
        }
        let p = find(&h).unwrap();
        assert!(p.waveform_list(&h).is_empty(), "the pen does not answer: fixed ordinals");
        assert_eq!(Waveform::Brush.ordinal(&p.waveform_list(&h)), 13);
        let list = unhex(LIST);
        assert_eq!(list.len(), 85);
        h.0.lock().unwrap().features.insert(("hidraw7".into(), 66), list.clone());
        let wl = p.waveform_list(&h);
        assert_eq!(wl.len(), 20);
        assert_eq!(Waveform::ALL.map(|w| w.ordinal(&wl)), [9, 10, 11, 12, 13, 15]);
        assert_eq!(p.report(Waveform::Brush.ordinal(&wl), 0x7F).unwrap(), vec![0x41, 0, 0x7F, 13, 0, 0, 0]);
        // a pen that lists them elsewhere: brush and ink swapped
        let mut other = list;
        other[45 + 6 * 2..45 + 6 * 2 + 2].copy_from_slice(&0x100Fu16.to_le_bytes());
        other[45 + 10 * 2..45 + 10 * 2 + 2].copy_from_slice(&0x100Bu16.to_le_bytes());
        h.0.lock().unwrap().features.insert(("hidraw7".into(), 66), other);
        let wl = p.waveform_list(&h);
        assert_eq!((Waveform::Brush.ordinal(&wl), Waveform::Ink.ordinal(&wl)), (9, 13));
    }

    #[test]
    fn waveform_names() {
        assert_eq!(Waveform::default(), Waveform::Brush);
        assert_eq!(Waveform::ALL.map(Waveform::name), ["ink", "pencil", "marker", "chisel-marker", "brush", "sparkle"]);
        assert_eq!(Waveform::ALL.map(Waveform::fixed_ordinal), [9, 10, 11, 12, 13, 15]);
        for w in Waveform::ALL {
            assert_eq!(Waveform::from_name(w.name()), Some(w));
        }
        assert_eq!(Waveform::from_name("eraser"), None);
        assert_eq!(Waveform::ChiselMarker.ordinal(&[]), 12);
    }

    #[test]
    fn ink_edges() {
        let mut i = InkFeedback::default();
        assert_eq!(i.update(false, false, true), None);
        assert_eq!(i.update(true, false, true), Some(InkAction::Start));
        assert_eq!(i.update(true, false, true), None);
        assert_eq!(i.update(false, false, true), Some(InkAction::Stop));
    }

    #[test]
    fn eraser_end_clicks_once() {
        let mut i = InkFeedback::default();
        assert_eq!(i.update(true, true, true), Some(InkAction::Click));
        assert_eq!(i.update(true, true, true), None, "nothing while rubbing");
        assert_eq!(i.update(false, true, true), None, "no stop: nothing was started");
        // switched off: the eraser end is silent
        assert_eq!(i.update(true, true, false), None);
        assert_eq!(i.update(false, true, false), None);
        // the tip end afterwards is as before
        assert_eq!(i.update(true, false, true), Some(InkAction::Start));
        assert_eq!(i.update(false, false, true), Some(InkAction::Stop));
    }

    /// Review I2/R2: a remembered pen node is not written to once the name means another device.
    #[test]
    fn a_remembered_pen_is_checked_before_writing() {
        let h = pen();
        let p = find(&h).unwrap();
        p.trigger(&h, ORDINAL_CLICK, 100).unwrap();
        h.0.lock().unwrap().nodes[0].product = 0x09B0; // the cover took hidraw7
        assert!(!p.alive(&h));
        assert_eq!(p.trigger(&h, ORDINAL_CLICK, 100).unwrap_err().kind(), io::ErrorKind::NotFound);
        assert_eq!(p.stop(&h).unwrap_err().kind(), io::ErrorKind::NotFound);
        assert_eq!(h.0.lock().unwrap().writes.len(), 1);
    }

    #[test]
    fn ending_stops_a_running_waveform_only() {
        let mut i = InkFeedback::default();
        assert_eq!(i.end(), None);
        i.update(true, false, true);
        assert_eq!(i.end(), Some(InkAction::Stop));
        assert_eq!(i.end(), None);
        assert_eq!(i.update(false, false, true), None, "the lift after it has nothing to stop");
        i.update(true, true, true);
        assert_eq!(i.end(), None, "the eraser end started nothing");
    }

    #[test]
    fn strength_is_the_intensity_byte() {
        assert_eq!(strength_to_intensity(100.0), Some(127));
        assert_eq!(strength_to_intensity(200.0), Some(254));
        assert_eq!(strength_to_intensity(50.0), Some(64));
        assert_eq!(strength_to_intensity(0.0), Some(2), "Windows' minimum");
        assert_eq!(strength_to_intensity(1000.0), Some(254), "Windows' maximum");
        assert_eq!(strength_to_intensity(f64::NAN), None);
        assert_eq!(intensity_to_strength(127), 100.0);
        assert_eq!(intensity_to_strength(254), 200.0);
        assert_eq!(strength_to_intensity(intensity_to_strength(90)), Some(90));
    }

    #[test]
    fn no_pen() {
        assert!(find(&FakeHid::default()).is_none());
    }
}
