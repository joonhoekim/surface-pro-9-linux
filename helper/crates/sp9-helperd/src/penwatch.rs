// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
//! Pen haptics from iptsd's virtual stylus ("IPTSD Virtual Stylus"), on its own thread while
//! `pen.ink_feedback` is on:
//!
//! - inking haptics: one start report to the pen on tip down (the waveform of
//!   `pen.ink_waveform`, its ordinal from the pen's waveform list) and one stop report on tip
//!   up, as Windows applications do. The eraser end plays no continuous waveform (on Windows
//!   neither): with `pen.eraser_click` its touch-down gets one click, nothing else;
//! - with `pen.speed_feedback`, the pen state to the digitizer (`pen_feedback::Feeder`):
//!   `8e a1` with the FastHostId when the digitizer node appears, after resume (and once
//!   more a few seconds later, as Windows does after the display comes back) and when the
//!   pen comes into range after 30 s; `8e a5` whenever the state changes: hovering, tip
//!   contact with the quantised speed, eraser contact (no speed). The pen plays the
//!   continuous waveforms only while the digitizer has a speed.
//!
//! The standalone surface-pen-feedback (userspace/pen) writes the same report; only one of
//! the two may run.

use crate::Shared;
use std::sync::Arc;

#[cfg(target_os = "linux")]
pub fn spawn(shared: Arc<Shared>) {
    std::thread::spawn(move || linux::run(shared));
}

#[cfg(not(target_os = "linux"))]
pub fn spawn(_shared: Arc<Shared>) {}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use crate::clock;
    use sp9_helper_core::config;
    use sp9_helper_core::input::SYS_INPUT;
    use sp9_helper_core::pen::{self, InkAction, InkFeedback, ORDINAL_CLICK};
    use sp9_helper_core::pen_feedback::{self as fb, Feeder};
    use std::io::Read;
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;
    use std::time::Duration;

    const STYLUS_NAME: &str = "IPTSD Virtual Stylus";
    const EV_SYN: u16 = 0x00;
    const EV_KEY: u16 = 0x01;
    const EV_ABS: u16 = 0x03;
    const ABS_X: u16 = 0x00;
    const ABS_Y: u16 = 0x01;
    const BTN_TOOL_PEN: u16 = 0x140;
    const BTN_TOOL_RUBBER: u16 = 0x141;
    const BTN_TOUCH: u16 = 0x14a;
    /// struct input_event on 64-bit: timeval (16) + type (2) + code (2) + value (4)
    const EVENT_SIZE: usize = 24;
    /// how often the settings are read again while events flow
    const CONFIG_EVERY_S: f64 = 0.25;
    /// how often a missing digitizer node is looked for again
    const DIGITIZER_RETRY_S: f64 = 2.0;
    /// Windows repeats `8e a1` this long after the display came back
    const DISPLAY_AGAIN_S: f64 = 3.8;

    fn stylus_event_node(shared: &Shared) -> Option<String> {
        shared
            .root
            .list(SYS_INPUT)
            .into_iter()
            .filter(|n| n.starts_with("event"))
            .find(|n| shared.root.read_opt(&format!("{SYS_INPUT}/{n}/device/name")).is_some_and(|name| name.starts_with(STYLUS_NAME)))
    }

    /// Units per mm of an absolute axis (EVIOCGABS: struct input_absinfo, resolution last).
    fn abs_resolution(f: &std::fs::File, axis: u16) -> f64 {
        let mut info = [0i32; 6];
        // EVIOCGABS(axis) = _IOR('E', 0x40 + axis, struct input_absinfo)
        let req = 0x8018_4540u32 + axis as u32;
        // SAFETY: info is the 24 bytes the request encodes.
        let r = unsafe { libc::ioctl(f.as_raw_fd(), req as _, info.as_mut_ptr()) };
        if r < 0 || info[5] <= 0 {
            1.0
        } else {
            info[5] as f64
        }
    }

    fn readable(f: &std::fs::File, ms: i32) -> std::io::Result<bool> {
        let mut p = libc::pollfd { fd: f.as_raw_fd(), events: libc::POLLIN, revents: 0 };
        // SAFETY: one valid pollfd.
        let r = unsafe { libc::poll(&mut p, 1, ms) };
        if r < 0 {
            let e = std::io::Error::last_os_error();
            return if e.kind() == std::io::ErrorKind::Interrupted { Ok(false) } else { Err(e) };
        }
        if p.revents & (libc::POLLNVAL | libc::POLLERR | libc::POLLHUP) != 0 {
            return Err(std::io::Error::other("stylus node gone"));
        }
        Ok(r > 0)
    }

    fn random_u16() -> u16 {
        let mut b = [0u8; 2];
        let ok = std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut b)).is_ok();
        if ok {
            u16::from_le_bytes(b)
        } else {
            std::process::id() as u16 ^ clock::monotonic().to_bits() as u16
        }
    }

    /// The digitizer side: the node, the FastHostId and what was sent last.
    struct Digitizer {
        feeder: Option<Feeder>,
        node: Option<String>,
        looked_at: f64,
        resume_mark: f64,
        /// when to send `8e a1` once more after a resume
        display_again: Option<f64>,
    }

    impl Digitizer {
        fn new() -> Self {
            Digitizer { feeder: None, node: None, looked_at: f64::NEG_INFINITY, resume_mark: clock::suspended(), display_again: None }
        }

        fn feeder(&mut self, shared: &Shared) -> &mut Feeder {
            self.feeder.get_or_insert_with(|| {
                let (fhid, kept) =
                    fb::load_fast_host_id(&shared.state_dir.join(fb::FHID_FILE), &shared.root.p(fb::LEGACY_FHID), random_u16);
                if let Err(e) = kept {
                    eprintln!("sp9-helperd: pen feedback: cannot keep the FastHostId in {}: {e}", shared.state_dir.display());
                }
                eprintln!("sp9-helperd: pen feedback: FastHostId 0x{fhid:04x}");
                Feeder::new(fhid)
            })
        }

        /// The node to write to; a node that appears (again) is told the FastHostId first.
        fn node(&mut self, shared: &Shared, now: f64) -> Option<String> {
            if clock::resumed(&mut self.resume_mark) {
                self.feeder(shared).forget();
                self.display_again = Some(now + DISPLAY_AGAIN_S);
            } else if self.display_again.is_some_and(|t| now >= t) {
                self.display_again = None;
                self.feeder(shared).forget();
            }
            if self.node.is_none() && now - self.looked_at >= DIGITIZER_RETRY_S {
                self.looked_at = now;
                self.node = fb::find_digitizer(&*shared.b.hid);
                if let Some(n) = &self.node {
                    eprintln!("sp9-helperd: pen feedback: digitizer {n}");
                    self.feeder(shared).forget();
                }
            }
            self.node.clone()
        }

        fn write(&mut self, shared: &Shared, data: &[u8]) -> bool {
            let Some(n) = &self.node else { return false };
            match shared.b.hid.set_feature(n, data) {
                Ok(()) => true,
                Err(e) => {
                    eprintln!("sp9-helperd: pen feedback: feature {} on {n}: {e}", fb::REPORT_ID);
                    self.node = None;
                    false
                }
            }
        }

        /// `8e a1` when it is due (start, new node, resume, every 30 s when the pen arrives).
        fn display(&mut self, shared: &Shared, now: f64) {
            if self.node(shared, now).is_none() || !self.feeder(shared).display_due(now) {
                return;
            }
            let (m, fhid) = {
                let f = self.feeder(shared);
                (f.display(now), f.fhid)
            };
            if !self.write(shared, &m) {
                self.feeder(shared).forget();
                return;
            }
            let Some(n) = &self.node else { return };
            if let Ok(got) = shared.b.hid.get_feature(n, fb::FHID_ECHO_REPORT, 3) {
                if got.get(1..3) != Some(&fhid.to_le_bytes()[..]) {
                    eprintln!(
                        "sp9-helperd: pen feedback: the digitizer reports another FastHostId ({:02x?})",
                        got.get(1..3).unwrap_or(&[])
                    );
                }
            }
        }

        fn send(&mut self, shared: &Shared, now: f64, msg: Option<[u8; fb::REPORT_LEN]>) {
            let Some(m) = msg else { return };
            if self.node(shared, now).is_none() || !self.write(shared, &m) {
                // not delivered: send the same value again once the node is back
                self.feeder(shared).forget();
            }
        }
    }

    /// The waveform list of the pen node it was read from (ordinal, waveform usage): read once
    /// per node, not on every stroke.
    type WaveformList = Option<(String, Vec<(i64, u16)>)>;

    fn ink_edge(shared: &Shared, cfg: &config::Pen, action: InkAction, list: &mut WaveformList) {
        // the pen's BLE node comes and goes as it sleeps: look it up on each edge
        let Some(p) = pen::find(&*shared.b.hid) else { return };
        let r = match action {
            InkAction::Start => {
                if list.as_ref().is_none_or(|(node, _)| *node != p.node) {
                    *list = Some((p.node.clone(), p.waveform_list(&*shared.b.hid)));
                }
                let ordinal = cfg.ink_waveform.ordinal(list.as_ref().map(|(_, l)| l.as_slice()).unwrap_or(&[]));
                p.trigger(&*shared.b.hid, ordinal, cfg.intensity)
            }
            InkAction::Click => p.trigger(&*shared.b.hid, ORDINAL_CLICK, cfg.intensity),
            InkAction::Stop => p.stop(&*shared.b.hid),
        };
        if let Err(e) = r {
            eprintln!("sp9-helperd: pen haptics: {e}");
        }
    }

    pub fn run(shared: Arc<Shared>) {
        let mut dig = Digitizer::new();
        let mut waveforms: WaveformList = None;
        loop {
            let mut cfg = shared.cfg().pen;
            if !cfg.ink_feedback {
                std::thread::sleep(Duration::from_secs(1));
                continue;
            }
            if cfg.speed_feedback {
                dig.display(&shared, clock::monotonic());
            }
            let Some(ev) = stylus_event_node(&shared) else {
                std::thread::sleep(Duration::from_secs(2));
                continue;
            };
            let opened =
                std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK).open(shared.root.p(&format!("/dev/input/{ev}")));
            let Ok(mut f) = opened else {
                std::thread::sleep(Duration::from_secs(2));
                continue;
            };
            let (rx, ry) = (abs_resolution(&f, ABS_X), abs_resolution(&f, ABS_Y));
            let mut ink = InkFeedback::default();
            let (mut x, mut y, mut moved, mut rubber) = (None::<i32>, None::<i32>, false, false);
            let mut cfg_at = clock::monotonic();
            let mut buf = [0u8; EVENT_SIZE * 64];
            loop {
                let in_range = dig.feeder.as_ref().is_some_and(Feeder::in_range);
                let ready = match readable(&f, if in_range { (Feeder::STILL_S * 1000.0) as i32 } else { 1000 }) {
                    Ok(r) => r,
                    Err(_) => break,
                };
                let now = clock::monotonic();
                if now - cfg_at >= CONFIG_EVERY_S || !ready {
                    cfg = shared.cfg().pen;
                    cfg_at = now;
                    if !cfg.ink_feedback {
                        if let Some(action) = ink.update(false, rubber, false) {
                            ink_edge(&shared, &cfg, action, &mut waveforms);
                        }
                        break;
                    }
                }
                if !ready {
                    if cfg.speed_feedback {
                        // after a resume (and once more a few seconds later) without waiting for the pen
                        if dig.node(&shared, now).is_some() && dig.feeder(&shared).display_pending() {
                            dig.display(&shared, now);
                        }
                        let m = dig.feeder(&shared).idle(now, cfg.speed_scale());
                        dig.send(&shared, now, m);
                    }
                    continue;
                }
                let n = match f.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
                    Err(_) => break,
                };
                for e in buf[..n].chunks_exact(EVENT_SIZE) {
                    let ty = u16::from_ne_bytes([e[16], e[17]]);
                    let code = u16::from_ne_bytes([e[18], e[19]]);
                    let value = i32::from_ne_bytes([e[20], e[21], e[22], e[23]]);
                    match (ty, code) {
                        (EV_ABS, ABS_X) => (x, moved) = (Some(value), true),
                        (EV_ABS, ABS_Y) => (y, moved) = (Some(value), true),
                        (EV_KEY, BTN_TOOL_PEN | BTN_TOOL_RUBBER) => {
                            rubber = code == BTN_TOOL_RUBBER && value != 0;
                            if cfg.speed_feedback {
                                dig.feeder(&shared).range(value != 0, rubber);
                                if value != 0 {
                                    dig.display(&shared, now);
                                }
                            }
                        }
                        (EV_KEY, BTN_TOUCH) => {
                            if cfg.speed_feedback {
                                dig.feeder(&shared).tip(value != 0);
                            }
                            if let Some(action) = ink.update(value != 0, rubber, cfg.eraser_click) {
                                ink_edge(&shared, &cfg, action, &mut waveforms);
                            }
                        }
                        (EV_SYN, _) if cfg.speed_feedback => {
                            let feeder = dig.feeder(&shared);
                            if let (true, Some(x), Some(y)) = (moved && feeder.in_range(), x, y) {
                                feeder.position(now, x as f64 / rx, y as f64 / ry);
                            }
                            moved = false;
                            let m = feeder.frame(now, cfg.speed_scale());
                            dig.send(&shared, now, m);
                        }
                        (EV_SYN, _) => moved = false,
                        _ => {}
                    }
                }
            }
            if let Some(feeder) = dig.feeder.as_mut() {
                feeder.range(false, false);
                feeder.tip(false);
            }
            std::thread::sleep(Duration::from_secs(1));
        }
    }
}
