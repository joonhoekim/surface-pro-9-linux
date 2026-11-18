// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
//! Cover coupling: a Slim Pen 2 put into the Type Cover's slot is bonded with this host, as
//! on Windows (docs/research/gaps/pen-vendor-exchange.md section 2.5). On its own thread
//! while `pen.auto_couple` is on or a manual request (`Pen.Couple`) is waiting.
//!
//! The thread keeps the cover's pen-bonding node open. When the node appears, after resume
//! and for a manual request it writes Feature 112 and Feature 86 (the host address); the
//! cover then announces a seated pen with Input 110, and by itself whenever a pen is seated
//! later. For a pen BlueZ has no bond for under its coupling address (or whose bond failed
//! to encrypt since, or on a manual request) it runs the sequence:
//!
//!   1. remove the device BlueZ has under the coupling address (Adapter1.RemoveDevice)
//!   2. Pairable on (Adapter1.Pairable; restored afterwards)
//!   3. 16 random bytes as the Temporary Key: MGMT Add Remote OOB Data (hash192, patch 0023)
//!   4. the same bytes to the cover (Output 111, a plain write)
//!   5. HCI LE Extended Create Connection to the coupling address on a raw HCI socket. The
//!      kernel only connects to devices its scan reported (mgmt Pair Device, an L2CAP
//!      connect and BlueZ's Device1.Connect all go through hci_connect_le_scan), and the
//!      pen's advert, directed to the host for about 1 s, was never reported by a scan here
//!   6. MGMT Pair Device (NoInputNoOutput): LE legacy OOB pairing, the pen gives LTK and IRK
//!   7. drop the Temporary Key, Trusted on, wait for BlueZ's HID-over-GATT node
//!
//! bluetoothd keeps running: it gets the connection through its ATT listener and the keys
//! through the same management events, discovers the services and arms auto-connect itself.
//! Full addresses and the key are never logged.

use crate::Shared;
use std::sync::{Arc, Condvar, Mutex};

pub const STATE_IDLE: &str = "idle";
pub const STATE_WAITING: &str = "waiting-for-pen";
pub const STATE_COUPLING: &str = "coupling";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    /// "idle", "waiting-for-pen" (a manual request waits for the cover to announce the pen),
    /// "coupling"
    pub state: String,
    /// result of the last attempt, "" none: "coupled", "coupled-no-hid", "already-coupled",
    /// "no-cover", "no-pen", "unsupported-kernel", "bluetooth-off", "failed: <why>"
    pub result: String,
    /// coupling address of the last pen the cover announced, masked; "" none
    pub address: String,
}

/// What the D-Bus object and the coupling thread share.
pub struct Couple {
    pub status: Mutex<Status>,
    request: Mutex<bool>,
    wake: Condvar,
}

impl Default for Couple {
    fn default() -> Self {
        Couple {
            status: Mutex::new(Status { state: STATE_IDLE.into(), ..Status::default() }),
            request: Mutex::new(false),
            wake: Condvar::new(),
        }
    }
}

impl Couple {
    pub fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }

    /// Ask for one coupling of the pen in the cover slot, replacing an existing bond.
    pub fn request(&self) {
        *self.request.lock().unwrap() = true;
        self.status.lock().unwrap().state = STATE_WAITING.into();
        self.wake.notify_all();
    }

    /// Wake the thread after a settings change.
    pub fn poke(&self) {
        self.wake.notify_all();
    }

    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    fn take_request(&self) -> bool {
        std::mem::take(&mut *self.request.lock().unwrap())
    }

    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    fn pending(&self) -> bool {
        *self.request.lock().unwrap()
    }

    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    fn wait(&self, d: std::time::Duration) {
        let g = self.request.lock().unwrap();
        if !*g {
            let _ = self.wake.wait_timeout(g, d);
        }
    }

    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    fn set(&self, state: &str, result: Option<String>) {
        let mut s = self.status.lock().unwrap();
        s.state = state.into();
        if let Some(r) = result {
            s.result = r;
        }
    }
}

#[cfg(target_os = "linux")]
pub fn spawn(shared: Arc<Shared>, conn: zbus::Connection) {
    std::thread::spawn(move || linux::run(shared, conn));
}

#[cfg(not(target_os = "linux"))]
pub fn spawn(_shared: Arc<Shared>, _conn: zbus::Connection) {}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use crate::clock;
    use sp9_helper_core::btmgmt::{self as bt, Event, HciEvent};
    use sp9_helper_core::cover::{self, mask_addr, Addr, CoverPen, Seen};
    use sp9_helper_core::pen;
    use std::collections::{HashMap, HashSet};
    use std::io::{self, Read, Write};
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::fs::OpenOptionsExt;
    use std::time::{Duration, Instant};
    use zbus::zvariant::{ObjectPath, OwnedValue, Value};

    const HCI_INDEX: u16 = 0;
    const ADAPTER: &str = "/org/bluez/hci0";
    /// a manual request waits this long for the cover to announce the pen
    const ANNOUNCE_WAIT: Duration = Duration::from_secs(5);
    /// the pen advertises for about 1 s after the key was written
    const CONNECT_WAIT: Duration = Duration::from_secs(3);
    const PAIR_WAIT: Duration = Duration::from_secs(30);
    const PAIR_SETTLE: Duration = Duration::from_millis(300);
    const PAIR_ATTEMPTS: u32 = 4;
    const LTK_WAIT: Duration = Duration::from_secs(2);
    const HID_WAIT: Duration = Duration::from_secs(10);
    /// a pen whose coupling failed is not tried again by itself before this
    const RETRY_AFTER: Duration = Duration::from_secs(60);

    fn log(msg: impl std::fmt::Display) {
        eprintln!("sp9-helperd: pen coupling: {msg}");
    }

    // ------------------------------------------------------------ sockets --

    #[repr(C)]
    struct SockaddrHci {
        family: libc::sa_family_t,
        dev: u16,
        channel: u16,
    }

    fn hci_socket(dev: u16, channel: u16) -> io::Result<std::fs::File> {
        // SAFETY: plain socket(2); the fd is owned by the File from here on.
        let fd = unsafe { libc::socket(bt::AF_BLUETOOTH, libc::SOCK_RAW | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK, bt::BTPROTO_HCI) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: fd is a fresh, valid descriptor.
        let f = unsafe { std::fs::File::from_raw_fd(fd) };
        let sa = SockaddrHci { family: bt::AF_BLUETOOTH as libc::sa_family_t, dev, channel };
        // SAFETY: sa is a valid sockaddr_hci of the given size.
        let r = unsafe { libc::bind(fd, &sa as *const SockaddrHci as *const libc::sockaddr, std::mem::size_of::<SockaddrHci>() as u32) };
        if r < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(f)
    }

    /// poll() for input; which of the fds are readable. An fd that went away is an error.
    fn poll_in(fds: &[&std::fs::File], timeout: Duration) -> io::Result<Vec<bool>> {
        let mut p: Vec<libc::pollfd> = fds.iter().map(|f| libc::pollfd { fd: f.as_raw_fd(), events: libc::POLLIN, revents: 0 }).collect();
        // SAFETY: p holds p.len() valid pollfds.
        let r = unsafe { libc::poll(p.as_mut_ptr(), p.len() as _, timeout.as_millis().min(i32::MAX as u128) as i32) };
        if r < 0 {
            let e = io::Error::last_os_error();
            return if e.kind() == io::ErrorKind::Interrupted { Ok(vec![false; fds.len()]) } else { Err(e) };
        }
        if p.iter().any(|x| x.revents & (libc::POLLNVAL | libc::POLLERR | libc::POLLHUP) != 0) {
            return Err(io::Error::other("device gone"));
        }
        Ok(p.iter().map(|x| x.revents & libc::POLLIN != 0).collect())
    }

    /// The management channel: commands of this thread and the events of every device that
    /// tell its key no longer works.
    struct Mgmt {
        sock: std::fs::File,
        /// devices whose encryption or pairing failed since the last look
        auth_failed: Vec<Addr>,
        /// devices that gave a long term key since the last look
        new_ltk: Vec<Addr>,
    }

    impl Mgmt {
        fn open() -> io::Result<Mgmt> {
            Ok(Mgmt { sock: hci_socket(bt::HCI_DEV_NONE, bt::HCI_CHANNEL_CONTROL)?, auth_failed: Vec::new(), new_ltk: Vec::new() })
        }

        /// Read what is queued; replies to `opcode` are returned, the rest is noted.
        fn drain(&mut self, opcode: Option<u16>) -> io::Result<Option<(u8, Vec<u8>)>> {
            let mut buf = [0u8; 1024];
            loop {
                let n = match self.sock.read(&mut buf) {
                    Ok(n) => n,
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(None),
                    Err(e) => return Err(e),
                };
                match bt::parse_event(&buf[..n]) {
                    Some((HCI_INDEX, Event::Reply { opcode: op, status, params })) if Some(op) == opcode => {
                        return Ok(Some((status, params)))
                    }
                    Some((HCI_INDEX, Event::AuthFailed { addr, .. })) => self.auth_failed.push(addr),
                    Some((HCI_INDEX, Event::NewLongTermKey { addr })) => self.new_ltk.push(addr),
                    Some((HCI_INDEX, Event::Disconnected { addr, reason: bt::DISCONN_AUTH_FAILURE, .. })) => self.auth_failed.push(addr),
                    _ => {}
                }
            }
        }

        fn command(&mut self, opcode: u16, pkt: &[u8], timeout: Duration) -> io::Result<(u8, Vec<u8>)> {
            self.sock.write_all(pkt)?;
            let end = Instant::now() + timeout;
            loop {
                if let Some(r) = self.drain(Some(opcode))? {
                    return Ok(r);
                }
                let left = end.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    return Err(io::Error::new(io::ErrorKind::TimedOut, format!("no reply to management command 0x{opcode:04x}")));
                }
                poll_in(&[&self.sock], left)?;
            }
        }

        fn info(&mut self) -> io::Result<bt::ControllerInfo> {
            let (status, params) = self.command(bt::OP_READ_INFO, &bt::read_info(HCI_INDEX), Duration::from_secs(2))?;
            if status != bt::STATUS_SUCCESS {
                return Err(io::Error::other(format!("Read Controller Information: {}", bt::status_name(status))));
            }
            bt::parse_info(&params).ok_or_else(|| io::Error::other("Read Controller Information: short reply"))
        }
    }

    /// LE Extended Create Connection on a raw HCI socket, waiting for the connection.
    fn connect(raw: &mut std::fs::File, peer: &Addr) -> Result<(), String> {
        raw.write_all(&bt::le_ext_create_conn(peer, true)).map_err(|e| format!("create connection: {e}"))?;
        let end = Instant::now() + CONNECT_WAIT;
        let mut buf = [0u8; 260];
        loop {
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                let _ = raw.write_all(&bt::le_create_conn_cancel());
                return Err("the pen did not answer the connection".into());
            }
            if !poll_in(&[raw], left).map_err(|e| e.to_string())?[0] {
                continue;
            }
            let n = match raw.read(&mut buf) {
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => continue,
                Err(e) => return Err(format!("HCI socket: {e}")),
            };
            match bt::parse_hci_event(&buf[..n]) {
                Some(HciEvent::CommandStatus { opcode: bt::OP_LE_EXT_CREATE_CONN, status }) if status != 0 => {
                    return Err(format!("create connection: HCI status 0x{status:02x}"));
                }
                Some(HciEvent::LeConnection { status, peer: p, .. }) if p == *peer || status == bt::HCI_UNKNOWN_CONN_ID => {
                    return if status == 0 { Ok(()) } else { Err(format!("connection: HCI status 0x{status:02x}")) };
                }
                _ => {}
            }
        }
    }

    // -------------------------------------------------------------- BlueZ --

    struct Bluez<'a>(&'a zbus::Connection);

    impl Bluez<'_> {
        fn device_path(addr: &Addr) -> String {
            format!("{ADAPTER}/dev_{}", cover::format_addr(addr).replace(':', "_"))
        }

        fn get(&self, path: &str, iface: &str, prop: &str) -> Option<OwnedValue> {
            let r =
                zbus::block_on(self.0.call_method(Some("org.bluez"), path, Some("org.freedesktop.DBus.Properties"), "Get", &(iface, prop)));
            r.ok()?.body().deserialize::<OwnedValue>().ok()
        }

        fn get_bool(&self, path: &str, iface: &str, prop: &str) -> Option<bool> {
            bool::try_from(self.get(path, iface, prop)?).ok()
        }

        fn set_bool(&self, path: &str, iface: &str, prop: &str, v: bool) -> Result<(), String> {
            let body = (iface, prop, Value::from(v));
            zbus::block_on(self.0.call_method(Some("org.bluez"), path, Some("org.freedesktop.DBus.Properties"), "Set", &body))
                .map(|_| ())
                .map_err(|e| format!("{prop}: {e}"))
        }

        /// BlueZ has a device under this address (bonded or not).
        fn known(&self, addr: &Addr) -> bool {
            self.get(&Self::device_path(addr), "org.bluez.Device1", "Paired").is_some()
        }

        fn bonded(&self, addr: &Addr) -> bool {
            let p = Self::device_path(addr);
            self.get_bool(&p, "org.bluez.Device1", "Paired") == Some(true)
                && self.get_bool(&p, "org.bluez.Device1", "Bonded") != Some(false)
        }

        fn remove(&self, addr: &Addr) -> Result<(), String> {
            let p = Self::device_path(addr);
            let path = ObjectPath::try_from(p.as_str()).map_err(|e| e.to_string())?;
            zbus::block_on(self.0.call_method(Some("org.bluez"), ADAPTER, Some("org.bluez.Adapter1"), "RemoveDevice", &(path,)))
                .map(|_| ())
                .map_err(|e| format!("RemoveDevice: {e}"))
        }
    }

    // ----------------------------------------------------------- sequence --

    fn random_key() -> io::Result<[u8; 16]> {
        let mut tk = [0u8; 16];
        std::fs::File::open("/dev/urandom")?.read_exact(&mut tk)?;
        if tk == [0; 16] {
            return Err(io::Error::other("no random data"));
        }
        Ok(tk)
    }

    fn wait_for(d: Duration, mut done: impl FnMut() -> bool) -> bool {
        let end = Instant::now() + d;
        loop {
            if done() {
                return true;
            }
            if Instant::now() >= end {
                return false;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }

    /// Steps 3 to 6 with the key in the kernel; the caller drops the key whatever happens.
    fn pair(cover: &CoverPen, dev: &mut std::fs::File, mgmt: &mut Mgmt, seen: &Seen, addr: &Addr, tk: &[u8; 16]) -> Result<(), String> {
        let report = cover.key_report(seen, tk)?;
        // before the key goes out: the pen's advert only lasts about a second
        let mut raw = hci_socket(HCI_INDEX, bt::HCI_CHANNEL_RAW).map_err(|e| format!("HCI socket: {e}"))?;
        let filter = bt::hci_event_filter();
        // SAFETY: filter is a struct hci_filter (16 bytes); SOL_HCI = 0, HCI_FILTER = 2.
        let r = unsafe { libc::setsockopt(raw.as_raw_fd(), 0, 2, filter.as_ptr() as *const libc::c_void, filter.len() as u32) };
        if r < 0 {
            return Err(format!("HCI filter: {}", io::Error::last_os_error()));
        }
        dev.write_all(&report).map_err(|e| format!("Output {}: {e}", cover::OUTPUT_KEY))?;
        connect(&mut raw, addr)?;
        drop(raw);
        log("connected, pairing");
        let cmd = bt::pair_device(HCI_INDEX, addr, bt::ADDR_LE_RANDOM, bt::IO_NO_INPUT_NO_OUTPUT);
        // Pair Device answers "success" at once, without pairing, while the kernel has not set up the
        // security manager channel of a connection that has just come up. The proof of a pairing with
        // bonding is the pen's long term key.
        for attempt in 0..PAIR_ATTEMPTS {
            std::thread::sleep(PAIR_SETTLE);
            mgmt.new_ltk.clear();
            let mut status = mgmt.command(bt::OP_PAIR_DEVICE, &cmd, PAIR_WAIT).map_err(|e| format!("Pair Device: {e}"))?.0;
            if status == bt::STATUS_ALREADY_PAIRED {
                // keys the kernel still has although BlueZ did not know the device
                let un = bt::unpair_device(HCI_INDEX, addr, bt::ADDR_LE_RANDOM, false);
                mgmt.command(bt::OP_UNPAIR_DEVICE, &un, Duration::from_secs(5)).map_err(|e| format!("Unpair Device: {e}"))?;
                status = mgmt.command(bt::OP_PAIR_DEVICE, &cmd, PAIR_WAIT).map_err(|e| format!("Pair Device: {e}"))?.0;
            }
            if status != bt::STATUS_SUCCESS {
                return Err(format!("Pair Device: {}", bt::status_name(status)));
            }
            let end = Instant::now() + LTK_WAIT;
            loop {
                mgmt.drain(None).map_err(|e| format!("management socket: {e}"))?;
                if mgmt.new_ltk.contains(addr) {
                    return Ok(());
                }
                let left = end.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    break;
                }
                poll_in(&[&mgmt.sock], left).map_err(|e| format!("management socket: {e}"))?;
            }
            log(format!("Pair Device returned without a key (attempt {})", attempt + 1));
        }
        Err("the pen did not give a key".into())
    }

    /// The whole sequence for a pen the cover announced; the result text for `Status`.
    fn couple(
        shared: &Shared,
        bz: &Bluez,
        cover: &CoverPen,
        dev: &mut std::fs::File,
        mgmt: &mut Mgmt,
        seen: &Seen,
        addr: &Addr,
    ) -> Result<&'static str, String> {
        let masked = mask_addr(addr);
        if bz.known(addr) {
            bz.remove(addr)?;
            log(format!("removed the device BlueZ had for {masked}"));
        }
        let pairable = bz.get_bool(ADAPTER, "org.bluez.Adapter1", "Pairable");
        if pairable == Some(false) {
            bz.set_bool(ADAPTER, "org.bluez.Adapter1", "Pairable", true)?;
        }
        let tk = random_key().map_err(|e| format!("key: {e}"))?;
        let add = bt::add_remote_oob(HCI_INDEX, addr, bt::ADDR_LE_RANDOM, &tk);
        let r = match mgmt.command(bt::OP_ADD_REMOTE_OOB_DATA, &add, Duration::from_secs(5)) {
            Ok((bt::STATUS_SUCCESS, _)) => pair(cover, dev, mgmt, seen, addr, &tk),
            Ok((bt::STATUS_INVALID_PARAMS, _)) => Err("unsupported-kernel".to_string()),
            Ok((s, _)) => Err(format!("Add Remote OOB Data: {}", bt::status_name(s))),
            Err(e) => Err(format!("Add Remote OOB Data: {e}")),
        };
        let clear = bt::add_remote_oob(HCI_INDEX, addr, bt::ADDR_LE_RANDOM, &[0; 16]);
        if let Err(e) = mgmt.command(bt::OP_ADD_REMOTE_OOB_DATA, &clear, Duration::from_secs(5)) {
            log(format!("dropping the key: {e}"));
        }
        if pairable == Some(false) {
            let _ = bz.set_bool(ADAPTER, "org.bluez.Adapter1", "Pairable", false);
        }
        r?;
        log(format!("paired with {masked}"));

        let path = Bluez::device_path(addr);
        if wait_for(Duration::from_secs(5), || bz.bonded(addr)) {
            if let Err(e) = bz.set_bool(&path, "org.bluez.Device1", "Trusted", true) {
                log(e);
            }
        } else {
            log("BlueZ does not show the bond");
        }
        if wait_for(HID_WAIT, || pen::find(&*shared.b.hid).is_some()) {
            return Ok("coupled");
        }
        if shared.cfg().pen.couple_restart_bluetooth {
            log("no HID node from BlueZ, restarting bluetooth.service (pen.couple_restart_bluetooth)");
            match std::process::Command::new("systemctl").args(["restart", "bluetooth.service"]).status() {
                Ok(s) if s.success() => {
                    if wait_for(Duration::from_secs(15), || pen::find(&*shared.b.hid).is_some()) {
                        return Ok("coupled");
                    }
                }
                Ok(s) => log(format!("systemctl: {s}")),
                Err(e) => log(format!("systemctl: {e}")),
            }
        }
        Ok("coupled-no-hid")
    }

    // ------------------------------------------------------------- thread --

    #[derive(Default)]
    struct Memory {
        /// bonds whose encryption failed since they were made (the pen has another key now)
        stale: HashSet<Addr>,
        failed_at: HashMap<Addr, Instant>,
    }

    /// Feature 112 and 86: the cover announces a seated pen at once and later ones by itself.
    fn announce(shared: &Shared, cover: &CoverPen, host: &Addr) -> Result<(), String> {
        let cap = cover.capability_report()?;
        let hostr = cover.host_report(host)?;
        shared.b.hid.set_feature(&cover.node, &cap).map_err(|e| format!("Feature {}: {e}", cover::FEATURE_CAPABILITY))?;
        shared.b.hid.set_feature(&cover.node, &hostr).map_err(|e| format!("Feature {}: {e}", cover::FEATURE_HOST))
    }

    /// One life of the cover node; returns when the node goes away, on an error (the text)
    /// or when there is nothing left to do (automatic coupling off, no request).
    fn session(shared: &Shared, conn: &zbus::Connection, cover: &CoverPen, mem: &mut Memory) -> Result<(), String> {
        let c = &shared.couple;
        let mut mgmt = Mgmt::open().map_err(|e| format!("management socket: {e}"))?;
        let info = mgmt.info().map_err(|e| e.to_string())?;
        if !info.has(bt::SETTING_POWERED) || !info.has(bt::SETTING_LE) {
            return Err("bluetooth-off".into());
        }
        let host = info.address;
        let mut dev = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(shared.root.p(&format!("/dev/{}", cover.node)))
            .map_err(|e| format!("{}: {e}", cover.node))?;
        let bz = Bluez(conn);
        let mut manual_until = c.take_request().then(|| Instant::now() + ANNOUNCE_WAIT);
        announce(shared, cover, &host)?;
        log(format!("cover {} told the host address {}", cover.node, mask_addr(&host)));
        let mut resume_mark = clock::suspended();
        let mut buf = [0u8; 64];
        loop {
            let ready = poll_in(&[&dev, &mgmt.sock], Duration::from_secs(1)).map_err(|_| String::new())?;
            if c.take_request() {
                manual_until = Some(Instant::now() + ANNOUNCE_WAIT);
                announce(shared, cover, &host)?;
            } else if clock::resumed(&mut resume_mark) {
                announce(shared, cover, &host)?;
            }
            if !shared.cfg().pen.auto_couple && manual_until.is_none() {
                return Ok(());
            }
            if ready[1] {
                mgmt.drain(None).map_err(|e| format!("management socket: {e}"))?;
            }
            for a in mgmt.auth_failed.drain(..) {
                if mem.stale.insert(a) {
                    log(format!("encryption with {} failed: its bond is replaced when that pen is seated", mask_addr(&a)));
                }
            }
            let mut seen = None;
            if ready[0] {
                loop {
                    match dev.read(&mut buf) {
                        Ok(0) => return Err(String::new()),
                        Ok(n) => seen = cover.parse_seen(&buf[..n]).or(seen),
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                        Err(_) => return Err(String::new()),
                    }
                }
            }
            if let Some(seen) = seen {
                let addr = cover::seen_address(&seen, &host);
                let manual = manual_until.take().is_some();
                c.status.lock().unwrap().address = mask_addr(&addr);
                let recent = mem.failed_at.get(&addr).is_some_and(|t| t.elapsed() < RETRY_AFTER);
                if !manual && (recent || (bz.bonded(&addr) && !mem.stale.contains(&addr))) {
                    if !recent {
                        c.set(STATE_IDLE, Some("already-coupled".into()));
                    }
                    continue;
                }
                c.set(STATE_COUPLING, None);
                log(format!("pen in the cover slot, coupling address {}{}", mask_addr(&addr), if manual { " (requested)" } else { "" }));
                let r = couple(shared, &bz, cover, &mut dev, &mut mgmt, &seen, &addr);
                // the attempt's own failures are not a stale bond
                mgmt.auth_failed.clear();
                // what the cover sent meanwhile belongs to this attempt
                while matches!(dev.read(&mut buf), Ok(n) if n > 0) {}
                let text = match r {
                    Ok(t) => {
                        mem.stale.remove(&addr);
                        mem.failed_at.remove(&addr);
                        t.to_string()
                    }
                    Err(e) => {
                        mem.failed_at.insert(addr, Instant::now());
                        if e == "unsupported-kernel" {
                            log("this kernel does not take a Temporary Key (Add Remote OOB Data: invalid parameters; kernel patch 0023)");
                            e
                        } else {
                            format!("failed: {e}")
                        }
                    }
                };
                log(format!("result: {text}"));
                c.set(STATE_IDLE, Some(text));
            } else if manual_until.is_some_and(|t| Instant::now() >= t) {
                manual_until = None;
                log("the cover announced no pen");
                c.set(STATE_IDLE, Some("no-pen".into()));
            }
        }
    }

    pub fn run(shared: Arc<Shared>, conn: zbus::Connection) {
        let c = &shared.couple;
        let mut mem = Memory::default();
        loop {
            if !shared.cfg().pen.auto_couple && !c.pending() {
                c.wait(Duration::from_secs(5));
                continue;
            }
            let Some(cover) = cover::find(&*shared.b.hid) else {
                if c.take_request() {
                    log("no cover with a pen slot");
                    c.set(STATE_IDLE, Some("no-cover".into()));
                }
                std::thread::sleep(Duration::from_secs(3));
                continue;
            };
            match session(&shared, &conn, &cover, &mut mem) {
                Ok(()) => {}
                // the node went away
                Err(e) if e.is_empty() => {}
                Err(e) => {
                    log(&e);
                    let manual = c.take_request() || c.status().state != STATE_IDLE;
                    if manual || e == "bluetooth-off" {
                        c.set(STATE_IDLE, Some(if e == "bluetooth-off" { e } else { format!("failed: {e}") }));
                    }
                    std::thread::sleep(Duration::from_secs(10));
                }
            }
            c.set(STATE_IDLE, None);
            std::thread::sleep(Duration::from_secs(1));
        }
    }
}
