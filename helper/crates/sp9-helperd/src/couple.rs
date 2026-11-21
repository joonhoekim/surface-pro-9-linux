// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
//! Cover coupling: a Slim Pen 2 put into the Type Cover's slot is bonded with this host, as
//! on Windows (docs/research/gaps/pen-vendor-exchange.md section 2.5). On its own thread
//! while `pen.auto_couple` is on or a manual request (`Pen.Couple`) is waiting.
//!
//! The thread keeps the cover's pen-bonding node open (a known cover only:
//! `cover::KNOWN_COVERS`). When the node appears, after resume and for a manual request it
//! writes Feature 112 and Feature 86 (the host address); the cover then announces a seated
//! pen with Input 110, and by itself whenever a pen is seated later. What happens then is
//! `cover::on_seated`:
//!
//! - BlueZ does not answer (bluetoothd is restarting): nothing is known, nothing is done;
//! - the pen's coupling address is not known (`Memory::pen_hash`): "coupling-unavailable";
//! - no bond under the pen's coupling address: the coupling (`coupling::couple`);
//! - a bond and BlueZ shows the pen connected: nothing, also for a request;
//! - a bond, not connected: nothing is assumed. That is a normal state: a healthy pen sits
//!   in the slot asleep, and after a resume it does not reconnect by itself. After 6 s (it
//!   may connect by itself) the cover is asked again and, when it still announces the pen,
//!   the pen is probed: a key goes to the cover (Output 111; the pen takes no connection
//!   without one) but not to the kernel, then LE Create Connection to the coupling address.
//!   Nothing is unpaired or paired. bluetoothd encrypts a link to a bonded device with the
//!   stored key as soon as its ATT channel is up (src/device.c device_attach_att; measured:
//!   LE Start Encryption 0.1 s after the connection, and the pen accepts its old key while
//!   it holds the new one). The key stays in the pen and is remembered as pending. A link
//!   that stays, with the services there, is a good bond ("connected"), and the sleeping pen
//!   is online again. A link the kernel drops with "authentication failure" is a dead bond:
//!   Windows coupled the pen since. No connection says nothing ("no-answer"); the bond
//!   stays, and the pen is not probed again for 60 s. After a resume nothing is done by
//!   itself; a request probes;
//! - a bond proven dead that way within the last two minutes, or a forced request: the bond
//!   is dropped and the coupling runs.
//!
//! The only proof of a dead bond is a link the kernel dropped because encryption failed
//! (Device Disconnected, reason 4). A failed pairing is none. A bond is never given up
//! because the pen is not connected or does not answer. It cannot be kept until the new key
//! is there either: with keys present Pair Device answers Already Paired, bluetoothd starts
//! encryption with the old key on the new link, and Unpair Device on a connected device
//! makes bluetoothd disconnect it (src/device.c device_set_unpaired). So the dead bond goes
//! first, with Unpair Device while the pen is not connected, and only after everything that
//! can fail without the pen has been done. Nothing here terminates a link: after a link the
//! host terminated the pen does not advertise for at least 30 s.
//!
//! The coupling itself, its order and its repeats are `coupling::couple`; this file is what
//! it acts through (`Ops`): the management socket, a raw HCI socket for the connection (the
//! kernel only connects to devices its scan reported, and the pen's advert, directed to the
//! host for about 1 s, was never reported by a scan here), the cover node and BlueZ over
//! D-Bus. bluetoothd keeps running: it gets the connection through its ATT listener and the
//! keys through the same management events, discovers the services and arms auto-connect
//! itself. Full addresses and the key are never logged.

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
    /// "connected" (a probed bond works), "no-answer" (the pen took no connection; a bond it
    /// has is kept), "stale-bond" (a probed bond is dead; the coupling follows),
    /// "coupling-unavailable" (the pen's coupling address is not known: no cover bond of
    /// the pen on this host and no coupling prefix), "no-cover", "no-pen",
    /// "unsupported-kernel", "bluetooth-off", "failed: <why>"
    pub result: String,
    /// coupling address of the last pen the cover announced, masked; "" none
    pub address: String,
}

/// What the D-Bus object and the coupling thread share.
pub struct Couple {
    pub status: Mutex<Status>,
    /// a manual request: Some(forced)
    request: Mutex<Option<bool>>,
    wake: Condvar,
}

impl Default for Couple {
    fn default() -> Self {
        Couple {
            status: Mutex::new(Status { state: STATE_IDLE.into(), ..Status::default() }),
            request: Mutex::new(None),
            wake: Condvar::new(),
        }
    }
}

impl Couple {
    pub fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }

    /// Ask for the pen in the cover slot to be looked at now: coupled when it has no bond,
    /// probed when it has one, its bond replaced when that is dead. `force` replaces the
    /// bond of a pen that is not connected without the proof. False while a coupling runs:
    /// nothing is queued behind it.
    pub fn request(&self, force: bool) -> bool {
        // the state check and the request are one step: the thread enters a coupling under
        // the same lock (`begin`)
        let mut st = self.status.lock().unwrap();
        if st.state == STATE_COUPLING {
            return false;
        }
        *self.request.lock().unwrap() = Some(force);
        st.state = STATE_WAITING.into();
        drop(st);
        self.wake.notify_all();
        true
    }

    /// Wake the thread after a settings change.
    pub fn poke(&self) {
        self.wake.notify_all();
    }

    /// The thread starts a coupling or a probe. A request stored up to now is served by it.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(crate) fn begin(&self) -> Option<bool> {
        let mut st = self.status.lock().unwrap();
        st.state = STATE_COUPLING.into();
        self.request.lock().unwrap().take()
    }

    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    fn take_request(&self) -> Option<bool> {
        self.request.lock().unwrap().take()
    }

    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    fn pending(&self) -> bool {
        self.request.lock().unwrap().is_some()
    }

    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    fn wait(&self, d: std::time::Duration) {
        let g = self.request.lock().unwrap();
        if g.is_none() {
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
    use sp9_helper_core::coupling::{self, Fail, Link, Ops, PairReply};
    use sp9_helper_core::cover::{self, mask_addr, Addr, CoverPen, PenHash, PenHashes, Seen};
    use sp9_helper_core::pen;
    use std::collections::HashMap;
    use std::io::{self, Read, Write};
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::fs::OpenOptionsExt;
    use std::time::{Duration, Instant};
    use zbus::zvariant::{OwnedValue, Value};

    /// a manual request waits this long for the cover to announce the pen
    const ANNOUNCE_WAIT: Duration = Duration::from_secs(5);
    /// the pen advertises for about 1 s after the key was written
    const CONNECT_WAIT: Duration = Duration::from_secs(3);
    /// after a cancelled Create Connection: the controller may have connected at the last moment
    const CANCEL_WAIT: Duration = Duration::from_millis(500);
    /// the controller was busy with another connection attempt
    const DISALLOWED_PAUSE: Duration = Duration::from_millis(200);
    /// longer than the kernel's own SMP timeout (30 s), so a late reply is not read as the
    /// reply to the next command
    const PAIR_WAIT: Duration = Duration::from_secs(35);
    const HID_WAIT: Duration = Duration::from_secs(10);
    /// a bonded pen that was seated is expected to be connected after this
    const CONNECT_GRACE: Duration = Duration::from_secs(6);
    /// the cover answers a new Feature 86 at once when the pen is still seated
    const CONFIRM_WAIT: Duration = Duration::from_secs(3);
    /// a pen coupled this recently is not coupled again by itself
    const RECOUPLE_AFTER: Duration = Duration::from_secs(60);
    /// the pen does not answer a second coupling right after one
    const MANUAL_AFTER: Duration = Duration::from_secs(15);
    /// a bonded pen that took no connection is not probed again by itself before this
    const PROBE_AFTER: Duration = Duration::from_secs(60);
    /// a probed link that is still up after this was encrypted with the stored key
    const PROBE_SETTLE: Duration = Duration::from_secs(2);
    const PROBE_WAIT: Duration = Duration::from_secs(10);
    /// how long a failed encryption counts as proof that a bond is dead
    const DEAD_FRESH: Duration = Duration::from_secs(120);
    const DBUS_TIMEOUT: Duration = Duration::from_secs(3);
    const HCI_COMMAND_DISALLOWED: u8 = 0x0C;
    const HCI_ACL_EXISTS: u8 = 0x0B;

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

    /// The management channel of one adapter: commands of this thread, and what the events
    /// say about the devices (`coupling::Events`).
    struct Mgmt {
        sock: std::fs::File,
        /// the adapter (the lowest controller index there is)
        index: u16,
        ev: coupling::Events,
        /// the pens coupled or probed through this socket: a pairing with one of them that asks
        /// for a confirmation or a passkey is not using the key and is refused at once
        /// (bluetoothd would ask an agent, or accept by itself)
        guard: coupling::Guard,
    }

    impl Mgmt {
        fn open() -> Result<Mgmt, String> {
            let sock = hci_socket(bt::HCI_DEV_NONE, bt::HCI_CHANNEL_CONTROL).map_err(|e| format!("management socket: {e}"))?;
            let mut m = Mgmt { sock, index: bt::HCI_DEV_NONE, ev: coupling::Events::default(), guard: coupling::Guard::default() };
            let (status, params) = m
                .command(bt::OP_READ_INDEX_LIST, &bt::read_index_list(), Duration::from_secs(2))
                .map_err(|e| format!("management socket: {e}"))?;
            if status != bt::STATUS_SUCCESS {
                return Err(format!("Read Controller Index List: {}", bt::status_name(status)));
            }
            m.index = bt::parse_index_list(&params).into_iter().min().ok_or("no Bluetooth adapter")?;
            Ok(m)
        }

        /// Read what is queued; the reply to `opcode` is returned, the events are noted.
        fn drain(&mut self, opcode: Option<u16>) -> io::Result<Option<(u8, Vec<u8>)>> {
            let mut buf = [0u8; 1024];
            loop {
                let n = match self.sock.read(&mut buf) {
                    Ok(n) => n,
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(None),
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                    Err(e) => return Err(e),
                };
                match bt::parse_event(&buf[..n]) {
                    Some((_, Event::Reply { opcode: op, status, params })) if Some(op) == opcode => return Ok(Some((status, params))),
                    Some((idx, ev)) if idx == self.index => self.ev.note(ev),
                    _ => {}
                }
                for r in self.guard.refusals(&mut self.ev) {
                    log("a pairing with the pen that does not use the key (no OOB flag): refused");
                    let pkt = match r {
                        coupling::Refusal::Confirm(a, t) => bt::user_confirm_neg_reply(self.index, &a, t),
                        coupling::Refusal::Passkey(a, t) => bt::user_passkey_neg_reply(self.index, &a, t),
                    };
                    let _ = self.sock.write_all(&pkt);
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

        fn info(&mut self) -> Result<bt::ControllerInfo, String> {
            let (status, params) = self
                .command(bt::OP_READ_INFO, &bt::read_info(self.index), Duration::from_secs(2))
                .map_err(|e| format!("management socket: {e}"))?;
            if status != bt::STATUS_SUCCESS {
                return Err(format!("Read Controller Information: {}", bt::status_name(status)));
            }
            bt::parse_info(&params).ok_or_else(|| "Read Controller Information: short reply".to_string())
        }

        /// Forget what the events said about one device (before watching it).
        fn forget(&mut self, addr: &Addr) {
            let _ = self.drain(None);
            for v in [&mut self.ev.ltk, &mut self.ev.dead, &mut self.ev.pairing_failed, &mut self.ev.disconnected] {
                v.retain(|a| a != addr);
            }
        }
    }

    /// The raw HCI socket of the adapter, hearing command status and LE meta events only.
    fn open_raw(index: u16) -> Result<std::fs::File, String> {
        let raw = hci_socket(index, bt::HCI_CHANNEL_RAW).map_err(|e| format!("HCI socket: {e}"))?;
        let filter = bt::hci_event_filter();
        // SAFETY: filter is a struct hci_filter (16 bytes); SOL_HCI = 0, HCI_FILTER = 2.
        let r = unsafe { libc::setsockopt(raw.as_raw_fd(), 0, 2, filter.as_ptr() as *const libc::c_void, filter.len() as u32) };
        if r < 0 {
            return Err(format!("HCI filter: {}", io::Error::last_os_error()));
        }
        Ok(raw)
    }

    /// The next event of the raw socket within `wait`.
    fn raw_event(raw: &mut std::fs::File, wait: Duration) -> Result<Option<HciEvent>, String> {
        let end = Instant::now() + wait;
        let mut buf = [0u8; 260];
        loop {
            match raw.read(&mut buf) {
                Ok(n) => {
                    if let Some(ev) = bt::parse_hci_event(&buf[..n]) {
                        return Ok(Some(ev));
                    }
                }
                Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted) => {
                    let left = end.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        return Ok(None);
                    }
                    poll_in(&[raw], left).map_err(|e| format!("HCI socket: {e}"))?;
                }
                Err(e) => return Err(format!("HCI socket: {e}")),
            }
        }
    }

    /// LE Set Address Resolution Enable on the raw socket; whether the controller took it. It
    /// refuses while it scans, advertises or connects.
    fn set_resolution(raw: &mut std::fs::File, enable: bool) -> bool {
        if raw.write_all(&bt::le_set_addr_resolution(enable)).is_err() {
            return false;
        }
        let end = Instant::now() + Duration::from_millis(500);
        loop {
            match raw_event(raw, end.saturating_duration_since(Instant::now())) {
                Ok(Some(HciEvent::CommandStatus { opcode: bt::OP_LE_SET_ADDR_RESOLUTION, status })) => return status == 0,
                Ok(Some(_)) => {}
                _ => return false,
            }
        }
    }

    /// LE Extended Create Connection on the raw HCI socket, waiting for the connection. A
    /// link that is up already counts as connected.
    ///
    /// `unresolved` (a coupling): the controller's address resolution is switched off for the
    /// attempt. Its resolving list can still hold the pen from an earlier bond (the kernel
    /// does not take entries out when it stops scanning), and a pen that still believes in
    /// that bond advertises from a private address the controller then resolves. On a link
    /// made that way the kernel computes the pairing's confirm value with the identity
    /// address, the pen with the private one: no pairing can succeed. With resolution off
    /// only the pen's directed advert from the static coupling address matches. The kernel
    /// switches resolution back on whenever it next programs its lists; it is switched back
    /// on here too.
    fn connect(raw: &mut std::fs::File, peer: &Addr, unresolved: bool) -> Result<Link, String> {
        // what queued up since the socket was opened is not about this connection
        let mut buf = [0u8; 260];
        while matches!(raw.read(&mut buf), Ok(n) if n > 0) {}
        let off = unresolved && set_resolution(raw, false);
        if unresolved && !off {
            log("the controller does not let address resolution be switched off now");
        }
        let r = create_connection(raw, peer);
        if off && !set_resolution(raw, true) {
            log("address resolution stays off until the kernel programs its lists again");
        }
        r
    }

    fn create_connection(raw: &mut std::fs::File, peer: &Addr) -> Result<Link, String> {
        let up = |resolved: bool| if resolved { Link::Resolved } else { Link::Up };
        let mut tried_again = false;
        'create: loop {
            raw.write_all(&bt::le_ext_create_conn(peer, true)).map_err(|e| format!("create connection: {e}"))?;
            let end = Instant::now() + CONNECT_WAIT;
            loop {
                let left = end.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    break 'create;
                }
                match raw_event(raw, left)? {
                    Some(HciEvent::CommandStatus { opcode: bt::OP_LE_EXT_CREATE_CONN, status }) => match status {
                        0 => {}
                        HCI_ACL_EXISTS => return Ok(Link::Up),
                        // the kernel is creating a connection of its own right now: once more
                        HCI_COMMAND_DISALLOWED if !tried_again => {
                            tried_again = true;
                            std::thread::sleep(DISALLOWED_PAUSE);
                            continue 'create;
                        }
                        s => return Err(format!("create connection: HCI status 0x{s:02x}")),
                    },
                    Some(HciEvent::LeConnection { status: 0, peer: p, resolved, .. }) if p == *peer => return Ok(up(resolved)),
                    Some(HciEvent::LeConnection { status: bt::HCI_UNKNOWN_CONN_ID, .. }) => return Ok(Link::NoAnswer),
                    Some(HciEvent::LeConnection { status, peer: p, .. }) if p == *peer => {
                        return Err(format!("connection: HCI status 0x{status:02x}"))
                    }
                    _ => {}
                }
            }
        }
        // no answer: cancel, and see how that ended; the connection may have come at the last moment
        raw.write_all(&bt::le_create_conn_cancel()).map_err(|e| format!("cancel: {e}"))?;
        let end = Instant::now() + CANCEL_WAIT;
        loop {
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(Link::NoAnswer);
            }
            match raw_event(raw, left)? {
                Some(HciEvent::LeConnection { status: 0, peer: p, resolved, .. }) if p == *peer => return Ok(up(resolved)),
                Some(HciEvent::LeConnection { status: bt::HCI_UNKNOWN_CONN_ID, .. }) => return Ok(Link::NoAnswer),
                _ => {}
            }
        }
    }

    // -------------------------------------------------------------- BlueZ --

    struct Bluez<'a> {
        conn: &'a zbus::Connection,
        /// "/org/bluez/hci0"
        adapter: String,
    }

    /// A D-Bus failure without its message (which can hold an object path with an address).
    fn dbus_error(e: &zbus::Error) -> String {
        match e {
            zbus::Error::MethodError(name, _, _) => name.to_string(),
            _ => "D-Bus call failed".into(),
        }
    }

    impl Bluez<'_> {
        fn device_path(&self, addr: &Addr) -> String {
            format!("{}/dev_{}", self.adapter, cover::format_addr(addr).replace(':', "_"))
        }

        /// A property call that gives up after `DBUS_TIMEOUT`.
        fn call<B>(&self, path: &str, method: &str, body: &B) -> Result<zbus::Message, String>
        where
            B: zbus::export::serde::Serialize + zbus::zvariant::DynamicType,
        {
            let call = self.conn.call_method(Some("org.bluez"), path, Some("org.freedesktop.DBus.Properties"), method, body);
            let timeout = async {
                async_io::Timer::after(DBUS_TIMEOUT).await;
                None
            };
            match zbus::block_on(futures_lite::future::or(async { Some(call.await) }, timeout)) {
                Some(Ok(m)) => Ok(m),
                Some(Err(e)) => Err(dbus_error(&e)),
                None => Err("no reply from bluetoothd".into()),
            }
        }

        fn get_bool(&self, path: &str, iface: &str, prop: &str) -> Option<bool> {
            let v = self.call(path, "Get", &(iface, prop)).ok()?.body().deserialize::<OwnedValue>().ok()?;
            bool::try_from(v).ok()
        }

        fn set_bool(&self, path: &str, iface: &str, prop: &str, v: bool) -> Result<(), String> {
            self.call(path, "Set", &(iface, prop, Value::from(v))).map(|_| ()).map_err(|e| format!("{prop}: {e}"))
        }

        /// bluetoothd answers for the adapter.
        fn alive(&self) -> bool {
            self.get_bool(&self.adapter, "org.bluez.Adapter1", "Powered").is_some()
        }

        /// BlueZ has a bond for the address. None: bluetoothd does not answer, so nothing is
        /// known. "No device object" while it does answer is "no bond".
        fn bonded(&self, addr: &Addr) -> Option<bool> {
            let p = self.device_path(addr);
            match self.get_bool(&p, "org.bluez.Device1", "Paired") {
                Some(true) => Some(self.get_bool(&p, "org.bluez.Device1", "Bonded") != Some(false)),
                Some(false) => Some(false),
                None => self.alive().then_some(false),
            }
        }

        fn connected(&self, addr: &Addr) -> bool {
            self.get_bool(&self.device_path(addr), "org.bluez.Device1", "Connected") == Some(true)
        }

        fn services_resolved(&self, addr: &Addr) -> bool {
            self.get_bool(&self.device_path(addr), "org.bluez.Device1", "ServicesResolved") == Some(true)
        }
    }

    // ----------------------------------------------------------- sequence --

    fn random_key() -> io::Result<[u8; 16]> {
        let mut tk = [0u8; 16];
        // SAFETY: tk is valid for its length.
        let n = unsafe { libc::getrandom(tk.as_mut_ptr() as *mut libc::c_void, tk.len(), 0) };
        if n != tk.len() as isize {
            return Err(io::Error::last_os_error());
        }
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

    fn pen_hid(shared: &Shared) -> bool {
        shared.b.hid.refresh();
        pen::find(&*shared.b.hid).is_some()
    }

    /// Feature 112 and 86: the cover announces a seated pen at once and later ones by itself.
    fn announce(shared: &Shared, cover: &CoverPen, host: &Addr) -> Result<(), String> {
        let cap = cover.capability_report()?;
        let hostr = cover.host_report(host)?;
        shared.b.hid.set_feature(&cover.node, &cap).map_err(|e| format!("Feature {}: {e}", cover::FEATURE_CAPABILITY))?;
        shared.b.hid.set_feature(&cover.node, &hostr).map_err(|e| format!("Feature {}: {e}", cover::FEATURE_HOST))
    }

    /// Ask the cover for the pen once more (Feature 86) and wait for it to announce this pen
    /// under the same coupling address (the same pen ID and flag 0012).
    fn seen_again(shared: &Shared, cover: &CoverPen, dev: &mut std::fs::File, host: &Addr, was: &Seen) -> Result<Option<Seen>, String> {
        let mut buf = [0u8; 64];
        while matches!(dev.read(&mut buf), Ok(n) if n > 0) {}
        announce(shared, cover, host)?;
        let end = Instant::now() + CONFIRM_WAIT;
        loop {
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(None);
            }
            if !poll_in(&[dev], left).map_err(|e| format!("{}: {e}", cover.node))?[0] {
                continue;
            }
            while let Ok(n) = dev.read(&mut buf) {
                if n == 0 {
                    return Err(format!("{}: gone", cover.node));
                }
                if let Some(seen) = cover.parse_seen(&buf[..n]).filter(|s| s.pen_id == was.pen_id && s.flag_0012 == was.flag_0012) {
                    return Ok(Some(seen));
                }
            }
        }
    }

    /// What one coupling acts on.
    struct Hands<'a> {
        shared: &'a Shared,
        bz: &'a Bluez<'a>,
        cover: &'a CoverPen,
        dev: &'a mut std::fs::File,
        mgmt: &'a mut Mgmt,
        host: Addr,
        addr: Addr,
        seen: Seen,
        tk: [u8; 16],
        report: Vec<u8>,
        raw: Option<std::fs::File>,
    }

    impl Hands<'_> {
        fn mgmt_cmd(&mut self, what: &str, opcode: u16, pkt: &[u8], wait: Duration) -> Result<u8, Fail> {
            self.mgmt.command(opcode, pkt, wait).map(|r| r.0).map_err(|e| Fail::Other(format!("{what}: {e}")))
        }
    }

    impl Ops for Hands<'_> {
        fn log(&mut self, msg: &str) {
            log(msg);
        }

        fn pause(&mut self, d: Duration) {
            std::thread::sleep(d);
        }

        fn connected(&mut self) -> bool {
            self.bz.connected(&self.addr)
        }

        fn add_key(&mut self) -> Result<(), Fail> {
            let pkt = bt::add_remote_oob(self.mgmt.index, &self.addr, bt::ADDR_LE_RANDOM, &self.tk);
            match self.mgmt_cmd("Add Remote OOB Data", bt::OP_ADD_REMOTE_OOB_DATA, &pkt, Duration::from_secs(5))? {
                bt::STATUS_SUCCESS => Ok(()),
                bt::STATUS_INVALID_PARAMS => Err(Fail::UnsupportedKernel),
                s => Err(Fail::Other(format!("Add Remote OOB Data: {}", bt::status_name(s)))),
            }
        }

        fn clear_key(&mut self) {
            let pkt = bt::add_remote_oob(self.mgmt.index, &self.addr, bt::ADDR_LE_RANDOM, &[0; 16]);
            if let Err(e) = self.mgmt.command(bt::OP_ADD_REMOTE_OOB_DATA, &pkt, Duration::from_secs(5)) {
                log(format!("taking the key back from the kernel: {e}"));
            }
        }

        fn disconnect(&mut self) {
            let pkt = bt::disconnect(self.mgmt.index, &self.addr, bt::ADDR_LE_RANDOM);
            match self.mgmt.command(bt::OP_DISCONNECT, &pkt, Duration::from_secs(5)) {
                Ok((bt::STATUS_SUCCESS | bt::STATUS_NOT_CONNECTED, _)) => {}
                Ok((s, _)) => log(format!("Disconnect: {}", bt::status_name(s))),
                Err(e) => log(format!("Disconnect: {e}")),
            }
        }

        fn prepare(&mut self) -> Result<(), Fail> {
            self.report = self.cover.key_report(&self.seen, &self.tk).map_err(Fail::Other)?;
            self.raw = Some(open_raw(self.mgmt.index).map_err(Fail::Other)?);
            Ok(())
        }

        fn unpair(&mut self) -> Result<(), Fail> {
            let pkt = bt::unpair_device(self.mgmt.index, &self.addr, bt::ADDR_LE_RANDOM, false);
            match self.mgmt_cmd("Unpair Device", bt::OP_UNPAIR_DEVICE, &pkt, Duration::from_secs(5))? {
                bt::STATUS_SUCCESS | bt::STATUS_NOT_PAIRED => {}
                s => return Err(Fail::Other(format!("Unpair Device: {}", bt::status_name(s)))),
            }
            // bluetoothd drops its keys and, the pen not being connected, its device object
            let (bz, addr) = (self.bz, self.addr);
            wait_for(Duration::from_secs(2), || bz.bonded(&addr) == Some(false));
            Ok(())
        }

        fn write_key(&mut self) -> Result<(), Fail> {
            self.dev.write_all(&self.report).map_err(|e| Fail::Other(format!("Output {}: {e}", cover::OUTPUT_KEY)))
        }

        fn connect(&mut self) -> Result<Link, Fail> {
            let raw = self.raw.as_mut().ok_or_else(|| Fail::Other("HCI socket not open".into()))?;
            connect(raw, &self.addr, true).map_err(Fail::Other)
        }

        fn pair(&mut self) -> Result<PairReply, Fail> {
            let pkt = bt::pair_device(self.mgmt.index, &self.addr, bt::ADDR_LE_RANDOM, bt::IO_NO_INPUT_NO_OUTPUT);
            match self.mgmt.command(bt::OP_PAIR_DEVICE, &pkt, PAIR_WAIT) {
                Ok((status, _)) => Ok(PairReply::from_status(status)),
                Err(e) if e.kind() == io::ErrorKind::TimedOut => {
                    let cancel = bt::cancel_pair_device(self.mgmt.index, &self.addr, bt::ADDR_LE_RANDOM);
                    let _ = self.mgmt.command(bt::OP_CANCEL_PAIR_DEVICE, &cancel, Duration::from_secs(2));
                    // the cancelled command's own reply
                    let _ = self.mgmt.drain(Some(bt::OP_PAIR_DEVICE));
                    Ok(PairReply::Timeout)
                }
                Err(e) => Err(Fail::Other(format!("Pair Device: {e}"))),
            }
        }

        fn key_arrived(&mut self, wait: Duration) -> bool {
            let end = Instant::now() + wait;
            loop {
                let _ = self.mgmt.drain(None);
                if self.mgmt.ev.ltk.contains(&self.addr) {
                    return true;
                }
                let left = end.saturating_duration_since(Instant::now());
                if left.is_zero() || poll_in(&[&self.mgmt.sock], left).is_err() {
                    return false;
                }
            }
        }

        fn announce(&mut self) -> Result<bool, Fail> {
            match seen_again(self.shared, self.cover, self.dev, &self.host, &self.seen).map_err(Fail::Other)? {
                Some(seen) => {
                    self.report = self.cover.key_report(&seen, &self.tk).map_err(Fail::Other)?;
                    self.seen = seen;
                    Ok(true)
                }
                None => Ok(false),
            }
        }
    }

    /// How a connection to a bonded pen ends when the kernel is not given the key the cover
    /// hands the pen: the stored bond has to carry the link. bluetoothd gets the link through
    /// its ATT listener and encrypts it with the stored key by itself; this only watches.
    #[allow(clippy::too_many_arguments)]
    fn probe(
        shared: &Shared,
        bz: &Bluez,
        cover: &CoverPen,
        dev: &mut std::fs::File,
        mgmt: &mut Mgmt,
        keys: &mut cover::PendingKeys,
        seen: &Seen,
        addr: &Addr,
    ) -> cover::Probe {
        mgmt.forget(addr);
        // a probe is no pairing: one bluetoothd starts on this link without the key is refused
        mgmt.guard.watch(*addr);
        // the pen only advertises once it has been handed a key. The kernel does not get it, but
        // the pen keeps it for its next pairing: it is pending from here on
        let connected =
            keys.key_for(addr, random_key).map_err(|e| format!("key: {e}")).and_then(|(tk, _)| cover.key_report(seen, &tk)).and_then(
                |report| {
                    let mut raw = open_raw(mgmt.index)?;
                    dev.write_all(&report).map_err(|e| format!("Output {}: {e}", cover::OUTPUT_KEY))?;
                    connect(&mut raw, addr, false)
                },
            );
        match connected {
            // (a bonded pen may answer from its private address: encryption works on either link)
            Ok(Link::Up | Link::Resolved) => {}
            Ok(Link::NoAnswer) => return cover::Probe::NoAnswer,
            Err(e) => {
                log(format!("probe: {e}"));
                return cover::Probe::NoAnswer;
            }
        }
        let up = Instant::now();
        loop {
            if mgmt.drain(None).is_err() {
                return cover::Probe::NoAnswer;
            }
            // (on this link, during the probe, a failed pairing is the pen refusing the bond too)
            if mgmt.ev.dead.contains(addr) || mgmt.ev.pairing_failed.contains(addr) {
                return cover::Probe::AuthFailure;
            }
            if mgmt.ev.disconnected.contains(addr) {
                log("probe: the link went down again without an encryption failure");
                return cover::Probe::NoAnswer;
            }
            // an encryption failure shows within a fraction of a second; a bond that works has
            // its services back (from BlueZ's cache) and the pen's HID node soon after
            if up.elapsed() >= PROBE_SETTLE && bz.connected(addr) && (bz.services_resolved(addr) || pen_hid(shared)) {
                return cover::Probe::Connected;
            }
            if up.elapsed() >= PROBE_WAIT {
                log("probe: connected, but BlueZ did not bring the pen's services up");
                return cover::Probe::NoAnswer;
            }
            let _ = poll_in(&[&mgmt.sock], Duration::from_millis(250));
        }
    }

    /// The whole coupling for a pen the cover announced; the result text for `Status`.
    /// `replace`: the pen has a bond that is proven dead (or the request was forced). Never
    /// called for a pen that has a bond and is connected.
    #[allow(clippy::too_many_arguments)]
    fn couple(
        shared: &Shared,
        bz: &Bluez,
        cover: &CoverPen,
        dev: &mut std::fs::File,
        mgmt: &mut Mgmt,
        keys: &mut cover::PendingKeys,
        host: &Addr,
        seen: &Seen,
        addr: &Addr,
        replace: bool,
    ) -> Result<&'static str, Fail> {
        let masked = mask_addr(addr);
        let (tk, pending) = keys.key_for(addr, random_key).map_err(|e| Fail::Other(format!("key: {e}")))?;
        log(if pending { "using the key this pen was handed before (no pairing has used it)" } else { "a new key" });
        // bondable: without it the pairing asks for no bonding and no key comes
        let pairable = bz.get_bool(&bz.adapter, "org.bluez.Adapter1", "Pairable");
        if pairable == Some(false) {
            bz.set_bool(&bz.adapter, "org.bluez.Adapter1", "Pairable", true).map_err(Fail::Other)?;
        }
        mgmt.forget(addr);
        // from here on no pairing with this address completes without the key
        mgmt.guard.watch(*addr);
        let mut hands =
            Hands { shared, bz, cover, dev, mgmt, host: *host, addr: *addr, seen: seen.clone(), tk, report: Vec::new(), raw: None };
        let r = coupling::couple(&mut hands, replace);
        hands.tk.fill(0);
        hands.report.fill(0);
        drop(hands);
        if pairable == Some(false) {
            if let Err(e) = bz.set_bool(&bz.adapter, "org.bluez.Adapter1", "Pairable", false) {
                log(format!("restoring {e}"));
            }
        }
        r?;
        keys.paired(addr);
        log(format!("paired with {masked}"));

        if !wait_for(Duration::from_secs(5), || bz.bonded(addr) == Some(true)) {
            return Err(Fail::Other("the pen gave its key, but BlueZ does not show the bond".into()));
        }
        if let Err(e) = bz.set_bool(&bz.device_path(addr), "org.bluez.Device1", "Trusted", true) {
            log(e);
        }
        if wait_for(HID_WAIT, || pen_hid(shared)) {
            return Ok("coupled");
        }
        if shared.cfg().pen.couple_restart_bluetooth {
            log("no HID node from BlueZ, restarting bluetooth.service (pen.couple_restart_bluetooth)");
            match std::process::Command::new("systemctl").args(["restart", "bluetooth.service"]).status() {
                Ok(s) if s.success() => {
                    if wait_for(Duration::from_secs(15), || pen_hid(shared)) {
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

    struct Memory {
        /// when the link to a pen was last dropped for a failed encryption
        dead: HashMap<Addr, Instant>,
        /// the last failed coupling of a pen and how many there were in a row
        failed: HashMap<Addr, (Instant, u32)>,
        coupled_at: HashMap<Addr, Instant>,
        probed_at: HashMap<Addr, Instant>,
        /// keys written to the cover that no pairing has used up; never logged
        keys: cover::PendingKeys,
        /// the pens' H that a pen answered under (`cover::HASH_FILE`)
        hashes: PenHashes,
        /// H learned from a bond or the prefix, not yet answered under; never logged
        tentative: HashMap<[u8; 4], PenHash>,
        /// suspended time when a resume was last seen
        resume_mark: f64,
        /// the cover is asked again only because of a resume, until then
        quiet_until: Option<Instant>,
        /// the session error last logged
        error: Option<String>,
    }

    impl Memory {
        /// The pen's H: the one it answered under before, else one learned from a pen bond on
        /// this host (an import of the Windows bond, or a coupling by an earlier version),
        /// else one from the user's prefix.
        fn pen_hash(&mut self, shared: &Shared, seen: &Seen, host: &Addr) -> Option<PenHash> {
            if let Some(h) = self.hashes.get(&seen.pen_id).or_else(|| self.tentative.get(&seen.pen_id).copied()) {
                return Some(h);
            }
            let h = match cover::learn_hash(&cover::pen_bonds(&shared.root, host), seen, host, &self.hashes) {
                Ok(h) => {
                    log("the pen's coupling address comes from its bond on this host");
                    Some(h)
                }
                Err(n) => {
                    if n > 1 {
                        log(format!("{n} pen bonds on this host could be this pen's: none is used"));
                    }
                    let prefix = shared.root.read_opt(cover::PREFIX_FILE)?;
                    match cover::parse_prefix(&prefix) {
                        Some(p) => {
                            log(format!("the pen's coupling address comes from the prefix in {}", cover::PREFIX_FILE));
                            Some(PenHash::from_prefix(&p, &seen.pen_id))
                        }
                        None => {
                            log(format!("{}: not 32 hex digits; ignored", cover::PREFIX_FILE));
                            None
                        }
                    }
                }
            }?;
            self.tentative.insert(seen.pen_id, h);
            Some(h)
        }

        /// The pen took a connection under its coupling address after the cover handed it a
        /// key (a probe that encrypted, or a coupling): its H is kept.
        fn answered(&mut self, shared: &Shared, pen_id: &[u8; 4], h: PenHash) {
            self.tentative.remove(pen_id);
            if self.hashes.insert(*pen_id, h) {
                let path = shared.state_dir.join(cover::HASH_FILE);
                if let Err(e) = self.hashes.save(&path) {
                    log(format!("cannot keep the pen's coupling address in {}: {e}", path.display()));
                }
            }
        }

        fn resumed(&mut self) -> bool {
            let r = clock::resumed(&mut self.resume_mark);
            if r {
                self.quiet_until = Some(Instant::now() + CONFIRM_WAIT);
            }
            r
        }
    }

    /// Why a session ended.
    enum End {
        /// automatic coupling is off and nothing is asked for
        Idle,
        /// the cover node went away
        CoverGone,
        BluetoothOff,
        Error(String),
    }

    /// One life of the cover node.
    fn session(shared: &Shared, conn: &zbus::Connection, cover: &CoverPen, mem: &mut Memory) -> End {
        let fail = |e: String| End::Error(e);
        let c = &shared.couple;
        let mut mgmt = match Mgmt::open() {
            Ok(m) => m,
            Err(e) => return fail(e),
        };
        let info = match mgmt.info() {
            Ok(i) => i,
            Err(e) => return fail(e),
        };
        if !info.has(bt::SETTING_POWERED) || !info.has(bt::SETTING_LE) {
            return End::BluetoothOff;
        }
        if info.has(bt::SETTING_PRIVACY) {
            // the kernel would compute the pairing's confirm value with a private address,
            // the pen with the public one the cover told it
            return fail("the adapter uses private addresses (privacy is on): the cover coupling needs the public address".into());
        }
        let host = info.address;
        let opened = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(shared.root.p(&format!("/dev/{}", cover.node)));
        let mut dev = match opened {
            Ok(d) => d,
            Err(e) => return fail(format!("{}: {e}", cover.node)),
        };
        let bz = Bluez { conn, adapter: format!("/org/bluez/hci{}", mgmt.index) };
        // a manual request waits for the cover's announcement until then; whether it is forced
        let mut manual: Option<(Instant, bool)> = c.take_request().map(|force| (Instant::now() + ANNOUNCE_WAIT, force));
        mem.resumed();
        macro_rules! announce {
            () => {
                if let Err(e) = announce(shared, cover, &host) {
                    return fail(e);
                }
            };
        }
        announce!();
        log(format!("cover {} told the host address {}", cover.node, mask_addr(&host)));
        mem.error = None;
        {
            // Bluetooth was off when the thread first looked (boot): that is over
            let mut st = c.status.lock().unwrap();
            if st.result == "bluetooth-off" {
                st.result.clear();
            }
        }
        // a bonded pen that was seated unconnected: look again then
        let mut check: Option<(Addr, Instant)> = None;
        // it was still not connected, and the cover was asked whether the pen is still there
        let mut confirm: Option<(Addr, Instant)> = None;
        // a coupling failed: ask the cover again then
        let mut retry_at: Option<Instant> = None;
        let mut buf = [0u8; 64];
        loop {
            let ready = match poll_in(&[&dev, &mgmt.sock], Duration::from_secs(1)) {
                Ok(r) => r,
                Err(_) => return End::CoverGone,
            };
            if let Some(force) = c.take_request() {
                manual = Some((Instant::now() + ANNOUNCE_WAIT, force));
                announce!();
            } else if mem.resumed() {
                announce!();
            } else if retry_at.is_some_and(|t| Instant::now() >= t) {
                retry_at = None;
                announce!();
            }
            let auto = shared.cfg().pen.auto_couple;
            if !auto && manual.is_none() {
                return End::Idle;
            }
            if ready[1] {
                if let Err(e) = mgmt.drain(None) {
                    return fail(format!("management socket: {e}"));
                }
            }
            // what the events said since the last look
            if mgmt.ev.removed {
                return fail("the Bluetooth adapter went away".into());
            }
            if mgmt.ev.settings.take().is_some_and(|s| s & bt::SETTING_POWERED == 0) {
                return End::BluetoothOff;
            }
            for a in mgmt.ev.dead.drain(..) {
                if mem.dead.insert(a, Instant::now()).is_none() {
                    log(format!("encryption with {} failed: its bond is dead", mask_addr(&a)));
                }
            }
            for a in mgmt.ev.ltk.drain(..) {
                // paired again (by anyone): the bond is new
                mem.dead.remove(&a);
            }
            mgmt.ev.pairing_failed.clear();
            mgmt.ev.disconnected.clear();

            if let Some((addr, _)) = check.filter(|(_, t)| Instant::now() >= *t) {
                check = None;
                if bz.connected(&addr) {
                    mem.dead.remove(&addr);
                    c.set(STATE_IDLE, Some("already-coupled".into()));
                } else {
                    log(format!(
                        "{} has a bond and is not connected after {} s: asking the cover whether it is still seated, to probe the bond",
                        mask_addr(&addr),
                        CONNECT_GRACE.as_secs()
                    ));
                    announce!();
                    confirm = Some((addr, Instant::now() + CONFIRM_WAIT));
                }
            }
            let mut seen = None;
            if ready[0] {
                loop {
                    match dev.read(&mut buf) {
                        Ok(0) => return End::CoverGone,
                        Ok(n) => seen = cover.parse_seen(&buf[..n]).or(seen),
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                        Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                        Err(_) => return End::CoverGone,
                    }
                }
            }
            let Some(seen) = seen else {
                if manual.is_some_and(|(t, _)| Instant::now() >= t) {
                    manual = None;
                    log("the cover announced no pen");
                    c.set(STATE_IDLE, Some("no-pen".into()));
                } else if confirm.is_some_and(|(_, t)| Instant::now() >= t) {
                    confirm = None;
                    log("the pen is no longer in the cover; nothing done");
                }
                continue;
            };

            let request = manual.take();
            let requested = request.is_some();
            let Some(hash) = mem.pen_hash(shared, &seen, &host) else {
                log("pen in the cover slot, but its coupling address is not known: no cover bond of this pen on this host and no coupling prefix. Pair it in pairing mode (hold the tail button), or couple it once under Windows and import that bond (tools/linux/pen-bond-from-windows.py)");
                c.status.lock().unwrap().address.clear();
                c.set(STATE_IDLE, Some("coupling-unavailable".into()));
                continue;
            };
            let addr = cover::seen_address(&seen, &host, &hash);
            let masked = mask_addr(&addr);
            c.status.lock().unwrap().address = masked.clone();
            let within = |m: &HashMap<Addr, Instant>, d: Duration| m.get(&addr).is_some_and(|t| t.elapsed() < d);
            let confirmed = confirm.take_if(|(a, _)| *a == addr).is_some();
            if requested && within(&mem.coupled_at, MANUAL_AFTER) {
                log("coupled a moment ago: request ignored");
                c.set(STATE_IDLE, Some("failed: coupled a moment ago, try again in a few seconds".into()));
                continue;
            }
            let facts = cover::SeatedPen {
                requested,
                forced: request.is_some_and(|r| r.1),
                bonded: bz.bonded(&addr),
                connected: bz.connected(&addr),
                stale: within(&mem.dead, DEAD_FRESH),
                failed_recently: mem.failed.get(&addr).is_some_and(|(t, n)| t.elapsed() < cover::retry_after(*n)),
                coupled_recently: within(&mem.coupled_at, RECOUPLE_AFTER),
                probed_recently: within(&mem.probed_at, PROBE_AFTER),
                after_resume: mem.quiet_until.is_some_and(|t| Instant::now() < t),
            };
            let replace = match cover::on_seated(facts) {
                cover::Seated::Connected => {
                    // (not proof of the pen's H: a bond learned from could be another pen's,
                    // connected meanwhile)
                    if requested {
                        log(format!("{masked} is connected under its bond: left alone"));
                    }
                    mem.dead.remove(&addr);
                    c.set(STATE_IDLE, Some("already-coupled".into()));
                    continue;
                }
                cover::Seated::Skip => {
                    if facts.bonded.is_none() {
                        log("bluetoothd does not answer: nothing is known about the pen's bond, nothing done");
                        if requested {
                            c.set(STATE_IDLE, Some("failed: bluetoothd does not answer".into()));
                        }
                    } else if requested {
                        c.set(STATE_IDLE, None);
                    }
                    continue;
                }
                cover::Seated::Probe if !requested && !confirmed => {
                    // it may connect by itself: look again before touching anything
                    if check.is_none() {
                        check = Some((addr, Instant::now() + CONNECT_GRACE));
                    }
                    continue;
                }
                cover::Seated::Probe => {
                    // (a request that came in up to here is served by this probe)
                    c.begin();
                    log(format!("{masked} has a bond and is not connected: probing it (a key to the cover only, connection, no pairing)"));
                    let outcome = probe(shared, &bz, cover, &mut dev, &mut mgmt, &mut mem.keys, &seen, &addr);
                    mgmt.forget(&addr);
                    while matches!(dev.read(&mut buf), Ok(n) if n > 0) {}
                    mem.probed_at.insert(addr, Instant::now());
                    let (text, dead) = cover::after_probe(outcome);
                    if dead {
                        log(format!("probe of {masked}: connected, encryption with the stored key failed: the bond is dead (coupled elsewhere since); asking the cover again to couple"));
                        mem.dead.insert(addr, Instant::now());
                        if requested {
                            manual = Some((Instant::now() + ANNOUNCE_WAIT, false));
                        }
                        c.set(if requested { STATE_WAITING } else { STATE_IDLE }, Some(text.into()));
                        announce!();
                    } else {
                        match outcome {
                            cover::Probe::Connected => {
                                mem.dead.remove(&addr);
                                mem.answered(shared, &seen.pen_id, hash);
                                log(format!(
                                    "probe of {masked}: connected and encrypted with the stored key: the bond is good, the pen is online"
                                ))
                            }
                            _ => log(format!(
                                "probe of {masked}: no answer: nothing is known, the bond is kept (not probed again for {} s)",
                                PROBE_AFTER.as_secs()
                            )),
                        }
                        c.set(STATE_IDLE, Some(text.into()));
                    }
                    continue;
                }
                cover::Seated::Couple => false,
                cover::Seated::Replace => true,
            };
            check = None;
            // (a request that came in up to here is served by this coupling)
            c.begin();
            let why = match (replace, facts.stale, requested) {
                (true, true, _) => "its bond is dead: replacing it",
                (true, false, _) => "forced: replacing its bond",
                (false, _, true) => "no bond: coupling (requested)",
                (false, _, false) => "no bond: coupling",
            };
            log(format!("pen in the cover slot, coupling address {masked}, {why}"));
            let r = couple(shared, &bz, cover, &mut dev, &mut mgmt, &mut mem.keys, &host, &seen, &addr, replace);
            // the attempt's own events are not evidence about the new bond
            mgmt.forget(&addr);
            // what the cover sent meanwhile belongs to this attempt
            while matches!(dev.read(&mut buf), Ok(n) if n > 0) {}
            let text = match r {
                Ok(t) => {
                    mem.answered(shared, &seen.pen_id, hash);
                    mem.dead.remove(&addr);
                    mem.failed.remove(&addr);
                    mem.coupled_at.insert(addr, Instant::now());
                    t.to_string()
                }
                Err(e) => {
                    let n = mem.failed.get(&addr).map_or(0, |f| f.1) + 1;
                    mem.failed.insert(addr, (Instant::now(), n));
                    let again = cover::retry_after(n);
                    let text = match e {
                        Fail::UnsupportedKernel => {
                            log("this kernel does not take a Temporary Key (Add Remote OOB Data: invalid parameters; kernel patch 0023)");
                            "unsupported-kernel".to_string()
                        }
                        Fail::NoAnswer => "no-answer".to_string(),
                        Fail::Refused => "failed: the pen refused the pairing".to_string(),
                        Fail::Other(e) => format!("failed: {e}"),
                    };
                    if text != "unsupported-kernel" {
                        log(format!("{masked}: asking the cover again in {} min, with the same key", again.as_secs() / 60));
                        retry_at = Some(Instant::now() + again + Duration::from_secs(1));
                    }
                    text
                }
            };
            log(format!("result: {text}"));
            c.set(STATE_IDLE, Some(text));
        }
    }

    pub fn run(shared: Arc<Shared>, conn: zbus::Connection) {
        let c = &shared.couple;
        let mut mem = Memory {
            dead: HashMap::new(),
            failed: HashMap::new(),
            coupled_at: HashMap::new(),
            probed_at: HashMap::new(),
            keys: cover::PendingKeys::default(),
            hashes: PenHashes::load(&shared.state_dir.join(cover::HASH_FILE)),
            tentative: HashMap::new(),
            resume_mark: clock::suspended(),
            quiet_until: None,
            error: None,
        };
        loop {
            if !shared.cfg().pen.auto_couple && !c.pending() {
                c.wait(Duration::from_secs(5));
                continue;
            }
            shared.b.hid.refresh();
            let Some(cover) = cover::find(&*shared.b.hid) else {
                if c.take_request().is_some() {
                    log("no cover with a pen slot");
                    c.set(STATE_IDLE, Some("no-cover".into()));
                }
                std::thread::sleep(Duration::from_secs(3));
                continue;
            };
            // a request that is waiting when the session ends gets an answer
            let asked = |c: &Couple| c.take_request().is_some() || c.status().state != STATE_IDLE;
            match session(&shared, &conn, &cover, &mut mem) {
                End::Idle => {}
                End::CoverGone => {
                    if asked(c) {
                        log("the cover went away");
                        c.set(STATE_IDLE, Some("no-cover".into()));
                    }
                }
                End::BluetoothOff => {
                    // at boot the adapter comes up a little later: look again soon, say it once
                    if c.status().result != "bluetooth-off" {
                        log("Bluetooth is off or not up yet; waiting for it");
                    }
                    c.take_request();
                    c.set(STATE_IDLE, Some("bluetooth-off".into()));
                    std::thread::sleep(Duration::from_secs(3));
                }
                End::Error(e) => {
                    // the same trouble is said once
                    if mem.error.as_ref() != Some(&e) {
                        log(&e);
                    }
                    if asked(c) {
                        c.set(STATE_IDLE, Some(format!("failed: {e}")));
                    }
                    mem.error = Some(e);
                    std::thread::sleep(Duration::from_secs(10));
                }
            }
            c.set(STATE_IDLE, None);
            std::thread::sleep(Duration::from_secs(1));
        }
    }
}
