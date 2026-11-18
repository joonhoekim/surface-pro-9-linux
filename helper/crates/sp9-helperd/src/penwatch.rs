// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
//! Pen haptics from iptsd's virtual stylus ("IPTSD Virtual Stylus"), on its own thread while
//! `pen.ink_feedback` is on. What to tell the pen and the digitizer is decided by
//! `stylus::Tracker` at the end of every event frame; this thread reads the events and does
//! the writes:
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
//! Leaving the stylus node (iptsd restarted, resume, haptics switched off, SIGTERM) stops a
//! running waveform and takes a contact back from the digitizer.
//!
//! Only one program may feed the digitizer. While it does, the thread holds an exclusive
//! flock on `DIGITIZER_LOCK`; another holder (a second helper, a feeder that takes the same
//! lock) keeps this one from writing. The standalone surface-pen-feedback (userspace/pen)
//! does not take the lock yet: do not run it next to the helper.

use crate::Shared;
use std::sync::Arc;

/// The lock of whoever writes pen feedback to the digitizer.
pub const DIGITIZER_LOCK: &str = "/run/lock/surface-pen-digitizer.lock";

#[cfg(target_os = "linux")]
pub fn spawn(shared: Arc<Shared>) {
    std::thread::spawn(move || linux::run(shared));
}

#[cfg(not(target_os = "linux"))]
pub fn spawn(shared: Arc<Shared>) {
    shared.pen_done.store(true, std::sync::atomic::Ordering::SeqCst);
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use crate::clock;
    use sp9_helper_core::config;
    use sp9_helper_core::hid::HidHandle;
    use sp9_helper_core::input::SYS_INPUT;
    use sp9_helper_core::pen::{self, InkAction, PenHaptic, ORDINAL_CLICK};
    use sp9_helper_core::pen_feedback::{self as fb, Feeder};
    use sp9_helper_core::stylus::{self, Keys, Out, Step, Tracker};
    use std::io::Read;
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;
    use std::sync::atomic::Ordering;
    use std::time::Duration;

    const STYLUS_NAME: &str = "IPTSD Virtual Stylus";
    const EVENT_SIZE: usize = std::mem::size_of::<libc::input_event>();
    /// poll timeout while the pen is away; also how fast a stop request is seen
    const IDLE_POLL_S: f64 = 0.25;
    /// how often the settings are read again while events flow
    const CONFIG_EVERY_S: f64 = 0.25;
    /// a missing or failing digitizer node is looked for again after this, doubling up to the
    /// maximum while it keeps failing
    const DIGITIZER_RETRY_S: f64 = 2.0;
    const DIGITIZER_RETRY_MAX_S: f64 = 60.0;
    /// Windows repeats `8e a1` this long after the display came back
    const DISPLAY_AGAIN_S: f64 = 3.8;
    /// how often the pen's node is looked at while no stroke is in progress
    const PEN_CHECK_S: f64 = 2.0;
    /// how often a digitizer lock someone else holds is tried again
    const LOCK_RETRY_S: f64 = 10.0;

    fn log(msg: impl std::fmt::Display) {
        eprintln!("sp9-helperd: pen feedback: {msg}");
    }

    fn stylus_event_node(shared: &Shared) -> Option<String> {
        shared
            .root
            .list(SYS_INPUT)
            .into_iter()
            .filter(|n| n.starts_with("event"))
            .find(|n| shared.root.read_opt(&format!("{SYS_INPUT}/{n}/device/name")).is_some_and(|name| name.starts_with(STYLUS_NAME)))
    }

    /// Units per mm of an absolute axis (EVIOCGABS: struct input_absinfo, resolution last);
    /// None when the device gives none.
    fn abs_resolution(f: &std::fs::File, axis: u16) -> Option<f64> {
        let mut info = [0i32; 6];
        // EVIOCGABS(axis) = _IOR('E', 0x40 + axis, struct input_absinfo)
        let req = 0x8018_4540u32 + axis as u32;
        // SAFETY: info is the 24 bytes the request encodes.
        let r = unsafe { libc::ioctl(f.as_raw_fd(), req as _, info.as_mut_ptr()) };
        (r >= 0 && info[5] > 0).then(|| info[5] as f64)
    }

    /// Event timestamps on CLOCK_MONOTONIC (EVIOCSCLOCKID), the clock of this thread's timers.
    fn monotonic_timestamps(f: &std::fs::File) -> bool {
        let clk: libc::c_int = libc::CLOCK_MONOTONIC;
        // EVIOCSCLOCKID = _IOW('E', 0xa0, int)
        // SAFETY: the request reads one int.
        unsafe { libc::ioctl(f.as_raw_fd(), 0x4004_45a0u32 as _, &clk) >= 0 }
    }

    /// The keys that are down now (EVIOCGKEY): what the stylus is doing when the node is
    /// opened and after the kernel dropped events.
    fn key_state(f: &std::fs::File) -> Option<Keys> {
        let mut bits = [0u8; 96]; // KEY_MAX 0x2ff
                                  // EVIOCGKEY(len) = _IOC(_IOC_READ, 'E', 0x18, len)
        let req = 0x8000_4518u32 | ((bits.len() as u32) << 16);
        // SAFETY: bits is the buffer of the length the request encodes.
        if unsafe { libc::ioctl(f.as_raw_fd(), req as _, bits.as_mut_ptr()) } < 0 {
            return None;
        }
        let down = |code: u16| bits[code as usize / 8] & (1 << (code % 8)) != 0;
        Some(Keys { pen: down(stylus::BTN_TOOL_PEN), rubber: down(stylus::BTN_TOOL_RUBBER), touch: down(stylus::BTN_TOUCH) })
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

    enum Lock {
        /// not needed now
        Released,
        Held(#[allow(dead_code)] std::fs::File),
        /// someone else feeds the digitizer; tried again at the time
        Busy(f64),
        /// the lock file cannot be made (no /run/lock): go on without it
        Unavailable,
    }

    /// The digitizer side: the open node and what keeps it honest.
    struct Digitizer {
        /// kept open: the handle stays with the device, whatever its hidraw name comes to mean
        handle: Option<Box<dyn HidHandle>>,
        /// the name last logged
        name: String,
        looked_at: f64,
        retry_s: f64,
        /// the error last logged; the same one is not logged again
        error: Option<String>,
        echo: Option<Vec<u8>>,
        resume_mark: f64,
        /// when to send `8e a1` once more after a resume
        display_again: Option<f64>,
        lock: Lock,
    }

    impl Digitizer {
        fn new() -> Self {
            Digitizer {
                handle: None,
                name: String::new(),
                looked_at: f64::NEG_INFINITY,
                retry_s: DIGITIZER_RETRY_S,
                error: None,
                echo: None,
                resume_mark: clock::suspended(),
                display_again: None,
                lock: Lock::Released,
            }
        }

        /// This thread may write to the digitizer: it holds the lock (or there is none to take).
        fn may_write(&mut self, shared: &Shared, now: f64) -> bool {
            match self.lock {
                Lock::Held(_) | Lock::Unavailable => return true,
                Lock::Busy(again) if now < again => return false,
                _ => {}
            }
            let path = shared.root.p(DIGITIZER_LOCK);
            let Ok(f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) else {
                self.lock = Lock::Unavailable;
                return true;
            };
            // SAFETY: flock on a valid fd.
            if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                if matches!(self.lock, Lock::Busy(_)) {
                    log("the digitizer is free again");
                }
                self.lock = Lock::Held(f);
                true
            } else {
                if !matches!(self.lock, Lock::Busy(_)) {
                    log(format!("another program feeds the digitizer ({} is locked): not writing", path.display()));
                }
                self.lock = Lock::Busy(now + LOCK_RETRY_S);
                false
            }
        }

        fn unlock(&mut self) {
            self.lock = Lock::Released;
        }

        fn failed(&mut self, feeder: &mut Feeder, msg: String) {
            if self.error.as_ref() != Some(&msg) {
                log(format!("{msg} (looking again in {} s)", self.retry_s));
                self.error = Some(msg);
            }
            self.handle = None;
            self.retry_s = (self.retry_s * 2.0).min(DIGITIZER_RETRY_MAX_S);
            // what was sent may not have arrived
            feeder.forget();
        }

        /// The open node; a node that appears (again) is told the FastHostId first.
        fn open(&mut self, shared: &Shared, now: f64, feeder: &mut Feeder) -> bool {
            if clock::resumed(&mut self.resume_mark) {
                // the digitizer forgot everything and usually has a new node
                self.handle = None;
                self.looked_at = f64::NEG_INFINITY;
                self.retry_s = DIGITIZER_RETRY_S;
                self.display_again = Some(now + DISPLAY_AGAIN_S);
                feeder.forget();
            } else if self.display_again.is_some_and(|t| now >= t) {
                self.display_again = None;
                feeder.forget();
            }
            if self.handle.is_none() && now - self.looked_at >= self.retry_s {
                self.looked_at = now;
                shared.b.hid.refresh();
                let Some(node) = fb::find_digitizer(&*shared.b.hid) else { return false };
                match shared.b.hid.open(&node) {
                    Ok(h) => {
                        if node != self.name || self.error.is_some() {
                            log(format!("digitizer {node}"));
                            self.name = node;
                        }
                        self.handle = Some(h);
                        feeder.forget();
                    }
                    Err(e) => self.failed(feeder, format!("{node}: {e}")),
                }
            }
            self.handle.is_some()
        }

        fn write(&mut self, feeder: &mut Feeder, data: &[u8]) -> bool {
            let Some(h) = self.handle.as_mut() else { return false };
            match h.set_feature(data) {
                Ok(()) => {
                    self.retry_s = DIGITIZER_RETRY_S;
                    self.error = None;
                    true
                }
                Err(e) => {
                    let msg = format!("feature {} on {}: {e}", fb::REPORT_ID, self.name);
                    self.failed(feeder, msg);
                    false
                }
            }
        }

        /// `8e a1` when it is due (start, new node, resume, every 30 s when the pen arrives).
        fn display(&mut self, shared: &Shared, now: f64, feeder: &mut Feeder) {
            if !self.may_write(shared, now) || !self.open(shared, now, feeder) || !feeder.display_due(now) {
                return;
            }
            let m = feeder.display(now);
            if !self.write(feeder, &m) {
                return;
            }
            // the firmware's copy of the FastHostId: unreadable means this is not the digitizer any more
            match self.handle.as_mut().map(|h| h.get_feature(fb::FHID_ECHO_REPORT, 3)) {
                Some(Ok(got)) => {
                    let echo = got.get(1..3).map(<[u8]>::to_vec);
                    if echo.as_deref() != Some(&feeder.fhid.to_le_bytes()[..]) && echo != self.echo {
                        log(format!("the digitizer reports another FastHostId ({:02x?})", echo.as_deref().unwrap_or(&[])));
                    }
                    self.echo = echo;
                }
                Some(Err(e)) => {
                    let msg = format!("feature {} on {}: {e}", fb::FHID_ECHO_REPORT, self.name);
                    self.failed(feeder, msg)
                }
                None => {}
            }
        }

        fn send(&mut self, shared: &Shared, now: f64, feeder: &mut Feeder, msg: Option<[u8; fb::REPORT_LEN]>) {
            let Some(m) = msg else { return };
            if !self.may_write(shared, now) || !self.open(shared, now, feeder) {
                // not delivered: `forget` makes the feeder say it again once it can
                feeder.forget();
                return;
            }
            self.write(feeder, &m);
        }
    }

    /// The pen side: the node found last and its waveform list, so a stroke does not scan sysfs
    /// and does not wait for a read over BLE.
    #[derive(Default)]
    struct PenOut {
        pen: Option<PenHaptic>,
        list: Vec<(i64, u16)>,
        checked_at: f64,
    }

    impl PenOut {
        /// Outside a stroke: notice the pen's node (the pen comes and goes as it sleeps) and read
        /// its waveform list then.
        fn look(&mut self, shared: &Shared, now: f64) {
            if now - self.checked_at < PEN_CHECK_S {
                return;
            }
            self.checked_at = now;
            if self.pen.as_ref().is_some_and(|p| p.alive(&*shared.b.hid)) {
                return;
            }
            self.pen = pen::find(&*shared.b.hid);
            self.list = self.pen.as_ref().map(|p| p.waveform_list(&*shared.b.hid)).unwrap_or_default();
        }

        fn act(&mut self, shared: &Shared, cfg: &config::Pen, action: InkAction) {
            let h = &*shared.b.hid;
            if !self.pen.as_ref().is_some_and(|p| p.alive(h)) {
                // the node changed since the last look: find it now; its list is read after the stroke
                h.refresh();
                self.pen = pen::find(h);
                self.list.clear();
                self.checked_at = f64::NEG_INFINITY;
            }
            let Some(p) = &self.pen else { return };
            let r = match action {
                InkAction::Start => p.trigger(h, cfg.waveform().ordinal(&self.list), cfg.intensity),
                InkAction::Click => p.trigger(h, ORDINAL_CLICK, cfg.intensity),
                InkAction::Stop => p.stop(h),
            };
            if let Err(e) = r {
                eprintln!("sp9-helperd: pen haptics: {e}");
                self.pen = None;
            }
        }
    }

    struct Io {
        dig: Digitizer,
        pen: PenOut,
    }

    impl Io {
        fn apply(&mut self, shared: &Shared, cfg: &config::Pen, tracker: &mut Tracker, now: f64, out: Out) {
            if let Some(action) = out.ink {
                self.pen.act(shared, cfg, action);
            }
            self.dig.send(shared, now, &mut tracker.feeder, out.frame);
        }
    }

    fn pause(shared: &Shared, s: f64) -> bool {
        let end = clock::monotonic() + s;
        while clock::monotonic() < end {
            if shared.stop.load(Ordering::SeqCst) {
                return false;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        !shared.stop.load(Ordering::SeqCst)
    }

    pub fn run(shared: Arc<Shared>) {
        let mut io = Io { dig: Digitizer::new(), pen: PenOut::default() };
        let mut tracker: Option<Tracker> = None;
        let mut no_resolution_logged = false;
        while pause(&shared, 0.0) {
            let mut cfg = shared.cfg().pen;
            if !cfg.ink_feedback {
                io.dig.unlock();
                if !pause(&shared, 1.0) {
                    break;
                }
                continue;
            }
            let tracker = tracker.get_or_insert_with(|| {
                let (fhid, kept) =
                    fb::load_fast_host_id(&shared.state_dir.join(fb::FHID_FILE), &shared.root.p(fb::LEGACY_FHID), random_u16);
                if let Err(e) = kept {
                    log(format!("cannot keep the FastHostId in {}: {e}", shared.state_dir.display()));
                }
                log(format!("FastHostId 0x{fhid:04x}"));
                Tracker::new(fhid)
            });
            if cfg.speed_feedback {
                io.dig.display(&shared, clock::monotonic(), &mut tracker.feeder);
            } else {
                io.dig.unlock();
            }
            let opened = stylus_event_node(&shared).and_then(|ev| {
                std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK).open(shared.root.p(&format!("/dev/input/{ev}"))).ok()
            });
            let Some(mut f) = opened else {
                if !pause(&shared, 2.0) {
                    break;
                }
                continue;
            };
            let event_time = monotonic_timestamps(&f);
            let resolution = abs_resolution(&f, stylus::ABS_X).zip(abs_resolution(&f, stylus::ABS_Y));
            if resolution.is_none() && !std::mem::replace(&mut no_resolution_logged, true) {
                log("the stylus axes have no resolution: the pen speed cannot be computed, no speed is sent");
            }
            tracker.opened(resolution);
            // the stylus may be in range or in contact already
            if let Some(keys) = key_state(&f) {
                let now = clock::monotonic();
                let out = tracker.resync(now, keys, &cfg);
                io.apply(&shared, &cfg, tracker, now, out);
            }
            let mut cfg_at = clock::monotonic();
            let mut buf = [0u8; EVENT_SIZE * 64];
            loop {
                let now = clock::monotonic();
                let mut wait = if tracker.in_range() { Feeder::STILL_S } else { IDLE_POLL_S };
                if let Some(w) = tracker.wait_s(now) {
                    wait = wait.min(w);
                }
                let ready = match readable(&f, (wait * 1000.0).ceil() as i32) {
                    Ok(r) => r,
                    Err(_) => break,
                };
                let now = clock::monotonic();
                if shared.stop.load(Ordering::SeqCst) {
                    break;
                }
                if now - cfg_at >= CONFIG_EVERY_S || !ready {
                    cfg = shared.cfg().pen;
                    cfg_at = now;
                    if !cfg.ink_feedback {
                        break;
                    }
                    if !cfg.speed_feedback {
                        io.dig.unlock();
                    }
                }
                if ready {
                    let n = match f.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => n,
                        Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted) => continue,
                        Err(_) => break,
                    };
                    for e in buf[..n].chunks_exact(EVENT_SIZE) {
                        // struct input_event: timeval (two longs), type, code, value
                        let field = |at: usize| i64::from_ne_bytes(e[at..at + 8].try_into().unwrap());
                        let t = if event_time { field(0) as f64 + field(8) as f64 * 1e-6 } else { now };
                        let ty = u16::from_ne_bytes([e[16], e[17]]);
                        let code = u16::from_ne_bytes([e[18], e[19]]);
                        let value = i32::from_ne_bytes([e[20], e[21], e[22], e[23]]);
                        let out = match tracker.event(t, ty, code, value, &cfg) {
                            Step::None => continue,
                            Step::Frame(out) => out,
                            // events were lost: what counts is the state now
                            Step::Resync => match key_state(&f) {
                                Some(keys) => tracker.resync(now, keys, &cfg),
                                None => continue,
                            },
                        };
                        io.apply(&shared, &cfg, tracker, now, out);
                    }
                    if cfg.speed_feedback && tracker.in_range() {
                        io.dig.display(&shared, now, &mut tracker.feeder);
                    }
                } else {
                    if !tracker.in_contact() {
                        io.pen.look(&shared, now);
                    }
                    if cfg.speed_feedback && tracker.feeder.display_pending() {
                        // after a resume (and once more a few seconds later) without waiting for the pen
                        io.dig.display(&shared, now, &mut tracker.feeder);
                    }
                }
                // keepalive, a frame the minimum interval held back, a setting that changed
                let frame = tracker.tick(now, &cfg);
                io.dig.send(&shared, now, &mut tracker.feeder, frame);
            }
            // the node is gone, the haptics are off or the daemon stops: leave nothing running
            let now = clock::monotonic();
            let out = tracker.release(now);
            io.apply(&shared, &cfg, tracker, now, out);
            if !pause(&shared, 1.0) {
                break;
            }
        }
        shared.pen_done.store(true, Ordering::SeqCst);
    }
}
