// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
//! Keyboard backlight key presses -> PropertiesChanged on Input, so front-ends follow the
//! firmware's level changes. With the kernel LED: poll() on brightness_hw_changed (sysfs
//! notify). With hidraw: read the cover's input reports, and on Input 213 with the key bit
//! (000C:0515) read Feature 214 again; emit when the level differs.

use crate::Shared;
use std::sync::Arc;

#[cfg(target_os = "linux")]
pub fn spawn(shared: Arc<Shared>, conn: zbus::Connection) {
    std::thread::spawn(move || linux::run(shared, conn));
}

#[cfg(not(target_os = "linux"))]
pub fn spawn(_shared: Arc<Shared>, _conn: zbus::Connection) {}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use crate::ifaces::{invalidate, Input, Snapshot, P_INPUT};
    use sp9_helper_core::input::{find_backlight, KbdBacklight, SYS_LEDS};
    use std::io::{Read, Seek};
    use std::os::fd::AsRawFd;
    use std::time::Duration;
    use zbus::object_server::SignalEmitter;

    fn emit(conn: &zbus::Connection) {
        zbus::block_on(async {
            if let Ok(em) = SignalEmitter::new(conn, P_INPUT) {
                invalidate(&em, Input::IFACE, &["KeyboardBacklight"]).await;
            }
        });
    }

    /// poll() one fd for `events`, true when ready.
    fn wait(fd: &impl AsRawFd, events: i16, ms: i32) -> std::io::Result<bool> {
        let mut p = libc::pollfd { fd: fd.as_raw_fd(), events, revents: 0 };
        // SAFETY: one valid pollfd.
        let r = unsafe { libc::poll(&mut p, 1, ms) };
        if r < 0 {
            return Err(std::io::Error::last_os_error());
        }
        if p.revents & libc::POLLNVAL != 0 {
            return Err(std::io::Error::other("fd closed"));
        }
        Ok(r > 0 && p.revents & (events | libc::POLLERR | libc::POLLHUP) != 0)
    }

    pub fn run(shared: Arc<Shared>, conn: zbus::Connection) {
        loop {
            match find_backlight(&*shared.b.leds, &*shared.b.hid) {
                Some(KbdBacklight::Led { name, .. }) => watch_led(&shared, &conn, &name),
                Some(KbdBacklight::Hid(k)) => {
                    let mut last = k.level(&*shared.b.hid);
                    let Ok(mut f) = std::fs::File::open(shared.root.p(&format!("/dev/{}", k.node))) else {
                        std::thread::sleep(Duration::from_secs(5));
                        continue;
                    };
                    let mut buf = [0u8; 64];
                    loop {
                        match wait(&f, libc::POLLIN, 5000) {
                            Ok(false) => continue,
                            Err(_) => break,
                            Ok(true) => {}
                        }
                        let Ok(n) = f.read(&mut buf) else { break };
                        if n == 0 {
                            break;
                        }
                        if !k.is_key_report(&buf[..n]) {
                            continue;
                        }
                        // the firmware steps the level itself; give it a moment
                        std::thread::sleep(Duration::from_millis(30));
                        let now = k.level(&*shared.b.hid);
                        if now != last {
                            last = now;
                            emit(&conn);
                        }
                    }
                }
                None => {}
            }
            std::thread::sleep(Duration::from_secs(3));
        }
    }

    /// Until the LED goes away or the backend changes: every hardware change (the kernel's
    /// led_classdev_notify_brightness_hw_changed) wakes poll() with POLLPRI.
    fn watch_led(shared: &Shared, conn: &zbus::Connection, name: &str) {
        let Ok(mut f) = std::fs::File::open(shared.root.p(&format!("{SYS_LEDS}/{name}/brightness_hw_changed"))) else {
            // no hardware notification: the 2 s property poll still sees `brightness`
            std::thread::sleep(Duration::from_secs(30));
            return;
        };
        let mut s = String::new();
        let _ = f.read_to_string(&mut s);
        loop {
            match wait(&f, libc::POLLPRI, 10_000) {
                Ok(true) => {
                    s.clear();
                    // The LED is gone (the cover was detached or re-enumerated): the old file
                    // reports an error for ever and poll() returns at once. Leave and look again.
                    if f.rewind().is_err() || f.read_to_string(&mut s).is_err() {
                        return;
                    }
                    emit(conn);
                }
                Ok(false) => {
                    if !shared.root.exists(&format!("{SYS_LEDS}/{name}")) {
                        return;
                    }
                }
                Err(_) => return,
            }
        }
    }
}
