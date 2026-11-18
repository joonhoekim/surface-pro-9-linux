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
//!   pairing of its own on a new link about a second after the connection, and with a kernel
//!   that forgets the key once it was used that pairing would use it up.
//! - A refused pairing gets one repeat on the link, then the round ends; the next round
//!   starts 20 s later with the priming write again (rounds a few seconds apart all failed,
//!   an attempt 25 s later went through).
//! - The kernel (patch 0023 as revised) forgets the key once a pairing used it: it is given
//!   again before every Pair Device. An older build keeps it; giving it again does no harm.
//! - bluetoothd starts a pairing of its own on a new link about a second after the
//!   connection, and with the key in the kernel that one is a legacy OOB pairing too. A long
//!   term key from either pairing is success. Already Paired or Busy from Pair Device after
//!   that is not a reason to unpair: unpairing a device bluetoothd has paired makes it drop
//!   the link, and after a link the host dropped the pen is silent for half a minute.
//! - There is never a second Create Connection on a link that is up.

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
    /// What can fail without the pen: pack the cover report, open the raw HCI socket.
    fn prepare(&mut self) -> Result<(), Fail>;
    /// Drop the keys the kernel has for the pen (MGMT Unpair Device, disconnect = 0).
    fn unpair(&mut self) -> Result<(), Fail>;
    /// The key to the cover (Output 111).
    fn write_key(&mut self) -> Result<(), Fail>;
    /// LE Create Connection; false when the pen takes none.
    fn connect(&mut self) -> Result<bool, Fail>;
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
        // the kernel forgets the key once a pairing used it
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
            ops.log("the link is up: pairing on it");
            ops.write_key()?;
        } else {
            // the pen takes the key only while it is not connected, and not at once: hand it over,
            // wait, ask the cover again, write the same key again, then connect
            ops.log("handing the key to the pen through the cover (no connection yet)");
            ops.write_key()?;
            ops.pause(PRIME_WAIT);
            if !ops.announce()? {
                return Err(Fail::Other("the cover does not announce the pen any more".into()));
            }
            ops.write_key()?;
            if !ops.connect()? {
                return Err(Fail::NoAnswer);
            }
            ops.log("connected, pairing");
        }
        if pair_on_link(ops)? {
            return Ok(());
        }
    }
    Err(Fail::Refused)
}

/// One coupling of the pen the cover announced. `replace`: the pen has a bond that is known
/// to be dead (or the request was forced); it is dropped once nothing but the pen can fail.
/// The key is taken back from the kernel however it ends.
pub fn couple(ops: &mut dyn Ops, replace: bool) -> Result<(), Fail> {
    // the first Add Remote OOB Data also tells whether this kernel takes a key at all
    ops.add_key()?;
    let r = match ops.prepare() {
        Ok(()) => rounds(ops, replace),
        Err(e) => Err(e),
    };
    ops.clear_key();
    r
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
        fn connect(&mut self) -> Result<bool, Fail> {
            assert!(!self.connected, "Create Connection on a link that is up");
            self.call("connect");
            self.connected = self.answers;
            Ok(self.answers)
        }
        fn pair(&mut self) -> Result<PairReply, Fail> {
            assert!(self.key_in_kernel, "Pair Device without the key in the kernel");
            // the kernel forgets the key once a pairing used it
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

    /// Review item 3: what can fail without the pen fails before the bond is dropped.
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

    /// Review item 4: only a link dropped for a failed encryption says a bond is dead. A
    /// failed pairing (ours, bluetoothd's) says nothing about it.
    #[test]
    fn only_a_failed_encryption_is_evidence() {
        let a = parse_addr("ED:08:D9:17:E1:E7").unwrap();
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
