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
//! - The key reaches the pen 0.4 to 1 s after it was written to the cover, and the pen takes
//!   the connection before that. A pairing started earlier meets a pen without the key (its
//!   response has no OOB flag) or with the previous one (Confirm value failed). So the first
//!   Pair Device waits, and a refused pairing is repeated with the same key on the same link:
//!   the link stays up after a failed pairing.
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

/// The pen gets the key this long after Output 111 at the latest (measured: 0.4 to 1 s).
pub const KEY_SETTLE: Duration = Duration::from_millis(1000);
/// Between two Pair Device on one link.
pub const PAIR_PAUSE: Duration = Duration::from_millis(1000);
/// Pair Device on one link, at most.
pub const PAIR_ATTEMPTS: u32 = 4;
/// The long term key event comes with the reply of a pairing that went through.
pub const LTK_WAIT: Duration = Duration::from_secs(2);
/// Someone else's pairing is under way (Busy): its key is waited for this long.
pub const BUSY_WAIT: Duration = Duration::from_secs(5);
/// Rounds of one coupling: asking the cover again, writing the key again.
pub const ROUNDS: u32 = crate::cover::COUPLE_ATTEMPTS;
/// Before the cover is asked again.
pub const ROUND_PAUSE: Duration = Duration::from_secs(3);

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

/// Pair Device on a link that is up, until the pen gives its key. Ok(false): refused every
/// time, or the link went down.
fn pair_on_link(ops: &mut dyn Ops) -> Result<bool, Fail> {
    // the key is still on its way to the pen
    ops.pause(KEY_SETTLE);
    let mut unpaired = false;
    for attempt in 0..PAIR_ATTEMPTS {
        if attempt > 0 {
            ops.pause(PAIR_PAUSE);
        }
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
                ops.log(&format!("Pair Device returned without a key (attempt {})", attempt + 1));
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
                if ops.key_arrived(BUSY_WAIT) {
                    return Ok(true);
                }
            }
            PairReply::AuthFailed => {
                ops.log(&format!(
                    "the pen refused the pairing (attempt {} of {PAIR_ATTEMPTS} on this link): the key had not reached it",
                    attempt + 1
                ));
            }
            PairReply::NotConnected => {
                ops.log("the link went down");
                return Ok(false);
            }
            PairReply::Timeout => return Err(Fail::Other("Pair Device: no reply".into())),
            PairReply::Other(s) => return Err(Fail::Other(format!("Pair Device: {}", btmgmt::status_name(s)))),
        }
    }
    Ok(ops.key_arrived(Duration::ZERO))
}

fn rounds(ops: &mut dyn Ops, replace: bool) -> Result<(), Fail> {
    if replace {
        ops.unpair()?;
        ops.log("dropped the old bond");
    }
    for round in 0..ROUNDS {
        if round > 0 {
            ops.log(&format!("asking the cover again, same key (round {} of {ROUNDS})", round + 1));
            ops.pause(ROUND_PAUSE);
            if ops.key_arrived(Duration::ZERO) {
                return Ok(());
            }
            if !ops.announce()? && !ops.connected() {
                return Err(Fail::Other("the pen refused the pairing and the cover does not announce it any more".into()));
            }
        }
        ops.write_key()?;
        // never a second connection on a link that is up
        if ops.connected() {
            ops.log("the link is up: pairing on it");
        } else if !ops.connect()? {
            return Err(Fail::NoAnswer);
        } else {
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
            self.call("write_key");
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

    #[test]
    fn a_plain_coupling() {
        let mut f = Fake::new();
        f.replies = [PairReply::Success].into();
        f.key_with_pair = Some(1);
        assert_eq!(couple(&mut f, false), Ok(()));
        assert_eq!(f.calls, ["add_key", "prepare", "write_key", "connect", "add_key", "pair", "clear_key"]);
        assert!(f.clock >= KEY_SETTLE, "the key is given time to reach the pen before the first Pair Device");
    }

    /// The device after round 5: the first Pair Device meets a pen without the key; the same
    /// key on the same link a second later works. No second connection, no new key.
    #[test]
    fn a_refused_pairing_is_repeated_on_the_same_link() {
        let mut f = Fake::new();
        f.replies = [PairReply::AuthFailed, PairReply::AuthFailed, PairReply::Success].into();
        f.key_with_pair = Some(3);
        assert_eq!(couple(&mut f, false), Ok(()));
        assert_eq!((f.count("connect"), f.count("write_key"), f.count("pair"), f.count("announce")), (1, 1, 3, 0));
        assert_eq!(f.count("add_key"), 4, "the key goes to the kernel again before every Pair Device");
        assert_eq!(f.calls.last().unwrap(), "clear_key");
    }

    /// Refused on the link every time: the cover is asked again and the key written again;
    /// the link is still up, so there is no second Create Connection (review item 9).
    #[test]
    fn then_the_cover_is_asked_again_without_a_second_connection() {
        let mut f = Fake::new();
        f.key_with_pair = Some(PAIR_ATTEMPTS as usize + 2);
        f.replies = std::iter::repeat_n(PairReply::AuthFailed, PAIR_ATTEMPTS as usize + 1).chain([PairReply::Success]).collect();
        assert_eq!(couple(&mut f, false), Ok(()));
        assert_eq!((f.count("connect"), f.count("announce"), f.count("write_key")), (1, 1, 2));
        // refused to the end
        let mut f = Fake::new();
        assert_eq!(couple(&mut f, false), Err(Fail::Refused));
        assert_eq!((f.count("connect"), f.count("pair")), (1, (ROUNDS * PAIR_ATTEMPTS) as usize));
        assert_eq!(f.count("announce"), ROUNDS as usize - 1);
        assert_eq!(f.calls.last().unwrap(), "clear_key");
    }

    /// The link went down after the refusal: the next round connects again.
    #[test]
    fn a_link_that_went_down_is_made_again() {
        let mut f = Fake::new();
        f.replies = [PairReply::AuthFailed, PairReply::Success].into();
        f.link_down_after = Some(1);
        f.key_with_pair = Some(2);
        assert_eq!(couple(&mut f, false), Ok(()));
        assert_eq!((f.count("connect"), f.count("announce"), f.count("write_key")), (2, 1, 2));
        // and when the cover does not announce the pen any more
        let mut f = Fake::new();
        f.link_down_after = Some(1);
        f.announces = false;
        assert!(matches!(couple(&mut f, false), Err(Fail::Other(_))));
        assert_eq!(f.count("connect"), 1);
    }

    /// bluetoothd pairs on the new link by itself; with the key in the kernel its pairing
    /// can be the one that goes through (review item 2).
    #[test]
    fn a_key_from_bluetoothds_pairing_is_success() {
        // Busy: its pairing is under way, the key comes
        let mut f = Fake::new();
        f.replies = [PairReply::Busy].into();
        f.key_at = Some(KEY_SETTLE + Duration::from_secs(1));
        assert_eq!(couple(&mut f, false), Ok(()));
        assert_eq!((f.count("pair"), f.count("unpair")), (1, 0));
        // Already Paired with the key event: paired, nothing to unpair
        let mut f = Fake::new();
        f.replies = [PairReply::AuthFailed, PairReply::AlreadyPaired].into();
        f.key_at = Some(KEY_SETTLE + PAIR_PAUSE + Duration::from_millis(500));
        assert_eq!(couple(&mut f, false), Ok(()));
        assert_eq!(f.count("unpair"), 0, "a bond that was just made is not removed");
        // the key came before our Pair Device was even sent
        let mut f = Fake::new();
        f.key_at = Some(Duration::from_millis(500));
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
        assert_eq!(f.calls, ["add_key", "prepare", "write_key", "connect", "clear_key"]);
        // a link that is up already (no bond, connected): pairing on it
        let mut f = Fake::new();
        f.connected = true;
        f.replies = [PairReply::Success].into();
        f.key_with_pair = Some(1);
        assert_eq!(couple(&mut f, false), Ok(()));
        assert_eq!((f.count("connect"), f.count("write_key")), (0, 1));
        // "success" without a key (the security manager channel was not up): again
        let mut f = Fake::new();
        f.replies = [PairReply::Success, PairReply::Success].into();
        f.key_with_pair = Some(2);
        assert_eq!(couple(&mut f, false), Ok(()));
        assert_eq!(f.count("pair"), 2);
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
