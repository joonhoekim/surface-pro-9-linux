// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
//! Slim Pen 2 haptics over its BLE HID hidraw node: the Simple Haptic Controller (page 0x0E)
//! output report (65 on this pen), the same reports userspace/pen/surface_pen/haptic.py writes.
//! Windows-style inking feedback: one start report on tip down (ink-continuous, ordinal 9),
//! one stop report on tip up, nothing in between (docs/research/gaps/pen-ble-trace.md 3.3).

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

    pub fn trigger(&self, h: &dyn HidBackend, ordinal: i64, intensity: u8) -> io::Result<()> {
        h.write_output(&self.node, &self.report(ordinal, intensity.max(1) as i64)?)
    }

    pub fn stop(&self, h: &dyn HidBackend) -> io::Result<()> {
        h.write_output(&self.node, &self.report(ORDINAL_STOP, STOP_INTENSITY)?)
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
