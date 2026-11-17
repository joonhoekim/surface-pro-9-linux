// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
//! Inking haptics: watch the tip of iptsd's virtual stylus (BTN_TOUCH on "IPTSD Virtual
//! Stylus") and send the pen one start report on tip down and one stop report on tip up, as
//! Windows does. Runs on its own thread while `pen.ink_feedback` is on.

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
    use sp9_helper_core::input::SYS_INPUT;
    use sp9_helper_core::pen::{self, InkAction, InkFeedback, ORDINAL_INK};
    use std::io::Read;
    use std::time::Duration;

    const STYLUS_NAME: &str = "IPTSD Virtual Stylus";
    const EV_KEY: u16 = 0x01;
    const BTN_TOUCH: u16 = 0x14a;
    /// struct input_event on 64-bit: timeval (16) + type (2) + code (2) + value (4)
    const EVENT_SIZE: usize = 24;

    fn stylus_event_node(shared: &Shared) -> Option<String> {
        shared
            .root
            .list(SYS_INPUT)
            .into_iter()
            .filter(|n| n.starts_with("event"))
            .find(|n| shared.root.read_opt(&format!("{SYS_INPUT}/{n}/device/name")).is_some_and(|name| name.starts_with(STYLUS_NAME)))
    }

    pub fn run(shared: Arc<Shared>) {
        loop {
            if !shared.cfg().pen.ink_feedback {
                std::thread::sleep(Duration::from_secs(1));
                continue;
            }
            let Some(ev) = stylus_event_node(&shared) else {
                std::thread::sleep(Duration::from_secs(2));
                continue;
            };
            let Ok(mut f) = std::fs::File::open(shared.root.p(&format!("/dev/input/{ev}"))) else {
                std::thread::sleep(Duration::from_secs(2));
                continue;
            };
            let mut ink = InkFeedback::default();
            let mut buf = [0u8; EVENT_SIZE * 16];
            while let Ok(n) = f.read(&mut buf) {
                if n == 0 {
                    break;
                }
                for e in buf[..n].chunks_exact(EVENT_SIZE) {
                    let ty = u16::from_ne_bytes([e[16], e[17]]);
                    let code = u16::from_ne_bytes([e[18], e[19]]);
                    let value = i32::from_ne_bytes([e[20], e[21], e[22], e[23]]);
                    if ty != EV_KEY || code != BTN_TOUCH {
                        continue;
                    }
                    let cfg = shared.cfg();
                    if !cfg.pen.ink_feedback {
                        continue;
                    }
                    let Some(action) = ink.update(value != 0) else { continue };
                    // the pen's BLE node comes and goes as it sleeps: look it up on each edge
                    let Some(p) = pen::find(&*shared.b.hid) else { continue };
                    let r = match action {
                        InkAction::Start => p.trigger(&*shared.b.hid, ORDINAL_INK, cfg.pen.intensity),
                        InkAction::Stop => p.stop(&*shared.b.hid),
                    };
                    if let Err(e) = r {
                        eprintln!("sp9-helperd: pen haptics: {e}");
                    }
                }
            }
            std::thread::sleep(Duration::from_secs(1));
        }
    }
}
