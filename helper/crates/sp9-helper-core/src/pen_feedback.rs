// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
//! Host -> digitizer pen feedback (THC feature report 9, usage FF00:C9), as Windows' touch
//! processor sends it and as userspace/pen/surface_pen/feedback.py does.
//!
//! The pen plays its continuous haptic waveforms (ink, pencil, ...) only while the digitizer
//! has a valid pen speed from the host: the host computes the speed, hands it to the
//! digitizer in this report and the digitizer passes it on to the pen. The digitizer keeps
//! the last value it got, so a message goes out when the value changes, not every frame.
//! docs/research/gaps/pen-vendor-exchange.md section 1.
//!
//! Messages are 64 bytes with the report ID; all start with `8e <type>`.

use crate::hid::{self, HidBackend, Kind};
use std::io;
use std::path::Path;

pub const DIGITIZER_VENDOR: u16 = 0x045E;
pub const DIGITIZER_PRODUCT: u16 = 0x0C52;
pub const REPORT_ID: u8 = 9;
pub const REPORT_LEN: usize = 64;
/// FFF4:0021 on the digitizer: the FastHostId its firmware has (`fe ff`: none yet).
pub const FHID_ECHO_REPORT: u8 = 115;

const T_DISPLAY: u8 = 0xA1;
const T_PEN_V06: u8 = 0xA5;
const LOCAL_ID_NONE: u8 = 0xFF;
/// Attribute bitmap: bits 2, 3 and 6 are always set on Windows, bit 1 is contact.
const A_BASE: u8 = 0x4C;
const A_CONTACT: u8 = 0x02;
/// Zero-force-inking indication: 3 while hovering (and for the eraser end), 1 in tip contact.
const ZFI_HOVER: u8 = 3;
const ZFI_CONTACT: u8 = 1;
/// Process type: alternates between these from frame to frame.
const PROCESS: [u8; 2] = [2, 4];
pub const SPEED_INVALID: u8 = 0xFF;
pub const SPEED_MAX: u8 = 63;

/// Validity bitmap of a frame without pen ID and LocalId; `V_SPEED` when the speed is valid.
const V_BASE: u16 = 0x0352;
const V_SPEED: u16 = 0x0400;

/// Pen speed that maps to the largest value: Windows computes min(v, 30) * 63 / 30 with v in
/// 0.01 mm per ms, i.e. 63 at 300 mm/s, one step per 4.76 mm/s (a fit of its frames against
/// raw input gives 4.74, correlation 0.994; this estimator over the same raw input gives
/// Windows' values with 300).
pub const DEFAULT_FULL_SCALE: f64 = 300.0;
pub const FULL_SCALE_RANGE: (f64, f64) = (50.0, 2000.0);
/// Windows sends 0 below about this speed; the helper only when asked to (`dead_zone`).
pub const WINDOWS_DEAD_ZONE: f64 = 15.0;
pub const DEAD_ZONE_MAX: f64 = 100.0;

/// How a pen speed becomes the speed value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpeedScale {
    /// mm/s that maps to the largest value
    pub full_scale: f64,
    /// mm/s below which the value is 0 (still a valid speed); 0 = none
    pub dead_zone: f64,
}

impl SpeedScale {
    pub fn new(full_scale: f64) -> Self {
        SpeedScale { full_scale, dead_zone: 0.0 }
    }

    pub fn quantize(&self, v_mm_s: f64) -> u8 {
        if v_mm_s < self.dead_zone {
            0
        } else {
            quantize(v_mm_s, self.full_scale)
        }
    }
}

/// File in the helper's state directory; the standalone surface-pen-feedback keeps its own in
/// `LEGACY_FHID`.
pub const FHID_FILE: &str = "pen-fast-host-id";
pub const LEGACY_FHID: &str = "/var/lib/surface-pen/fast-host-id";

/// `8e a1`: display state, hinge angle and the FastHostId. The firmware echoes it in
/// feature 17 and takes the FastHostId from it.
pub fn display_states(fhid: u16, pen_id: u32) -> [u8; REPORT_LEN] {
    let mut b = [0u8; REPORT_LEN];
    b[0] = REPORT_ID;
    b[1] = 0x8E;
    b[2] = T_DISPLAY;
    b[3] = 1; // display on
    b[5..9].copy_from_slice(&0x190u32.to_le_bytes()); // hinge angle
    b[37..41].copy_from_slice(&pen_id.to_le_bytes());
    b[41..43].copy_from_slice(&fhid.to_le_bytes());
    b
}

/// What the pen does, as far as the feedback frame says it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PenState {
    /// in range, not touching
    Hover,
    /// the tip touches, moving at this speed value (0..=63)
    Tip(u8),
    /// the eraser end touches: Windows gives no speed for it, and the pen plays no continuous
    /// waveform with that end
    Eraser,
    /// the tip touches and the host cannot tell its speed (the stylus axes have no
    /// resolution): contact without a speed. Not a frame seen from Windows
    Contact,
}

/// `8e a5` (V06) as Windows' touch processor sends it for a pen it has no pen ID and LocalId
/// for (about half of its own contact frames; the pen vibrates with them). Pen ID, the
/// energy value and LocalId stay empty; `frame` picks the alternating process type.
/// docs/research/gaps/pen-vendor-exchange.md section 1.
pub fn pen_feedback(seq: u8, fhid: u16, state: PenState) -> [u8; REPORT_LEN] {
    let mut b = [0u8; REPORT_LEN];
    b[0] = REPORT_ID;
    b[1] = 0x8E;
    b[2] = T_PEN_V06;
    b[3] = seq;
    b[4] = 0x02; // pen section
    b[5] = 0x01;
    b[7] = 0x90;
    b[8] = 0x01;
    b[37] = LOCAL_ID_NONE;
    b[38] = if state == PenState::Hover { A_BASE } else { A_BASE | A_CONTACT };
    b[40..42].copy_from_slice(&fhid.to_le_bytes());
    b[42] = 1; // protocol type
    b[43] = if matches!(state, PenState::Tip(_) | PenState::Contact) { ZFI_CONTACT } else { ZFI_HOVER };
    b[44] = PROCESS[(seq & 1) as usize];
    let (speed, valid) = match state {
        PenState::Tip(q) => (q.min(SPEED_MAX), V_BASE | V_SPEED),
        _ => (SPEED_INVALID, V_BASE),
    };
    b[45] = speed;
    b[47..49].copy_from_slice(&valid.to_le_bytes());
    b
}

/// Pen speed as the 6-bit value of the report. Halves round to even, as the reference does.
pub fn quantize(v_mm_s: f64, full_scale_mm_s: f64) -> u8 {
    if v_mm_s.is_nan() || v_mm_s <= 0.0 || full_scale_mm_s.is_nan() || full_scale_mm_s <= 0.0 {
        return 0;
    }
    ((v_mm_s / full_scale_mm_s).min(1.0) * SPEED_MAX as f64).round_ties_even() as u8
}

/// Speed from stylus positions the way Windows does it: pairs of samples 14..32 ms apart,
/// smoothed over the last three estimates with weights 0.5, 0.33, 0.17.
#[derive(Default)]
pub struct SpeedEstimator {
    /// (t, x mm, y mm), newest last
    samples: Vec<(f64, f64, f64)>,
    /// mm/s, newest first
    estimates: Vec<f64>,
}

impl SpeedEstimator {
    const MIN_DT: f64 = 0.014;
    const MAX_DT: f64 = 0.032;
    const WEIGHTS: [f64; 3] = [0.5, 0.33, 0.17];

    pub fn reset(&mut self) {
        self.samples.clear();
        self.estimates.clear();
    }

    pub fn add(&mut self, t: f64, x_mm: f64, y_mm: f64) -> f64 {
        // a gap: what was estimated before it says nothing about the movement now
        if self.samples.last().is_some_and(|s| t - s.0 > Self::MAX_DT) {
            self.reset();
        }
        self.samples.push((t, x_mm, y_mm));
        self.samples.retain(|s| t - s.0 <= Self::MAX_DT);
        if let Some(&(t0, x0, y0)) = self.samples.iter().find(|s| t - s.0 >= Self::MIN_DT) {
            let v = ((x_mm - x0).powi(2) + (y_mm - y0).powi(2)).sqrt() / (t - t0);
            self.estimates.insert(0, v);
            self.estimates.truncate(Self::WEIGHTS.len());
        }
        self.value()
    }

    pub fn value(&self) -> f64 {
        if self.estimates.is_empty() {
            return 0.0;
        }
        let w = &Self::WEIGHTS[..self.estimates.len()];
        self.estimates.iter().zip(w).map(|(a, b)| a * b).sum::<f64>() / w.iter().sum::<f64>()
    }
}

/// The digitizer's hidraw node: 045E:0C52 with feature report 9.
pub fn find_digitizer(h: &dyn HidBackend) -> Option<String> {
    h.nodes().into_iter().filter(|n| n.vendor == DIGITIZER_VENDOR && n.product == DIGITIZER_PRODUCT).find_map(|n| {
        let d = hid::parse(&n.descriptor).ok()?;
        let has = d.fields_of(Kind::Feature, REPORT_ID).next().is_some();
        has.then_some(n.name)
    })
}

fn parse_fhid(text: &str) -> Option<u16> {
    let t = text.trim();
    let v = match t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        Some(h) => u16::from_str_radix(h, 16).ok()?,
        None => t.parse().ok()?,
    };
    (v != 0 && v < 0x400).then_some(v)
}

/// The 10-bit FastHostId: the one kept in `file`; else the one the standalone
/// surface-pen-feedback kept in `legacy` (taken over, so the digitizer and a cover that
/// learnt it keep seeing the same host); else `random` (1..=0x3FF, made up once as Windows
/// does). Returns the value and whether it could be kept in `file`.
pub fn load_fast_host_id(file: &Path, legacy: &Path, random: impl FnOnce() -> u16) -> (u16, io::Result<()>) {
    if let Some(v) = std::fs::read_to_string(file).ok().and_then(|t| parse_fhid(&t)) {
        return (v, Ok(()));
    }
    let v = std::fs::read_to_string(legacy).ok().and_then(|t| parse_fhid(&t)).unwrap_or_else(|| (random() % 0x3FF) + 1);
    (v, crate::sys::write_atomic(file, format!("0x{v:04x}\n").as_bytes()))
}

/// What the digitizer was told last.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Sent {
    Nothing,
    /// something was sent, but it may not have arrived or the digitizer may have lost it
    Unknown,
    Known(PenState),
}

/// What to send to the digitizer and when, fed with the stylus state. Times are seconds on
/// the clock of the input events (monotonic).
pub struct Feeder {
    pub fhid: u16,
    seq: u8,
    est: SpeedEstimator,
    sent: Sent,
    sent_at: f64,
    /// a state change was held back by `MIN_INTERVAL_S`: `wait_s` says when to come back
    held: bool,
    display_at: Option<f64>,
    in_range: bool,
    rubber: bool,
    tip: bool,
    last_pos: f64,
    /// the positions are in mm (the axes have a resolution)
    speed_usable: bool,
}

impl Feeder {
    /// resend an unchanged value this often while the pen is in range
    pub const KEEPALIVE_S: f64 = 1.0;
    /// no new position for this long while in contact: the pen stands still
    pub const STILL_S: f64 = 0.06;
    /// Windows sends about 134 messages a second
    pub const MIN_INTERVAL_S: f64 = 0.007;
    pub const DISPLAY_EVERY_S: f64 = 30.0;

    pub fn new(fhid: u16) -> Self {
        Feeder {
            fhid,
            seq: 0,
            est: SpeedEstimator::default(),
            sent: Sent::Nothing,
            sent_at: f64::NEG_INFINITY,
            held: false,
            display_at: None,
            in_range: false,
            rubber: false,
            tip: false,
            last_pos: f64::NEG_INFINITY,
            speed_usable: true,
        }
    }

    pub fn in_range(&self) -> bool {
        self.in_range
    }

    /// Whether the positions fed are in mm. Without it no speed is sent.
    pub fn set_speed_usable(&mut self, usable: bool) {
        self.speed_usable = usable;
    }

    /// The digitizer lost what it was told, or may not have got it (new node, resume, a failed
    /// write): the next `display_due` is true and the next frame is sent whatever its value,
    /// also the hover frame of a pen that is out of range.
    pub fn forget(&mut self) {
        self.display_at = None;
        if self.sent != Sent::Nothing {
            self.sent = Sent::Unknown;
        }
    }

    /// The digitizer has not been told the FastHostId since the last `forget`.
    pub fn display_pending(&self) -> bool {
        self.display_at.is_none()
    }

    /// `8e a1` is due: never sent, forgotten, or older than `DISPLAY_EVERY_S`.
    pub fn display_due(&self, now: f64) -> bool {
        self.display_at.is_none_or(|t| now - t > Self::DISPLAY_EVERY_S)
    }

    pub fn display(&mut self, now: f64) -> [u8; REPORT_LEN] {
        self.display_at = Some(now);
        display_states(self.fhid, 0)
    }

    /// BTN_TOOL_PEN / BTN_TOOL_RUBBER; `rubber` says which end is in range.
    pub fn range(&mut self, in_range: bool, rubber: bool) {
        if in_range != self.in_range || (in_range && rubber) != self.rubber {
            self.est.reset();
        }
        self.in_range = in_range;
        self.rubber = in_range && rubber;
    }

    /// BTN_TOUCH.
    pub fn tip(&mut self, down: bool) {
        self.tip = down;
    }

    /// A new position (a frame in which ABS_X or ABS_Y changed), in mm.
    pub fn position(&mut self, now: f64, x_mm: f64, y_mm: f64) {
        self.est.add(now, x_mm, y_mm);
        self.last_pos = now;
    }

    fn feedback(&mut self, now: f64, state: PenState, force: bool) -> Option<[u8; REPORT_LEN]> {
        if self.sent == Sent::Known(state) && now - self.sent_at < Self::KEEPALIVE_S {
            self.held = false;
            return None;
        }
        // (a microsecond of slack: `wait_s` hands out the exact remainder)
        if !force && now - self.sent_at < Self::MIN_INTERVAL_S - 1e-6 {
            self.held = true;
            return None;
        }
        self.held = false;
        let m = pen_feedback(self.seq, self.fhid, state);
        self.seq = self.seq.wrapping_add(1);
        self.sent = Sent::Known(state);
        self.sent_at = now;
        Some(m)
    }

    /// The message to send now, if the state changed (or the keepalive is due): at the end of
    /// an event frame, and when `wait_s` or the poll timeout ran out. The speed goes out only
    /// while the tip touches: the frame after the tip lifts and every eraser-end frame carry
    /// "no speed", so the digitizer does not keep a stale one. A tip that got no new position
    /// for `STILL_S` stands still (speed 0), whatever events keep coming. A pen that left the
    /// range gets one last hover frame.
    pub fn frame(&mut self, now: f64, scale: SpeedScale) -> Option<[u8; REPORT_LEN]> {
        if !self.in_range {
            return match self.sent {
                Sent::Nothing | Sent::Known(PenState::Hover) => {
                    self.held = false;
                    None
                }
                _ => self.feedback(now, PenState::Hover, false),
            };
        }
        let state = match (self.tip, self.rubber) {
            (false, _) => PenState::Hover,
            (true, true) => PenState::Eraser,
            (true, false) if !self.speed_usable => PenState::Contact,
            (true, false) => {
                if now - self.last_pos >= Self::STILL_S {
                    self.est.reset();
                }
                PenState::Tip(scale.quantize(self.est.value()))
            }
        };
        self.feedback(now, state, false)
    }

    /// Seconds until a state change that `MIN_INTERVAL_S` held back can go out (call `frame`
    /// again then); None when nothing is waiting.
    pub fn wait_s(&self, now: f64) -> Option<f64> {
        self.held.then(|| (Self::MIN_INTERVAL_S - (now - self.sent_at)).max(0.0))
    }

    /// Stop telling the digitizer about a contact: the hover ("no speed") frame, at once, when
    /// the last thing it was told is a contact or is not known. For when sending is switched
    /// off and when the stylus node goes away.
    pub fn release(&mut self, now: f64) -> Option<[u8; REPORT_LEN]> {
        match self.sent {
            Sent::Nothing | Sent::Known(PenState::Hover) => None,
            _ => self.feedback(now, PenState::Hover, true),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hid::{build, FakeHid, HidNode};
    use crate::sys::FakeTree;

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    fn bytes(text: &str) -> Vec<u8> {
        text.split_whitespace().map(|b| u8::from_str_radix(b, 16).unwrap()).collect()
    }

    /// Frames read out of Windows' touch processor for a pen without pen ID and LocalId
    /// (FastHostId 0x0143): hovering, and the tip in contact at speed 0.
    #[test]
    fn frames_match_windows() {
        let zeros = |n: usize| "00 ".repeat(n);
        let head = |seq: &str| format!("09 8e a5 {seq} 02 01 00 90 01 {}", zeros(8 + 20));
        let hover = bytes(&format!("{} ff 4c 00 43 01 01 03 02 ff 00 52 03 {}", head("00"), zeros(15)));
        assert_eq!(hover.len(), REPORT_LEN);
        assert_eq!(pen_feedback(0, 0x143, PenState::Hover).to_vec(), hover);
        let contact = bytes(&format!("{} ff 4e 00 43 01 01 01 04 00 00 52 07 {}", head("01"), zeros(15)));
        assert_eq!(pen_feedback(1, 0x143, PenState::Tip(0)).to_vec(), contact);
        // the process type alternates 2, 4 with the sequence
        assert_eq!([0u8, 1, 2, 255].map(|s| pen_feedback(s, 0x143, PenState::Hover)[44]), [2, 4, 2, 4]);
        // speed only for the tip; bit 0 of the attributes is never set
        let m = pen_feedback(2, 0x143, PenState::Tip(40));
        assert_eq!((m[38], m[43], m[45], &m[47..49]), (0x4E, 1, 40, &[0x52, 0x07][..]));
        assert_eq!(pen_feedback(2, 0x143, PenState::Tip(200))[45], 63);
        // eraser end in contact: contact bit, ZFI as when hovering, no speed
        let m = pen_feedback(2, 0x143, PenState::Eraser);
        assert_eq!((m[38], m[43], m[45], &m[47..49]), (0x4E, 3, 0xFF, &[0x52, 0x03][..]));
        assert!(m[9..37].iter().all(|b| *b == 0), "no pen ID, no energy value");
    }

    #[test]
    fn display_states_buffer() {
        assert_eq!(hex(&display_states(0x143, 0)), format!("098ea10100900100{}4301{}", "00".repeat(33), "00".repeat(21)));
    }

    #[test]
    fn pen_speed() {
        let mut e = SpeedEstimator::default();
        let mut v = 0.0;
        for i in 0..20 {
            v = e.add(i as f64 * 0.0077, i as f64 * 0.77, 0.0); // 100 mm/s
        }
        assert!((v - 100.0).abs() < 0.5, "{v}");
        assert_eq!(quantize(v, 860.0), 7);
        assert_eq!(quantize(5000.0, 860.0), 63);
        assert_eq!(quantize(0.0, 860.0), 0);
        assert_eq!(quantize(f64::NAN, 290.0), 0);
        assert_eq!(quantize(145.0, 290.0), 32); // 31.5 rounds to even
                                                // Windows: min(v, 30) * 63 / 30 with v in 0.01 mm per ms
        assert_eq!(DEFAULT_FULL_SCALE, 300.0);
        for (v, want) in [(300.0, 63), (1000.0, 63), (150.0, 32), (100.0, 21), (4.76, 1), (2.0, 0)] {
            assert_eq!(quantize(v, DEFAULT_FULL_SCALE), want, "{v} mm/s");
        }
        e.reset();
        assert_eq!(e.value(), 0.0);
        // one sample, or samples closer than 14 ms, give no estimate
        assert_eq!(e.add(0.0, 0.0, 0.0), 0.0);
        assert_eq!(e.add(0.005, 1.0, 0.0), 0.0);
    }

    #[test]
    fn feeder_sends_on_change_only() {
        let fs = SpeedScale::new(290.0);
        let mut f = Feeder::new(0x143);
        assert!(f.display_due(0.0));
        assert_eq!(&f.display(0.0)[..3], &[9, 0x8E, 0xA1]);
        assert!(!f.display_due(10.0) && f.display_due(31.0) && !f.display_pending());
        assert!(f.frame(0.0, fs).is_none(), "out of range, nothing sent before");
        f.range(true, false);
        // hovering at 100 mm/s: one hover frame, the speed stays out
        let mut sent = Vec::new();
        for i in 0..60 {
            let t = 1.0 + i as f64 * 0.0077;
            f.position(t, i as f64 * 0.77, 0.0);
            sent.extend(f.frame(t, fs));
        }
        assert_eq!(sent.len(), 1);
        assert_eq!((sent[0][38], sent[0][45]), (0x4C, 0xFF));
        // the tip comes down while moving: the speed is there at once, then only on change
        f.tip(true);
        let mut sent = Vec::new();
        for i in 60..130 {
            let t = 1.0 + i as f64 * 0.0077;
            f.position(t, i as f64 * 0.77, 0.0);
            sent.extend(f.frame(t, fs));
        }
        assert_eq!(sent.len(), 1, "100 mm/s all the way");
        assert_eq!((sent[0][38], sent[0][43], sent[0][45], sent[0][3]), (0x4E, 1, 22, 1), "100 / 290 * 63");
        // standing still: 0 once after STILL_S, then the keepalive
        assert!(f.frame(2.02, fs).is_none());
        assert_eq!(f.frame(2.2, fs).unwrap()[45], 0);
        assert!(f.frame(2.3, fs).is_none());
        assert_eq!(f.frame(3.3, fs).unwrap()[45], 0, "keepalive");
        // the tip lifts: a frame without a speed, so the digitizer does not keep the last one
        f.tip(false);
        let m = f.frame(3.4, fs).unwrap();
        assert_eq!((m[38], m[43], m[45], &m[47..49]), (0x4C, 3, 0xFF, &[0x52, 0x03][..]));
        assert!(f.frame(3.5, fs).is_none());
        // forgetting resends the same state
        f.forget();
        assert!(f.display_due(3.6) && f.display_pending());
        assert!(f.frame(3.6, fs).is_some());
        // leaving the range in contact: one last hover frame
        f.tip(true);
        f.position(4.0, 0.0, 0.0);
        assert!(f.frame(4.0, fs).is_some());
        f.range(false, false);
        f.tip(false);
        assert_eq!(f.frame(4.1, fs).unwrap()[45], 0xFF);
        assert!(f.frame(5.0, fs).is_none() && f.frame(9.0, fs).is_none());
    }

    /// Review B1: iptsd sends an event frame every digitizer frame (ABS_MISC timestamp), so the
    /// poll timeout never runs in range; a tip that stops must still go to speed 0.
    #[test]
    fn a_tip_that_stops_goes_to_zero_while_frames_keep_coming() {
        let fs = SpeedScale::new(300.0);
        let mut f = Feeder::new(0x143);
        f.range(true, false);
        f.tip(true);
        let mut t = 1.0;
        for i in 0..60 {
            t = 1.0 + i as f64 * 0.0077;
            f.position(t, i as f64 * 1.54, 0.0); // 200 mm/s
            f.frame(t, fs);
        }
        assert_eq!(f.sent, Sent::Known(PenState::Tip(42)));
        // 2 s of frames without a new position
        let mut last = Vec::new();
        for i in 1..260 {
            last.extend(f.frame(t + i as f64 * 0.0077, fs).map(|m| m[45]));
        }
        assert_eq!(last.first(), Some(&0), "speed 0 once the position is {} ms old", Feeder::STILL_S * 1000.0);
        assert!(last.iter().all(|q| *q == 0), "{last:?}");
        assert_eq!(f.sent, Sent::Known(PenState::Tip(0)));
    }

    /// Review B2: estimates have an age: hovering fast, stopping and touching down later does
    /// not put the old speed into the first contact frame.
    #[test]
    fn an_old_estimate_is_not_a_speed() {
        let fs = SpeedScale::new(300.0);
        let hover_fast = |f: &mut Feeder| {
            f.range(true, false);
            for i in 0..40 {
                let t = 1.0 + i as f64 * 0.0077;
                f.position(t, i as f64 * 1.54, 0.0);
                f.frame(t, fs);
            }
        };
        // touch-down without a new position
        let mut f = Feeder::new(0x143);
        hover_fast(&mut f);
        f.tip(true);
        assert_eq!(f.frame(2.3, fs).unwrap()[45], 0);
        // touch-down with a position: the gap drops what was estimated before it
        let mut f = Feeder::new(0x143);
        hover_fast(&mut f);
        f.tip(true);
        f.position(2.3, 61.0, 0.0);
        assert_eq!(f.frame(2.3, fs).unwrap()[45], 0);
        let mut e = SpeedEstimator::default();
        for i in 0..10 {
            e.add(i as f64 * 0.0077, i as f64 * 1.54, 0.0);
        }
        assert!(e.value() > 190.0);
        assert_eq!(e.add(1.0, 20.0, 0.0), 0.0, "a sample after a gap starts over");
    }

    /// Review B3: a state change inside the minimum interval is not lost: `wait_s` tells when
    /// to come back, and the lift frame goes out then.
    #[test]
    fn a_change_held_back_by_the_interval_goes_out() {
        let fs = SpeedScale::new(300.0);
        let mut f = Feeder::new(0x143);
        f.range(true, false);
        f.tip(true);
        assert!(f.frame(1.0, fs).is_some());
        assert_eq!(f.wait_s(1.0), None);
        // the lift 2 ms later (two frames in one read)
        f.tip(false);
        assert!(f.frame(1.002, fs).is_none());
        let wait = f.wait_s(1.002).unwrap();
        assert!((wait - 0.005).abs() < 1e-9, "{wait}");
        let m = f.frame(1.002 + wait, fs).unwrap();
        assert_eq!((m[38], m[45]), (0x4C, 0xFF), "the lift frame");
        assert_eq!(f.wait_s(1.01), None);
        // a change that undoes itself inside the interval leaves nothing waiting
        f.tip(true);
        assert!(f.frame(1.0075, fs).is_none() && f.wait_s(1.0075).is_some());
        f.tip(false);
        assert!(f.frame(1.008, fs).is_none());
        assert_eq!(f.wait_s(1.008), None);
    }

    /// Review R4: after a write that failed the digitizer may still hold a contact with a
    /// speed: a pen that is out of range by then gets its hover frame all the same.
    #[test]
    fn a_failed_write_is_followed_by_a_hover_frame() {
        let fs = SpeedScale::new(300.0);
        let mut f = Feeder::new(0x143);
        f.forget();
        assert!(f.frame(0.5, fs).is_none(), "nothing was ever sent: nothing to take back");
        f.range(true, false);
        f.tip(true);
        assert!(f.frame(1.0, fs).is_some());
        f.tip(false);
        let lift = f.frame(1.1, fs);
        assert!(lift.is_some());
        f.forget(); // the lift frame was not delivered
        f.range(false, false);
        let m = f.frame(1.2, fs).unwrap();
        assert_eq!((m[38], m[45]), (0x4C, 0xFF));
        assert!(f.frame(1.3, fs).is_none());
    }

    #[test]
    fn release_takes_a_contact_back_at_once() {
        let fs = SpeedScale::new(300.0);
        let mut f = Feeder::new(0x143);
        assert!(f.release(0.0).is_none());
        f.range(true, false);
        f.tip(true);
        assert!(f.frame(1.0, fs).is_some());
        let m = f.release(1.001).unwrap();
        assert_eq!((m[38], m[45], m[3]), (0x4C, 0xFF, 1), "inside the minimum interval too");
        assert!(f.release(1.002).is_none());
    }

    /// Review R7: axes without a resolution give no mm, so no speed is sent.
    #[test]
    fn no_resolution_no_speed() {
        let fs = SpeedScale::new(300.0);
        let mut f = Feeder::new(0x143);
        f.set_speed_usable(false);
        f.range(true, false);
        f.tip(true);
        f.position(1.0, 0.0, 0.0);
        f.position(1.02, 5000.0, 0.0);
        let m = f.frame(1.02, fs).unwrap();
        assert_eq!((m[38], m[43], m[45], &m[47..49]), (0x4E, 1, 0xFF, &[0x52, 0x03][..]));
    }

    #[test]
    fn dead_zone_sends_zero_below_the_threshold() {
        let off = SpeedScale::new(300.0);
        let on = SpeedScale { dead_zone: WINDOWS_DEAD_ZONE, ..off };
        assert_eq!((off.quantize(10.0), on.quantize(10.0)), (2, 0));
        assert_eq!((off.quantize(14.9), on.quantize(14.9)), (3, 0));
        assert_eq!((off.quantize(15.0), on.quantize(15.0)), (3, 3), "at the threshold the value is the usual one");
        assert_eq!((off.quantize(200.0), on.quantize(200.0)), (42, 42));
        assert_eq!(on.quantize(0.0), 0);
        // through the feeder: 10 mm/s in contact is speed 0, and still a valid speed
        let frames = |scale: SpeedScale| {
            let mut f = Feeder::new(0x143);
            f.range(true, false);
            f.tip(true);
            let mut last = None;
            for i in 0..60 {
                let t = 1.0 + i as f64 * 0.0077;
                f.position(t, i as f64 * 0.077, 0.0);
                last = f.frame(t, scale).or(last);
            }
            last.unwrap()
        };
        let (a, b) = (frames(off), frames(on));
        assert_eq!((a[45], &a[47..49]), (2, &[0x52, 0x07][..]));
        assert_eq!((b[45], &b[47..49]), (0, &[0x52, 0x07][..]));
    }

    #[test]
    fn eraser_end_never_gets_a_speed() {
        let fs = SpeedScale::new(DEFAULT_FULL_SCALE);
        let mut f = Feeder::new(0x143);
        f.range(true, true);
        f.tip(true);
        let mut sent = Vec::new();
        for i in 0..130 {
            let t = 1.0 + i as f64 * 0.0077;
            f.position(t, i as f64 * 0.77, 0.0);
            sent.extend(f.frame(t, fs));
        }
        sent.extend(f.frame(2.5, fs));
        assert_eq!(sent.len(), 2, "the contact frame and its keepalive");
        for m in &sent {
            assert_eq!((m[38], m[43], m[45], &m[47..49]), (0x4E, 3, 0xFF, &[0x52, 0x03][..]));
        }
        // the tip end afterwards gets its speed again
        f.range(false, false);
        f.range(true, false);
        assert!(matches!(f.frame(3.0, fs), Some(m) if m[45] == 0));
    }

    #[test]
    fn digitizer_node() {
        use build::*;
        let desc =
            cat(&[page(0xFF00), usage(0x01), collection(1), id(9), usage(0xC9), lmin(0), lmax(255), size(8), count(63), feature(2), end()]);
        let h = FakeHid::default();
        assert!(find_digitizer(&h).is_none());
        let node = |name: &str, product, descriptor: Vec<u8>| HidNode {
            name: name.into(),
            bus: 1,
            vendor: DIGITIZER_VENDOR,
            product,
            hid_name: "Intel Touch Host Controller".into(),
            descriptor,
        };
        h.0.lock().unwrap().nodes.push(node("hidraw5", 0x09B0, desc.clone()));
        h.0.lock().unwrap().nodes.push(node("hidraw7", DIGITIZER_PRODUCT, desc));
        assert_eq!(find_digitizer(&h).as_deref(), Some("hidraw7"));
        h.set_feature("hidraw7", &display_states(0x143, 0)).unwrap();
        assert_eq!(h.0.lock().unwrap().feature_writes[0].1.len(), REPORT_LEN);
    }

    #[test]
    fn fast_host_id_is_kept_and_taken_over() {
        let t = FakeTree::new("fhid");
        let file = t.root.p("/var/lib/sp9/pen-fast-host-id");
        let legacy = t.root.p(LEGACY_FHID);
        // nothing yet: made up, kept
        let (v, kept) = load_fast_host_id(&file, &legacy, || 0x3FF + 0x2A);
        assert_eq!(v, 0x2B);
        kept.unwrap();
        assert_eq!(load_fast_host_id(&file, &legacy, || panic!("kept")).0, 0x2B);
        // the standalone daemon's value is taken over when the helper has none
        std::fs::remove_file(&file).unwrap();
        t.file(LEGACY_FHID, "0x0143\n");
        assert_eq!(load_fast_host_id(&file, &legacy, || panic!("legacy")).0, 0x143);
        assert_eq!(t.read("/var/lib/sp9/pen-fast-host-id"), "0x0143");
        // an invalid file is replaced
        t.file("/var/lib/sp9/pen-fast-host-id", "0x0000\n");
        t.file(LEGACY_FHID, "junk\n");
        assert_eq!(load_fast_host_id(&file, &legacy, || 7).0, 8);
        assert_eq!(parse_fhid("323"), Some(323));
        assert_eq!(parse_fhid("0x400"), None);
    }
}
