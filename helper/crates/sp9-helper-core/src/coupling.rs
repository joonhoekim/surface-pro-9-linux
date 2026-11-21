// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
//! The order of a cover coupling, without the sockets: what is done to the kernel, the cover
//! and BlueZ goes through [`Ops`], so the order can be tested against a scripted fake.
//! `sp9-helperd`'s coupling thread implements `Ops` on the real things.
//!
//! What the order has to respect (docs/research/gaps/pen-vendor-exchange.md, docs/helper-reference.md):
//!
//! - Everything that can fail without the pen (the kernel refusing the key, packing the
//!   report, the raw socket) is done before an existing bond is dropped.
//! - The pen takes the key from the cover only while it is not connected, and that takes
//!   longer than the time between a write and a connection made right after it (inferred;
//!   measured: a coupling that wrote the key and connected at once was refused, by a pen
//!   without the key or with an earlier one, whether or not the key was repeated). So the
//!   key is handed over first, without connecting (the priming write); 2.5 s later the cover
//!   is asked again, the same key written again, and only then the connection made. That
//!   paired at the first try, three times out of three.
//! - After the connection the first Pair Device goes out after 0.4 s: bluetoothd starts a
//!   pairing of its own on a new link about a second after the connection, and with an
//!   earlier build of patch 0023, which forgot the key as soon as a pairing used it, that
//!   pairing would use it up.
//! - A refused pairing gets one repeat on the link, then the round ends; the next round
//!   starts 20 s later with the priming write again (rounds a few seconds apart all failed,
//!   an attempt 25 s later went through).
//! - The kernel (patch 0023) forgets the key once a pairing with it has completed, and keeps
//!   it after a failed one; while it holds a key for the pen it refuses a pairing that would
//!   not use it (the pen answering without the OOB flag) instead of falling back to Just
//!   Works. Earlier builds forgot the key as soon as a pairing used it, or kept it for good:
//!   it is given again before every Pair Device, which does no harm with any of them.
//! - bluetoothd starts a pairing of its own on a new link about a second after the
//!   connection, and with the key in the kernel that one is a legacy OOB pairing too. A long
//!   term key from either pairing is success. Already Paired or Busy from Pair Device after
//!   that is not a reason to unpair: unpairing a device bluetoothd has paired makes it drop
//!   the link, and after a link the host dropped the pen is silent for half a minute.
//! - No pairing completes on a coupling link without the key (fail closed, as patch 0023 is
//!   in the kernel): from the first coupling or probe of a pen on, the helper refuses every
//!   confirmation or passkey request for its coupling address, whoever started the pairing
//!   and whatever the link's address type ([`Guard`]); the key stays in the kernel for the
//!   whole coupling, also on a link through the pen's private address; and a coupling that
//!   fails with a link up and no key from the pen drops that link before it takes the key
//!   back. A reconnection under an existing bond is encryption with the stored key, not a
//!   pairing, and is not touched.
//! - There is never a second Create Connection on a link that is up.
//! - A link that came up through the pen's resolvable private address cannot be paired on:
//!   the kernel computes the pairing's confirm value with the identity address the controller
//!   resolved (and its "resolved" address type), the pen with the address it used on the air
//!   (measured: "Confirm value failed" on every such link, never on a link to the static
//!   coupling address). That happens when the controller's resolving list still has the pen
//!   from an earlier bond and the pen, which still believes in that bond, advertises to
//!   reconnect. `Ops::connect` keeps the controller from resolving while it connects; a link
//!   that is resolved all the same is not paired on.

use crate::btmgmt::{self, Event};
use crate::cover::Addr;
use std::time::Duration;

/// Between the priming write (the key to the cover, no connection) and asking the cover again.
pub const PRIME_WAIT: Duration = Duration::from_millis(2500);
/// Between the connection and the first Pair Device.
pub const KEY_SETTLE: Duration = Duration::from_millis(400);
/// Before the one repeat of a refused pairing on the same link.
pub const PAIR_PAUSE: Duration = Duration::from_millis(1000);
/// Pair Device the pen may refuse on one link.
pub const PAIR_ATTEMPTS: u32 = 2;
/// The long term key event comes with the reply of a pairing that went through.
pub const LTK_WAIT: Duration = Duration::from_secs(2);
/// Someone else's pairing is under way (Busy): its key is waited for this long.
pub const BUSY_WAIT: Duration = Duration::from_secs(5);
/// Rounds of one coupling, each beginning with the priming write.
pub const ROUNDS: u32 = 2;
/// Between two rounds.
pub const ROUND_PAUSE: Duration = Duration::from_secs(20);

/// Why a coupling ended without a bond.
#[derive(Clone, Debug, PartialEq)]
pub enum Fail {
    /// the pen took no connection
    NoAnswer,
    /// the kernel does not take a Temporary Key (no patch 0023)
    UnsupportedKernel,
    /// the pen refused every pairing
    Refused,
    Other(String),
}

/// How LE Create Connection ended.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Link {
    /// connected to the pen's static coupling address
    Up,
    /// connected, but through the pen's private address (the controller resolved it)
    Resolved,
    /// the pen took no connection
    NoAnswer,
}

/// The reply to Pair Device.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PairReply {
    Success,
    AlreadyPaired,
    /// another pairing with the device is under way
    Busy,
    /// the pen refused: no key yet, another key, or the confirmation was turned down
    AuthFailed,
    /// the link is gone
    NotConnected,
    /// no reply; the pairing was cancelled
    Timeout,
    Other(u8),
}

impl PairReply {
    pub fn from_status(status: u8) -> PairReply {
        match status {
            btmgmt::STATUS_SUCCESS => PairReply::Success,
            btmgmt::STATUS_ALREADY_PAIRED => PairReply::AlreadyPaired,
            btmgmt::STATUS_BUSY => PairReply::Busy,
            btmgmt::STATUS_AUTH_FAILED => PairReply::AuthFailed,
            btmgmt::STATUS_NOT_CONNECTED | btmgmt::STATUS_DISCONNECTED => PairReply::NotConnected,
            s => PairReply::Other(s),
        }
    }
}

/// What a coupling does to the world.
pub trait Ops {
    fn log(&mut self, msg: &str);
    fn pause(&mut self, d: Duration);
    /// A link to the pen is up.
    fn connected(&mut self) -> bool;
    /// Give the key to the kernel (MGMT Add Remote OOB Data).
    fn add_key(&mut self) -> Result<(), Fail>;
    /// Take it back; never fails the coupling.
    fn clear_key(&mut self);
    /// Drop the link to the pen (MGMT Disconnect); never fails the coupling.
    fn disconnect(&mut self);
    /// What can fail without the pen: pack the cover report, open the raw HCI socket.
    fn prepare(&mut self) -> Result<(), Fail>;
    /// Drop the keys the kernel has for the pen (MGMT Unpair Device, disconnect = 0).
    fn unpair(&mut self) -> Result<(), Fail>;
    /// The key to the cover (Output 111).
    fn write_key(&mut self) -> Result<(), Fail>;
    /// LE Create Connection to the coupling address, with the controller's address resolution
    /// off for its duration where that is possible.
    fn connect(&mut self) -> Result<Link, Fail>;
    /// MGMT Pair Device, waiting for its reply.
    fn pair(&mut self) -> Result<PairReply, Fail>;
    /// The pen gave a long term key since the coupling began (to whoever paired); waits up
    /// to `wait` for it.
    fn key_arrived(&mut self, wait: Duration) -> bool;
    /// Ask the cover for the pen again (Feature 86); false when it does not announce it.
    fn announce(&mut self) -> Result<bool, Fail>;
}

/// Pair Device on a link that is up, until the pen gives its key. Ok(false): refused, or
/// the link went down.
fn pair_on_link(ops: &mut dyn Ops) -> Result<bool, Fail> {
    ops.pause(KEY_SETTLE);
    let (mut refused, mut unpaired, mut empty) = (0, false, 0);
    loop {
        // bluetoothd's own pairing may have got there first
        if ops.key_arrived(Duration::ZERO) {
            return Ok(true);
        }
        if !ops.connected() {
            ops.log("the link went down");
            return Ok(false);
        }
        // earlier builds of patch 0023 forgot the key once a pairing used it
        ops.add_key()?;
        match ops.pair()? {
            PairReply::Success => {
                if ops.key_arrived(LTK_WAIT) {
                    return Ok(true);
                }
                // the kernel answers "success" without pairing while the security manager channel
                // of a fresh link is not up
                empty += 1;
                ops.log(&format!("Pair Device returned without a key ({empty})"));
                if empty >= 3 {
                    return Ok(false);
                }
                ops.pause(KEY_SETTLE);
            }
            PairReply::AlreadyPaired => {
                if ops.key_arrived(LTK_WAIT) {
                    return Ok(true);
                }
                if unpaired {
                    return Err(Fail::Other("Pair Device: already paired, and no key came".into()));
                }
                // keys the kernel has although BlueZ shows no bond and no pairing ran: old ones
                ops.log("the kernel has old keys for the pen that BlueZ does not show: dropping them");
                ops.unpair()?;
                unpaired = true;
            }
            PairReply::Busy => {
                ops.log("another pairing is under way: waiting for its key");
                return Ok(ops.key_arrived(BUSY_WAIT));
            }
            PairReply::AuthFailed => {
                refused += 1;
                ops.log(&format!("the pen refused the pairing ({refused} of {PAIR_ATTEMPTS} on this link)"));
                if refused >= PAIR_ATTEMPTS {
                    return Ok(ops.key_arrived(Duration::ZERO));
                }
                ops.pause(PAIR_PAUSE);
            }
            PairReply::NotConnected => {
                ops.log("the link went down");
                return Ok(false);
            }
            PairReply::Timeout => return Err(Fail::Other("Pair Device: no reply".into())),
            PairReply::Other(s) => return Err(Fail::Other(format!("Pair Device: {}", btmgmt::status_name(s)))),
        }
    }
}

fn rounds(ops: &mut dyn Ops, replace: bool) -> Result<(), Fail> {
    if replace {
        ops.unpair()?;
        ops.log("dropped the old bond");
    }
    // the link that is up came through the pen's private address
    let mut resolved = false;
    for round in 0..ROUNDS {
        if round > 0 {
            ops.log(&format!("round {} of {ROUNDS} in {} s", round + 1, ROUND_PAUSE.as_secs()));
            ops.pause(ROUND_PAUSE);
            if ops.key_arrived(Duration::ZERO) {
                return Ok(());
            }
            if !ops.announce()? && !ops.connected() {
                return Err(Fail::Other("the pen refused the pairing and the cover does not announce it any more".into()));
            }
        }
        // never a second connection on a link that is up
        if ops.connected() {
            if resolved {
                ops.log("the link through the pen's private address is still up: nothing to pair on");
                continue;
            }
            ops.log("the link is up: pairing on it");
            ops.write_key()?;
        } else {
            resolved = false;
            // the pen takes the key only while it is not connected, and not at once: hand it over,
            // wait, ask the cover again, write the same key again, then connect
            ops.log("handing the key to the pen through the cover (no connection yet)");
            ops.write_key()?;
            ops.pause(PRIME_WAIT);
            if !ops.announce()? {
                return Err(Fail::Other("the cover does not announce the pen any more".into()));
            }
            ops.write_key()?;
            match ops.connect()? {
                Link::NoAnswer => return Err(Fail::NoAnswer),
                Link::Resolved => {
                    // the pairing's confirm value cannot match on this link
                    ops.log("connected through the pen's private address (the controller still resolves its old identity): no pairing on this link");
                    resolved = true;
                    // bluetoothd pairs on the link by itself. The key stays in the kernel: that
                    // pairing uses it and fails on the confirm value, or it does not and is
                    // refused (by the kernel, and here by the Guard). Taking the key back would
                    // let it run as Just Works.
                    continue;
                }
                Link::Up => ops.log("connected, pairing"),
            }
        }
        if pair_on_link(ops)? {
            return Ok(());
        }
    }
    Err(if resolved {
        Fail::Other("the pen only answers from its private address (stale resolving list entry in the controller)".into())
    } else {
        Fail::Refused
    })
}

/// One coupling of the pen the cover announced. `replace`: the pen has a bond that is known
/// to be dead (or the request was forced); it is dropped once nothing but the pen can fail.
/// The key is taken back from the kernel however it ends; when the coupling failed and a
/// link to the pen is still up without a key from it, the link is dropped first, so that no
/// pairing can run on it once the key is gone.
pub fn couple(ops: &mut dyn Ops, replace: bool) -> Result<(), Fail> {
    // the first Add Remote OOB Data also tells whether this kernel takes a key at all
    ops.add_key()?;
    let r = match ops.prepare() {
        Ok(()) => rounds(ops, replace),
        Err(e) => Err(e),
    };
    if r.is_err() && ops.connected() && !ops.key_arrived(Duration::ZERO) {
        ops.log("the coupling failed with a link up and no key from the pen: dropping the link");
        ops.disconnect();
    }
    ops.clear_key();
    r
}

/// A pairing request the helper refuses.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Refusal {
    /// User Confirmation Negative Reply
    Confirm(Addr, u8),
    /// User Passkey Negative Reply
    Passkey(Addr, u8),
}

/// The coupling addresses of the pens this helper has coupled or probed. A pairing with one
/// of them that asks the user anything (a confirmation: Just Works; a passkey) is not out of
/// band, and a coupling link has no other legitimate pairing: the pen pairs without the key
/// only in pairing mode, from its other address. So such a request is refused whoever started
/// the pairing (Pair Device of the helper, bluetoothd's own pairing on a new link) and
/// whatever address type the link has (the pen's static coupling address, or the identity
/// address a resolved link reports). Matched by address only.
#[derive(Debug, Default)]
pub struct Guard {
    addrs: Vec<Addr>,
}

impl Guard {
    pub fn watch(&mut self, addr: Addr) {
        if !self.addrs.contains(&addr) {
            self.addrs.push(addr);
        }
    }

    pub fn watches(&self, addr: &Addr) -> bool {
        self.addrs.contains(addr)
    }

    /// Takes the requests out of `ev` and returns the ones to refuse; the rest are left to
    /// bluetoothd's agent.
    pub fn refusals(&self, ev: &mut Events) -> Vec<Refusal> {
        let confirm = std::mem::take(&mut ev.confirm).into_iter().filter(|(a, _)| self.watches(a)).map(|(a, t)| Refusal::Confirm(a, t));
        let passkey = std::mem::take(&mut ev.passkey).into_iter().filter(|(a, _)| self.watches(a)).map(|(a, t)| Refusal::Passkey(a, t));
        confirm.chain(passkey).collect()
    }
}

/// What the management events said about the devices since the last look.
#[derive(Debug, Default, PartialEq)]
pub struct Events {
    /// gave a long term key: a pairing with bonding went through
    pub ltk: Vec<Addr>,
    /// the link was dropped because encryption with the stored key failed ("PIN or key
    /// missing"): the only proof that a bond is dead
    pub dead: Vec<Addr>,
    /// a pairing failed. Not a statement about a bond: on LE this is a failed pairing
    /// (bluetoothd's too), not a failed encryption
    pub pairing_failed: Vec<Addr>,
    /// the link went down for another reason
    pub disconnected: Vec<Addr>,
    /// the kernel asks to confirm a pairing: (address, address type)
    pub confirm: Vec<(Addr, u8)>,
    /// the kernel asks for a pairing's passkey: (address, address type)
    pub passkey: Vec<(Addr, u8)>,
    /// the adapter's settings changed; the last ones
    pub settings: Option<u32>,
    /// the adapter went away
    pub removed: bool,
}

impl Events {
    pub fn note(&mut self, ev: Event) {
        match ev {
            Event::NewLongTermKey { addr } => self.ltk.push(addr),
            Event::Disconnected { addr, reason: btmgmt::DISCONN_AUTH_FAILURE, .. } => self.dead.push(addr),
            Event::Disconnected { addr, .. } => self.disconnected.push(addr),
            Event::AuthFailed { addr, .. } => self.pairing_failed.push(addr),
            Event::UserConfirmRequest { addr, addr_type } => self.confirm.push((addr, addr_type)),
            Event::UserPasskeyRequest { addr, addr_type } => self.passkey.push((addr, addr_type)),
            Event::NewSettings(s) => self.settings = Some(s),
            Event::IndexRemoved => self.removed = true,
            Event::Reply { .. } | Event::Other(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cover::parse_addr;
    use std::collections::VecDeque;

    /// A scripted world: replies to Pair Device, when the key event arrives, what fails.
    #[derive(Default)]
    struct Fake {
        calls: Vec<String>,
        replies: VecDeque<PairReply>,
        /// the long term key arrives with this (1-based) Pair Device
        key_with_pair: Option<usize>,
        /// ... or by itself once this much time has passed (bluetoothd's pairing)
        key_at: Option<Duration>,
        connected: bool,
        /// the link goes down after this many Pair Device
        link_down_after: Option<usize>,
        answers: bool,
        announces: bool,
        key_ok: bool,
        prepare_ok: bool,
        pairs: usize,
        clock: Duration,
        key_in_kernel: bool,
        key_added: bool,
        /// when each write to the cover happened, and which key it carried
        times: Vec<Duration>,
        keys_written: Vec<u8>,
        /// this many connections come up through the pen's private address
        resolved_links: u32,
        resolved_stays: bool,
        /// whether the kernel had the key when each resolved link came up
        key_on_resolved: Vec<bool>,
    }

    impl Fake {
        fn new() -> Fake {
            Fake { answers: true, announces: true, key_ok: true, prepare_ok: true, ..Fake::default() }
        }
        fn call(&mut self, c: &str) {
            self.calls.push(c.to_string());
        }
        fn count(&self, c: &str) -> usize {
            self.calls.iter().filter(|x| *x == c).count()
        }
        fn pos(&self, c: &str) -> Option<usize> {
            self.calls.iter().position(|x| x == c)
        }
        fn key(&self) -> bool {
            self.key_with_pair.is_some_and(|n| self.pairs >= n) || self.key_at.is_some_and(|t| self.clock >= t)
        }
    }

    impl Ops for Fake {
        fn log(&mut self, _msg: &str) {}
        fn pause(&mut self, d: Duration) {
            self.clock += d;
        }
        fn connected(&mut self) -> bool {
            self.connected
        }
        fn add_key(&mut self) -> Result<(), Fail> {
            self.call("add_key");
            if !self.key_ok {
                return Err(Fail::UnsupportedKernel);
            }
            self.key_in_kernel = true;
            self.key_added = true;
            Ok(())
        }
        fn clear_key(&mut self) {
            self.call("clear_key");
            self.key_in_kernel = false;
        }
        fn disconnect(&mut self) {
            assert_eq!(self.count("clear_key"), 0, "the link is dropped before the key is taken back");
            self.call("disconnect");
            self.connected = false;
        }
        fn prepare(&mut self) -> Result<(), Fail> {
            self.call("prepare");
            if self.prepare_ok {
                Ok(())
            } else {
                Err(Fail::Other("HCI socket".into()))
            }
        }
        fn unpair(&mut self) -> Result<(), Fail> {
            self.call("unpair");
            Ok(())
        }
        fn write_key(&mut self) -> Result<(), Fail> {
            assert!(self.key_added, "the kernel gets the key before the cover does");
            self.call("write_key");
            self.times.push(self.clock);
            self.keys_written.push(1);
            Ok(())
        }
        fn connect(&mut self) -> Result<Link, Fail> {
            assert!(!self.connected, "Create Connection on a link that is up");
            self.call("connect");
            self.connected = self.answers;
            if self.resolved_links > 0 && self.answers {
                self.resolved_links -= 1;
                self.key_on_resolved.push(self.key_in_kernel);
                // (the host drops such a link itself after bluetoothd's own pairing fails on it)
                self.connected = self.resolved_stays;
                return Ok(Link::Resolved);
            }
            Ok(if self.answers { Link::Up } else { Link::NoAnswer })
        }
        fn pair(&mut self) -> Result<PairReply, Fail> {
            assert!(self.key_in_kernel, "Pair Device without the key in the kernel");
            // as an earlier build of patch 0023 does: the key is gone after a pairing
            self.key_in_kernel = false;
            self.call("pair");
            self.pairs += 1;
            if self.link_down_after == Some(self.pairs) {
                self.connected = false;
            }
            Ok(self.replies.pop_front().unwrap_or(PairReply::AuthFailed))
        }
        fn key_arrived(&mut self, wait: Duration) -> bool {
            if !self.key() {
                self.clock += wait;
            }
            self.key()
        }
        fn announce(&mut self) -> Result<bool, Fail> {
            self.call("announce");
            Ok(self.announces)
        }
    }

    /// The order that paired three times out of three on the device: the key to the cover
    /// first, without a connection; 2.5 s; the cover asked again; the same key again; then the
    /// connection and, 0.4 s later, Pair Device with the key in the kernel.
    #[test]
    fn a_primed_coupling() {
        let mut f = Fake::new();
        f.replies = [PairReply::Success].into();
        f.key_with_pair = Some(1);
        assert_eq!(couple(&mut f, false), Ok(()));
        assert_eq!(f.calls, ["add_key", "prepare", "write_key", "announce", "write_key", "connect", "add_key", "pair", "clear_key"]);
        assert_eq!(f.times[0], Duration::ZERO, "the priming write");
        assert_eq!(f.times[1], PRIME_WAIT, "the second write, after the wait and the cover's new announcement");
        assert_eq!(f.clock, PRIME_WAIT + KEY_SETTLE);
        assert_eq!(f.keys_written, [1, 1], "the same key in both writes");
    }

    /// A refused pairing: one repeat on the link with the key given to the kernel again, then
    /// the round ends; the next one starts 20 s later with the priming write again.
    #[test]
    fn a_refused_pairing() {
        // the repeat goes through: no second connection
        let mut f = Fake::new();
        f.replies = [PairReply::AuthFailed, PairReply::Success].into();
        f.key_with_pair = Some(2);
        assert_eq!(couple(&mut f, false), Ok(()));
        assert_eq!((f.count("connect"), f.count("write_key"), f.count("pair"), f.count("announce")), (1, 2, 2, 1));
        assert_eq!(f.count("add_key"), 3, "before the coupling and before every Pair Device");
        // refused twice, the link went down: round 2 primes again, 20 s later
        let mut f = Fake::new();
        f.replies = [PairReply::AuthFailed, PairReply::AuthFailed, PairReply::Success].into();
        f.link_down_after = Some(2);
        f.key_with_pair = Some(3);
        assert_eq!(couple(&mut f, false), Ok(()));
        assert_eq!((f.count("connect"), f.count("write_key"), f.count("pair")), (2, 4, 3));
        assert_eq!(f.count("announce"), 3, "round 1 priming, round 2 start, round 2 priming");
        assert!(f.times[2] - f.times[1] >= ROUND_PAUSE, "{:?}", f.times);
        assert_eq!(f.times[3] - f.times[2], PRIME_WAIT);
        assert_eq!(f.keys_written, [1, 1, 1, 1], "the pending key stays");
        // refused to the end: two rounds, two Pair Device each, and that is all
        let mut f = Fake::new();
        f.link_down_after = Some(2);
        assert_eq!(couple(&mut f, false), Err(Fail::Refused));
        assert_eq!(f.count("pair"), (ROUNDS * PAIR_ATTEMPTS) as usize);
        assert_eq!(f.calls.last().unwrap(), "clear_key");
    }

    /// The link stayed up after the refusals: round 2 pairs on it without a second Create
    /// Connection (review item 9); there is no priming on a link that is up.
    #[test]
    fn no_second_connection_on_a_link_that_is_up() {
        let mut f = Fake::new();
        f.replies = [PairReply::AuthFailed, PairReply::AuthFailed, PairReply::Success].into();
        f.key_with_pair = Some(3);
        assert_eq!(couple(&mut f, false), Ok(()));
        assert_eq!((f.count("connect"), f.count("write_key")), (1, 3));
        // a link that is up from the start (no bond, connected)
        let mut f = Fake::new();
        f.connected = true;
        f.replies = [PairReply::Success].into();
        f.key_with_pair = Some(1);
        assert_eq!(couple(&mut f, false), Ok(()));
        assert_eq!((f.count("connect"), f.count("write_key"), f.count("announce")), (0, 1, 0));
    }

    /// Measured: on a link the controller resolved, the pairing's confirm value never matches.
    /// No Pair Device on such a link; the next round connects again.
    #[test]
    fn no_pairing_on_a_resolved_link() {
        let mut f = Fake::new();
        f.resolved_links = 1;
        f.replies = [PairReply::Success].into();
        f.key_with_pair = Some(1);
        assert_eq!(couple(&mut f, false), Ok(()));
        assert_eq!((f.count("connect"), f.count("pair")), (2, 1));
        let first_pair = f.pos("pair").unwrap();
        assert_eq!(f.calls[..first_pair].iter().filter(|c| *c == "connect").count(), 2, "the pairing is on the second link");
        // every link resolved: no pairing at all, and the failure says why
        let mut f = Fake::new();
        f.resolved_links = 5;
        f.resolved_stays = true;
        match couple(&mut f, false) {
            Err(Fail::Other(e)) => assert!(e.contains("private address"), "{e}"),
            other => panic!("{other:?}"),
        }
        assert_eq!(f.count("pair"), 0);
        // fail closed: the link that stayed up is dropped, and only then the key taken back
        assert_eq!(f.calls[f.calls.len() - 2..], ["disconnect", "clear_key"]);
    }

    /// bluetoothd's own pairing on a resolved link must not run as Just Works: the key stays in
    /// the kernel while such a link is up, and is taken back only at the end.
    #[test]
    fn the_key_stays_on_a_resolved_link() {
        let mut f = Fake::new();
        f.resolved_links = 1;
        f.replies = [PairReply::Success].into();
        f.key_with_pair = Some(1);
        assert_eq!(couple(&mut f, false), Ok(()));
        assert_eq!(f.key_on_resolved, [true]);
        assert_eq!(f.count("clear_key"), 1);
        assert_eq!(f.calls.last().unwrap(), "clear_key");
        // the link stays up and every round finds it: the key is in the kernel the whole time
        let mut f = Fake::new();
        f.resolved_links = 1;
        f.resolved_stays = true;
        assert!(couple(&mut f, false).is_err());
        assert_eq!((f.count("clear_key"), f.count("disconnect")), (1, 1));
    }

    /// A failed coupling leaves no link up on which a pairing without the key could run; a
    /// coupling that went through, or one that ended with no link, drops nothing.
    #[test]
    fn a_failed_coupling_drops_its_link() {
        // refused twice per round on a link that stays up
        let mut f = Fake::new();
        assert_eq!(couple(&mut f, false), Err(Fail::Refused));
        assert_eq!(f.calls[f.calls.len() - 2..], ["disconnect", "clear_key"]);
        assert!(!f.connected);
        // bluetoothd's pairing gave the key in the end: the bond is kept, the link too
        let mut f = Fake::new();
        f.replies = [PairReply::Busy].into();
        f.key_at = Some(PRIME_WAIT + KEY_SETTLE + Duration::from_secs(1));
        assert_eq!(couple(&mut f, false), Ok(()));
        assert_eq!(f.count("disconnect"), 0);
        // no answer: there is no link to drop
        let mut f = Fake::new();
        f.answers = false;
        assert_eq!(couple(&mut f, false), Err(Fail::NoAnswer));
        assert_eq!(f.count("disconnect"), 0);
    }

    /// Requests to confirm or enter a passkey for a watched coupling address are refused,
    /// whatever the address type the kernel reports; others go to bluetoothd's agent. The
    /// events that carry a reconnection (a new key, a link going down) are left alone.
    #[test]
    fn the_guard_refuses_pairings_without_the_key() {
        let pen: Addr = [0xC1, 2, 3, 4, 5, 6];
        let mouse = parse_addr("11:22:33:44:55:66").unwrap();
        let mut g = Guard::default();
        let mut e = Events::default();
        e.note(Event::UserConfirmRequest { addr: pen, addr_type: btmgmt::ADDR_LE_RANDOM });
        assert!(g.refusals(&mut e).is_empty(), "nothing is watched yet");
        assert!(e.confirm.is_empty(), "the request is taken out all the same");
        g.watch(pen);
        g.watch(pen);
        for ev in [
            Event::UserConfirmRequest { addr: pen, addr_type: btmgmt::ADDR_LE_RANDOM },
            Event::UserConfirmRequest { addr: pen, addr_type: btmgmt::ADDR_LE_PUBLIC },
            Event::UserPasskeyRequest { addr: pen, addr_type: btmgmt::ADDR_LE_RANDOM },
            Event::UserConfirmRequest { addr: mouse, addr_type: btmgmt::ADDR_LE_RANDOM },
            Event::NewLongTermKey { addr: pen },
            Event::Disconnected { addr: pen, addr_type: btmgmt::ADDR_LE_RANDOM, reason: 0x13 },
        ] {
            e.note(ev);
        }
        assert_eq!(
            g.refusals(&mut e),
            [
                Refusal::Confirm(pen, btmgmt::ADDR_LE_RANDOM),
                Refusal::Confirm(pen, btmgmt::ADDR_LE_PUBLIC),
                Refusal::Passkey(pen, btmgmt::ADDR_LE_RANDOM)
            ]
        );
        assert!(e.confirm.is_empty() && e.passkey.is_empty());
        assert_eq!((e.ltk, e.disconnected), (vec![pen], vec![pen]), "a reconnection's events stay");
    }

    #[test]
    fn the_cover_stops_announcing() {
        // after the priming write: nothing is connected
        let mut f = Fake::new();
        f.announces = false;
        assert!(matches!(couple(&mut f, false), Err(Fail::Other(_))));
        assert_eq!((f.count("connect"), f.count("write_key")), (0, 1));
        assert_eq!(f.calls.last().unwrap(), "clear_key");
    }

    /// bluetoothd pairs on the new link by itself; with the key in the kernel its pairing
    /// can be the one that goes through (review item 2).
    #[test]
    fn a_key_from_bluetoothds_pairing_is_success() {
        // Busy: its pairing is under way, the key comes
        let mut f = Fake::new();
        f.replies = [PairReply::Busy].into();
        f.key_at = Some(PRIME_WAIT + KEY_SETTLE + Duration::from_secs(1));
        assert_eq!(couple(&mut f, false), Ok(()));
        assert_eq!((f.count("pair"), f.count("unpair")), (1, 0));
        // Already Paired with the key event: paired, nothing to unpair
        let mut f = Fake::new();
        f.replies = [PairReply::AuthFailed, PairReply::AlreadyPaired].into();
        f.key_at = Some(PRIME_WAIT + KEY_SETTLE + PAIR_PAUSE + Duration::from_millis(500));
        assert_eq!(couple(&mut f, false), Ok(()));
        assert_eq!(f.count("unpair"), 0, "a bond that was just made is not removed");
        // the key came before our Pair Device was even sent
        let mut f = Fake::new();
        f.key_at = Some(PRIME_WAIT + Duration::from_millis(200));
        assert_eq!(couple(&mut f, false), Ok(()));
        assert_eq!(f.count("pair"), 0);
    }

    /// Already Paired without any key event: keys the kernel kept although BlueZ shows no
    /// bond. They are dropped once, then the pairing runs.
    #[test]
    fn already_paired_without_a_key_event() {
        let mut f = Fake::new();
        f.replies = [PairReply::AlreadyPaired, PairReply::Success].into();
        f.key_with_pair = Some(2);
        assert_eq!(couple(&mut f, false), Ok(()));
        assert_eq!((f.count("unpair"), f.count("pair")), (1, 2));
        // twice: something is wrong, and nothing more is removed
        let mut f = Fake::new();
        f.replies = [PairReply::AlreadyPaired, PairReply::AlreadyPaired].into();
        assert!(matches!(couple(&mut f, false), Err(Fail::Other(_))));
        assert_eq!(f.count("unpair"), 1);
    }

    /// What can fail without the pen fails before the bond is dropped.
    #[test]
    fn checks_come_before_the_unpair() {
        let mut f = Fake::new();
        f.key_ok = false;
        assert_eq!(couple(&mut f, true), Err(Fail::UnsupportedKernel));
        assert_eq!(f.calls, ["add_key"], "the bond is untouched, the cover got nothing");
        let mut f = Fake::new();
        f.prepare_ok = false;
        assert!(matches!(couple(&mut f, true), Err(Fail::Other(_))));
        assert_eq!(f.calls, ["add_key", "prepare", "clear_key"]);
        // when they pass: key and preparation, then the unpair, then the cover
        let mut f = Fake::new();
        f.replies = [PairReply::Success].into();
        f.key_with_pair = Some(1);
        assert_eq!(couple(&mut f, true), Ok(()));
        let p = |c| f.pos(c).unwrap();
        assert!(p("add_key") < p("prepare") && p("prepare") < p("unpair") && p("unpair") < p("write_key") && p("write_key") < p("connect"));
        assert_eq!(f.count("unpair"), 1);
    }

    #[test]
    fn no_answer_and_other_endings() {
        let mut f = Fake::new();
        f.answers = false;
        assert_eq!(couple(&mut f, false), Err(Fail::NoAnswer));
        assert_eq!(f.calls, ["add_key", "prepare", "write_key", "announce", "write_key", "connect", "clear_key"]);
        // "success" without a key (the security manager channel was not up): again, and that
        // does not count as a refusal
        let mut f = Fake::new();
        f.replies = [PairReply::Success, PairReply::Success, PairReply::AuthFailed, PairReply::Success].into();
        f.key_with_pair = Some(4);
        assert_eq!(couple(&mut f, false), Ok(()));
        assert_eq!((f.count("pair"), f.count("connect")), (4, 1));
        for (reply, text) in [(PairReply::Timeout, "no reply"), (PairReply::Other(0x03), "failed")] {
            let mut f = Fake::new();
            f.replies = [reply].into();
            match couple(&mut f, false) {
                Err(Fail::Other(e)) => assert!(e.contains(text), "{e}"),
                other => panic!("{other:?}"),
            }
            assert_eq!(f.calls.last().unwrap(), "clear_key");
        }
    }

    #[test]
    fn pair_replies() {
        use PairReply::*;
        let r = PairReply::from_status;
        assert_eq!(
            [r(0x00), r(0x13), r(0x0A), r(0x05), r(0x02), r(0x0E), r(0x0B)],
            [Success, AlreadyPaired, Busy, AuthFailed, NotConnected, NotConnected, Other(0x0B)]
        );
    }

    /// Only a link dropped for a failed encryption says a bond is dead. A
    /// failed pairing (ours, bluetoothd's) says nothing about it.
    #[test]
    fn only_a_failed_encryption_is_evidence() {
        let a: Addr = [0xC1, 2, 3, 4, 5, 6];
        let mut e = Events::default();
        e.note(Event::AuthFailed { addr: a, addr_type: 2, status: 5 });
        e.note(Event::Disconnected { addr: a, addr_type: 2, reason: 3 });
        e.note(Event::Disconnected { addr: a, addr_type: 2, reason: 0 });
        assert!(e.dead.is_empty());
        assert_eq!((e.pairing_failed.len(), e.disconnected.len()), (1, 2));
        e.note(Event::Disconnected { addr: a, addr_type: 2, reason: btmgmt::DISCONN_AUTH_FAILURE });
        assert_eq!(e.dead, [a]);
        e.note(Event::NewLongTermKey { addr: a });
        e.note(Event::UserConfirmRequest { addr: a, addr_type: 2 });
        e.note(Event::NewSettings(btmgmt::SETTING_LE));
        e.note(Event::Other(0x0B));
        assert_eq!((e.ltk.len(), e.confirm.len(), e.settings, e.removed), (1, 1, Some(btmgmt::SETTING_LE), false));
        e.note(Event::IndexRemoved);
        assert!(e.removed);
    }
}
