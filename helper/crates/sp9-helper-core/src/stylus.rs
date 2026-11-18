// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
//! The stylus as iptsd's virtual device ("IPTSD Virtual Stylus") reports it, turned into what
//! the pen and the digitizer are to be told: the inking haptics (`pen::InkFeedback`) and the
//! pen feedback frames (`pen_feedback::Feeder`).
//!
//! Decisions are taken at the end of an event frame (SYN_REPORT), not at the single events:
//! iptsd emits BTN_TOUCH before BTN_TOOL_PEN and BTN_TOOL_RUBBER, so only the whole frame
//! says which end touched. The feeder always follows the stylus; the settings only decide
//! what is sent, so switching the speed feedback on or off in mid-stroke leaves nothing
//! stale. No I/O here: `sp9-helperd`'s pen thread reads the events and does the writes.

use crate::config;
use crate::pen::{InkAction, InkFeedback};
use crate::pen_feedback::{Feeder, REPORT_LEN};

pub const EV_SYN: u16 = 0x00;
pub const EV_KEY: u16 = 0x01;
pub const EV_ABS: u16 = 0x03;
pub const SYN_REPORT: u16 = 0;
pub const SYN_DROPPED: u16 = 3;
pub const ABS_X: u16 = 0x00;
pub const ABS_Y: u16 = 0x01;
pub const BTN_TOOL_PEN: u16 = 0x140;
pub const BTN_TOOL_RUBBER: u16 = 0x141;
pub const BTN_TOUCH: u16 = 0x14a;

/// The keys of the stylus that matter here.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Keys {
    pub pen: bool,
    pub rubber: bool,
    pub touch: bool,
}

/// What to do at the end of a frame.
#[derive(Debug, Default, PartialEq)]
pub struct Out {
    /// for the pen (Output 65)
    pub ink: Option<InkAction>,
    /// for the digitizer (Feature 9)
    pub frame: Option<[u8; REPORT_LEN]>,
}

/// What an event asks of the caller.
#[derive(Debug, PartialEq)]
pub enum Step {
    /// inside a frame: nothing yet
    None,
    /// the kernel dropped events (SYN_DROPPED): read the key state and call `resync`
    Resync,
    /// end of a frame
    Frame(Out),
}

pub struct Tracker {
    pub feeder: Feeder,
    ink: InkFeedback,
    /// state as of the last frame, and as the events of the frame in progress change it
    keys: Keys,
    next: Keys,
    x: Option<i32>,
    y: Option<i32>,
    moved: bool,
    /// units per mm of ABS_X and ABS_Y; None when the axes have no resolution
    resolution: Option<(f64, f64)>,
    /// the last frame decision went to the digitizer (speed feedback on)
    sending: bool,
}

impl Tracker {
    pub fn new(fhid: u16) -> Self {
        Tracker {
            feeder: Feeder::new(fhid),
            ink: InkFeedback::default(),
            keys: Keys::default(),
            next: Keys::default(),
            x: None,
            y: None,
            moved: false,
            resolution: None,
            sending: false,
        }
    }

    /// A stylus node was opened: its axis resolutions (units per mm). Without them positions
    /// are not mm and no speed is sent.
    pub fn opened(&mut self, resolution: Option<(f64, f64)>) {
        self.resolution = resolution;
        self.feeder.set_speed_usable(resolution.is_some());
        self.x = None;
        self.y = None;
        self.moved = false;
    }

    pub fn in_range(&self) -> bool {
        self.feeder.in_range()
    }

    /// The tip or the eraser end touches.
    pub fn in_contact(&self) -> bool {
        self.keys.touch
    }

    /// One input event with its timestamp (seconds, monotonic).
    pub fn event(&mut self, t: f64, ty: u16, code: u16, value: i32, cfg: &config::Pen) -> Step {
        match (ty, code) {
            (EV_ABS, ABS_X) => (self.x, self.moved) = (Some(value), true),
            (EV_ABS, ABS_Y) => (self.y, self.moved) = (Some(value), true),
            (EV_KEY, BTN_TOOL_PEN) => self.next.pen = value != 0,
            (EV_KEY, BTN_TOOL_RUBBER) => self.next.rubber = value != 0,
            (EV_KEY, BTN_TOUCH) => self.next.touch = value != 0,
            (EV_SYN, SYN_DROPPED) => return Step::Resync,
            (EV_SYN, SYN_REPORT) => return Step::Frame(self.end_of_frame(t, cfg)),
            _ => {}
        }
        Step::None
    }

    /// The key state read from the device (after opening it, after SYN_DROPPED): whatever
    /// was missed, the stylus is in this state now.
    pub fn resync(&mut self, t: f64, keys: Keys, cfg: &config::Pen) -> Out {
        self.next = keys;
        // positions around the gap say nothing about the speed
        self.moved = false;
        self.end_of_frame(t, cfg)
    }

    fn end_of_frame(&mut self, t: f64, cfg: &config::Pen) -> Out {
        self.keys = self.next;
        let k = self.keys;
        let in_range = k.pen || k.rubber;
        self.feeder.range(in_range, k.rubber);
        self.feeder.tip(k.touch);
        let ink = self.ink.update(k.touch, k.rubber, cfg.eraser_click);
        if let (true, true, Some(x), Some(y), Some((rx, ry))) = (self.moved, in_range, self.x, self.y, self.resolution) {
            self.feeder.position(t, x as f64 / rx, y as f64 / ry);
        }
        self.moved = false;
        Out { ink, frame: self.digitizer(t, cfg) }
    }

    /// The frame for the digitizer now, or, when the speed feedback was just switched off,
    /// the one frame that takes a contact back.
    fn digitizer(&mut self, t: f64, cfg: &config::Pen) -> Option<[u8; REPORT_LEN]> {
        if cfg.speed_feedback {
            self.sending = true;
            self.feeder.frame(t, cfg.speed_scale())
        } else if std::mem::take(&mut self.sending) {
            self.feeder.release(t)
        } else {
            None
        }
    }

    /// No event arrived (poll timeout, or `wait_s` ran out): keepalive, a frame that was held
    /// back, a setting that changed.
    pub fn tick(&mut self, now: f64, cfg: &config::Pen) -> Option<[u8; REPORT_LEN]> {
        self.digitizer(now, cfg)
    }

    /// How long to wait for events at most (seconds) so a held-back frame goes out in time.
    pub fn wait_s(&self, now: f64) -> Option<f64> {
        self.sending.then(|| self.feeder.wait_s(now)).flatten()
    }

    /// The stylus node is gone, the haptics were switched off or the daemon stops: stop a
    /// running waveform and take a contact back from the digitizer. The stylus counts as out
    /// of range afterwards.
    pub fn release(&mut self, now: f64) -> Out {
        self.keys = Keys::default();
        self.next = Keys::default();
        self.feeder.range(false, false);
        self.feeder.tip(false);
        self.sending = false;
        Out { ink: self.ink.end(), frame: self.feeder.release(now) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pen_feedback::{pen_feedback, PenState};

    const FHID: u16 = 0x143;

    fn cfg() -> config::Pen {
        let mut c = config::Pen::default();
        c.ink_feedback = true;
        c
    }

    fn tracker() -> Tracker {
        let mut t = Tracker::new(FHID);
        t.opened(Some((100.0, 100.0)));
        t
    }

    /// Feed events, return what the frame at their end asks for.
    fn frame(t: &mut Tracker, at: f64, events: &[(u16, u16, i32)], cfg: &config::Pen) -> Out {
        for (ty, code, value) in events {
            assert_eq!(t.event(at, *ty, *code, *value, cfg), Step::None);
        }
        match t.event(at, EV_SYN, SYN_REPORT, 0, cfg) {
            Step::Frame(o) => o,
            other => panic!("{other:?}"),
        }
    }

    fn state(m: Option<[u8; REPORT_LEN]>) -> Option<(u8, u8)> {
        m.map(|m| (m[38], m[45]))
    }

    /// Review B5: iptsd sends BTN_TOUCH before the tool keys. An eraser end that touches in
    /// the frame it comes into range is the eraser: a click, no continuous start.
    #[test]
    fn the_tool_is_decided_at_the_end_of_the_frame() {
        let c = cfg();
        let mut t = tracker();
        let o = frame(&mut t, 1.0, &[(EV_KEY, BTN_TOUCH, 1), (EV_KEY, BTN_TOOL_PEN, 0), (EV_KEY, BTN_TOOL_RUBBER, 1)], &c);
        assert_eq!(o.ink, Some(InkAction::Click));
        assert_eq!(o.frame.map(|m| m.to_vec()), Some(pen_feedback(0, FHID, PenState::Eraser).to_vec()));
        assert_eq!(frame(&mut t, 1.1, &[(EV_KEY, BTN_TOUCH, 0)], &c).ink, None, "nothing to stop");
        // and a tip in the same order
        let mut t = tracker();
        let o = frame(&mut t, 1.0, &[(EV_KEY, BTN_TOUCH, 1), (EV_KEY, BTN_TOOL_PEN, 1), (EV_KEY, BTN_TOOL_RUBBER, 0)], &c);
        assert_eq!(o.ink, Some(InkAction::Start));
        assert_eq!(state(o.frame), Some((0x4E, 0)));
    }

    /// Review B4: the feeder follows the stylus whatever the setting; switching the speed
    /// feedback off in contact takes the contact back, and switching it on finds the pen.
    #[test]
    fn toggling_the_speed_feedback_leaves_nothing_stale() {
        let on = cfg();
        let mut off = cfg();
        off.speed_feedback = false;
        let mut t = tracker();
        assert_eq!(state(frame(&mut t, 1.0, &[(EV_KEY, BTN_TOOL_PEN, 1)], &on).frame), Some((0x4C, 0xFF)));
        assert_eq!(state(frame(&mut t, 1.1, &[(EV_KEY, BTN_TOUCH, 1)], &on).frame), Some((0x4E, 0)));
        // switched off in contact: one hover frame, then silence; the inking haptics go on
        let o = frame(&mut t, 1.2, &[], &off);
        assert_eq!(state(o.frame), Some((0x4C, 0xFF)));
        assert_eq!(frame(&mut t, 1.3, &[], &off).frame, None);
        let o = frame(&mut t, 1.4, &[(EV_KEY, BTN_TOUCH, 0)], &off);
        assert_eq!((o.ink, o.frame), (Some(InkAction::Stop), None));
        // the pen leaves while off: the tracker knows, so no fast polling and no keepalive later
        assert_eq!(frame(&mut t, 1.5, &[(EV_KEY, BTN_TOOL_PEN, 0)], &off).frame, None);
        assert!(!t.in_range());
        assert_eq!(t.tick(5.0, &on), None, "back on without a pen: nothing to say");
        // a pen that came into range (and touched) while off is there when it is switched on
        assert_eq!(frame(&mut t, 6.0, &[(EV_KEY, BTN_TOOL_PEN, 1), (EV_KEY, BTN_TOUCH, 1)], &off).frame, None);
        assert_eq!(state(t.tick(6.5, &on)), Some((0x4E, 0)));
    }

    /// Review R1: the key state read from the device replaces whatever was missed.
    #[test]
    fn resync_after_dropped_events() {
        let c = cfg();
        let mut t = tracker();
        frame(&mut t, 1.0, &[(EV_KEY, BTN_TOOL_PEN, 1), (EV_KEY, BTN_TOUCH, 1)], &c);
        assert!(t.in_contact());
        // BTN_TOUCH 0 was dropped: SYN_DROPPED asks for the state
        assert_eq!(t.event(1.5, EV_SYN, SYN_DROPPED, 0, &c), Step::Resync);
        let o = t.resync(1.5, Keys { pen: true, rubber: false, touch: false }, &c);
        assert_eq!(o.ink, Some(InkAction::Stop));
        assert_eq!(state(o.frame), Some((0x4C, 0xFF)));
        assert!(!t.in_contact() && t.in_range());
        // opened in mid-stroke: the stroke is picked up
        let mut t = tracker();
        let o = t.resync(2.0, Keys { pen: true, rubber: false, touch: true }, &c);
        assert_eq!(o.ink, Some(InkAction::Start));
        assert_eq!(state(o.frame), Some((0x4E, 0)));
        // nothing changed: nothing to do
        assert_eq!(t.resync(2.1, Keys { pen: true, rubber: false, touch: true }, &c), Out::default());
    }

    /// Review R3: leaving (node gone, haptics off, the daemon stops) stops the waveform and
    /// takes the contact back.
    #[test]
    fn release_cleans_up() {
        let c = cfg();
        let mut t = tracker();
        assert_eq!(t.release(0.5), Out::default(), "nothing running");
        frame(&mut t, 1.0, &[(EV_KEY, BTN_TOOL_PEN, 1), (EV_KEY, BTN_TOUCH, 1)], &c);
        let o = t.release(1.001);
        assert_eq!(o.ink, Some(InkAction::Stop));
        assert_eq!(state(o.frame), Some((0x4C, 0xFF)), "at once, inside the minimum interval");
        assert!(!t.in_range() && !t.in_contact());
        assert_eq!(t.release(1.1), Out::default());
        // hovering only: the digitizer already has a hover frame
        frame(&mut t, 2.0, &[(EV_KEY, BTN_TOOL_PEN, 1)], &c);
        assert_eq!(t.release(2.5), Out::default());
        // the eraser end in contact: no waveform to stop, the contact is taken back
        frame(&mut t, 3.0, &[(EV_KEY, BTN_TOOL_RUBBER, 1), (EV_KEY, BTN_TOUCH, 1)], &c);
        let o = t.release(3.5);
        assert_eq!((o.ink, state(o.frame)), (None, Some((0x4C, 0xFF))));
    }

    /// Review B3 through the tracker: contact and lift 2 ms apart; the lift goes out when
    /// `wait_s` says so.
    #[test]
    fn two_frames_close_together() {
        let c = cfg();
        let mut t = tracker();
        frame(&mut t, 1.0, &[(EV_KEY, BTN_TOOL_PEN, 1)], &c);
        assert_eq!(state(frame(&mut t, 1.1, &[(EV_KEY, BTN_TOUCH, 1)], &c).frame), Some((0x4E, 0)));
        let o = frame(&mut t, 1.102, &[(EV_KEY, BTN_TOUCH, 0)], &c);
        assert_eq!((o.ink, o.frame), (Some(InkAction::Stop), None), "the pen is told at once, the digitizer is held back");
        let wait = t.wait_s(1.102).unwrap();
        assert!(wait > 0.004 && wait < 0.0051, "{wait}");
        assert_eq!(state(t.tick(1.102 + wait, &c)), Some((0x4C, 0xFF)));
        assert_eq!(t.wait_s(1.11), None);
    }

    /// Positions become mm with the axis resolution; without one no speed is sent (R7).
    #[test]
    fn positions_and_resolution() {
        let c = cfg();
        let mut t = tracker();
        frame(&mut t, 1.0, &[(EV_KEY, BTN_TOOL_PEN, 1), (EV_KEY, BTN_TOUCH, 1), (EV_ABS, ABS_X, 0), (EV_ABS, ABS_Y, 0)], &c);
        let mut last = None;
        for i in 1..40 {
            // 100 units per mm, 154 units per 7.7 ms: 200 mm/s
            let o = frame(&mut t, 1.0 + i as f64 * 0.0077, &[(EV_ABS, ABS_X, i * 154)], &c);
            last = state(o.frame).or(last);
        }
        assert_eq!(last, Some((0x4E, 42)));
        let mut t = Tracker::new(FHID);
        t.opened(None);
        let o = frame(&mut t, 1.0, &[(EV_KEY, BTN_TOOL_PEN, 1), (EV_KEY, BTN_TOUCH, 1), (EV_ABS, ABS_X, 5)], &c);
        assert_eq!(o.frame.map(|m| m.to_vec()), Some(pen_feedback(0, FHID, PenState::Contact).to_vec()));
    }
}
