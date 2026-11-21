// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
//! The pen-bonding collection of the Type Cover (vendor page FFF4, application FFF4:0001):
//! how the cover tells the host which pen sits in its slot and how the host hands the pen a
//! key through the cover. docs/research/gaps/pen-vendor-exchange.md section 2.
//!
//!   Feature 112  FFF4:0010 = 1: announce pens with Input 110
//!   Feature 86   FFF4:0003 = host Bluetooth address, least significant byte first; the cover
//!                announces a seated pen at once, and by itself whenever a pen is seated later
//!   Input 110    pen ID (FF0F:0050, 4 B), pen pairing address (FFF4:0002, 6 B), flags
//!   Output 111   pen ID and address as received, flag 0008 as received, FFF4:0014 = the
//!                16 B Temporary Key of the LE legacy OOB pairing
//!
//! Layouts come from the report descriptor. The digitizer (045E:0C52) has the same
//! collection; it is not the cover.

use crate::hid::{self, Descriptor, HidBackend, Kind};

pub const TLC: u32 = hid::usage(0xFFF4, 0x0001);
pub const PEN_ID: u32 = hid::usage(0xFF0F, 0x0050);
pub const PEN_ADDR: u32 = hid::usage(0xFFF4, 0x0002);
pub const HOST_ADDR: u32 = hid::usage(0xFFF4, 0x0003);
pub const FLAG_0008: u32 = hid::usage(0xFFF4, 0x0008);
pub const FLAG_0010: u32 = hid::usage(0xFFF4, 0x0010);
pub const FLAG_0012: u32 = hid::usage(0xFFF4, 0x0012);
pub const KEY: u32 = hid::usage(0xFFF4, 0x0014);

pub const INPUT_SEEN: u8 = 110;
pub const OUTPUT_KEY: u8 = 111;
pub const FEATURE_HOST: u8 = 86;
pub const FEATURE_CAPABILITY: u8 = 112;

/// A Bluetooth address, most significant byte first as it is printed.
pub type Addr = [u8; 6];

/// "AA:BB:CC:DD:EE:FF".
pub fn format_addr(a: &Addr) -> String {
    a.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(":")
}

/// "XX:XX:XX:XX:XX:FF": what logs, D-Bus and files other than the state directory get.
pub fn mask_addr(a: &Addr) -> String {
    format!("XX:XX:XX:XX:XX:{:02X}", a[5])
}

pub fn parse_addr(s: &str) -> Option<Addr> {
    let mut a = [0u8; 6];
    let mut it = s.trim().split(':');
    for b in &mut a {
        let p = it.next()?;
        if p.len() != 2 {
            return None;
        }
        *b = u8::from_str_radix(p, 16).ok()?;
    }
    it.next().is_none().then_some(a)
}

/// A pen the cover announced (Input 110).
#[derive(Clone, Debug, PartialEq)]
pub struct Seen {
    /// FF0F:0050 as it is in the report
    pub pen_id: [u8; 4],
    /// FFF4:0002 as it is in the report (the pen's pairing-mode address, LSB first)
    pub pen_addr: [u8; 6],
    pub flag_0008: bool,
    /// the pen uses the address derived with the host address (Feature 86 was written)
    pub flag_0012: bool,
}

pub struct CoverPen {
    pub node: String,
    desc: Descriptor,
}

/// The covers known to have the pen slot: (bus, vendor, product). The node gets the host's
/// Bluetooth address and the pairing key, and what it reports ends as a trusted bond, so a
/// descriptor that looks right is not enough.
pub const KNOWN_COVERS: &[(u16, u16, u16)] = &[
    // Surface Pro Signature Keyboard with pen slot, as on the Surface Pro 9 (surface-hid, bus "host")
    (0x19, 0x045E, 0x09B0),
];

/// The cover's hidraw node with the pen-bonding collection.
pub fn find(h: &dyn HidBackend) -> Option<CoverPen> {
    h.nodes().into_iter().filter(|n| KNOWN_COVERS.contains(&(n.bus, n.vendor, n.product))).find_map(|n| {
        let desc = hid::parse(&n.descriptor).ok()?;
        let complete = desc.has_application(TLC)
            && desc.find(Kind::Feature, HOST_ADDR, None).is_some_and(|f| f.report_id == FEATURE_HOST)
            && desc.find(Kind::Feature, FLAG_0010, None).is_some_and(|f| f.report_id == FEATURE_CAPABILITY)
            && desc.fields_of(Kind::Input, INPUT_SEEN).any(|f| f.has_usage(PEN_ID))
            && desc.fields_of(Kind::Output, OUTPUT_KEY).any(|f| f.has_usage(KEY));
        complete.then_some(CoverPen { node: n.name, desc })
    })
}

impl CoverPen {
    /// Feature 112 with FFF4:0010 set.
    pub fn capability_report(&self) -> Result<Vec<u8>, String> {
        self.desc.pack(Kind::Feature, FEATURE_CAPABILITY, &[(FLAG_0010, 1)])
    }

    /// Feature 86 with the host address (flag 0009 clear).
    pub fn host_report(&self, host: &Addr) -> Result<Vec<u8>, String> {
        let mut buf = self.desc.pack(Kind::Feature, FEATURE_HOST, &[])?;
        let lsb_first: Vec<u8> = host.iter().rev().copied().collect();
        self.desc.put_bytes(Kind::Feature, FEATURE_HOST, HOST_ADDR, &mut buf, &lsb_first)?;
        Ok(buf)
    }

    /// Length of an Input 110 report with its ID.
    pub fn seen_len(&self) -> usize {
        self.desc.report_len(Kind::Input, INPUT_SEEN)
    }

    /// An input report of the node: Some for Input 110 with a pen in it.
    pub fn parse_seen(&self, data: &[u8]) -> Option<Seen> {
        if data.first() != Some(&INPUT_SEEN) || data.len() < self.seen_len() {
            return None;
        }
        let bytes = |u| self.desc.get_bytes(Kind::Input, INPUT_SEEN, u, data);
        let flag = |u| {
            self.desc.fields_of(Kind::Input, INPUT_SEEN).find(|f| f.has_usage(u)).and_then(|f| f.read(data, self.desc.skip(), 0)) == Some(1)
        };
        let seen = Seen {
            pen_id: bytes(PEN_ID)?.try_into().ok()?,
            pen_addr: bytes(PEN_ADDR)?.try_into().ok()?,
            flag_0008: flag(FLAG_0008),
            flag_0012: flag(FLAG_0012),
        };
        (seen.pen_id != [0; 4]).then_some(seen)
    }

    /// Output 111: the key for the pen the cover announced.
    pub fn key_report(&self, seen: &Seen, tk: &[u8; 16]) -> Result<Vec<u8>, String> {
        let mut buf = self.desc.pack(Kind::Output, OUTPUT_KEY, &[(FLAG_0008, i64::from(seen.flag_0008))])?;
        self.desc.put_bytes(Kind::Output, OUTPUT_KEY, PEN_ID, &mut buf, &seen.pen_id)?;
        self.desc.put_bytes(Kind::Output, OUTPUT_KEY, PEN_ADDR, &mut buf, &seen.pen_addr)?;
        self.desc.put_bytes(Kind::Output, OUTPUT_KEY, KEY, &mut buf, tk)?;
        Ok(buf)
    }
}

const ADDR_MASK: u64 = (1 << 46) - 1;
const ADDR_STATIC: u64 = 0xC000_0000_0000;

fn addr_bits(a: &Addr) -> u64 {
    a.iter().fold(0u64, |v, b| (v << 8) | u64::from(*b))
}

fn bits_addr(q: u64) -> Addr {
    q.to_be_bytes()[2..].try_into().unwrap()
}

/// The part of a pen's coupling address that belongs to the pen alone: 46 bits, one value per
/// pen ID, the same toward every host. The static random address the pen takes connections
/// on from the host it was coupled with through the cover slot is
///
///   coupling address = (H XOR host address) in the low 46 bits, the top two bits set
///
/// (flag 0012 of Input 110 set: the cover was told the host address with Feature 86). The
/// variant without a host address (flag 0012 clear; not seen over the air) is NOT H in the
/// low 46 bits, the top two bits set. So one bond made by a cover coupling (on Windows, or by
/// this helper) gives H: its address XOR the address of the host it was made with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PenHash(u64);

impl PenHash {
    /// H from the address of a bond a cover coupling made with `host`. None for an address
    /// that is not static random, which no cover coupling gives.
    pub fn from_bond(coupled: &Addr, host: &Addr) -> Option<PenHash> {
        let a = addr_bits(coupled);
        (a & ADDR_STATIC == ADDR_STATIC).then_some(PenHash((a ^ addr_bits(host)) & ADDR_MASK))
    }

    /// H from the 16-byte coupling prefix, for a user who has it (`PREFIX_FILE`): the low 46
    /// bits of the first 8 bytes, read little-endian, of SHA-1 over the prefix and the pen ID
    /// reversed.
    pub fn from_prefix(prefix: &[u8; 16], pen_id: &[u8; 4]) -> PenHash {
        let mut msg = prefix.to_vec();
        msg.extend(pen_id.iter().rev());
        let h = sha1(&msg);
        PenHash(u64::from_le_bytes(h[..8].try_into().unwrap()) & ADDR_MASK)
    }

    /// The pen's coupling address toward `host`, or the variant without a host.
    pub fn address(&self, host: Option<&Addr>) -> Addr {
        let q = match host {
            Some(a) => self.0 ^ addr_bits(a),
            None => !self.0,
        };
        bits_addr((q & ADDR_MASK) | ADDR_STATIC)
    }
}

/// The address a seen pen will take connections on from this host.
pub fn seen_address(seen: &Seen, host: &Addr, h: &PenHash) -> Addr {
    h.address(seen.flag_0012.then_some(host))
}

/// The pens' H by pen ID, in the helper's state directory (root only). Lines
/// "<pen ID, 8 hex digits> <H, 12 hex digits>".
pub const HASH_FILE: &str = "pen-coupling";
/// The 16-byte coupling prefix, 32 hex digits, when a user supplies it.
pub const PREFIX_FILE: &str = "/etc/sp9/pen-coupling-prefix";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PenHashes(Vec<([u8; 4], PenHash)>);

fn hex_bytes(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

impl PenHashes {
    /// The file's entries; lines that do not parse are left out.
    pub fn parse(text: &str) -> PenHashes {
        let entries = text
            .lines()
            .filter_map(|l| {
                let mut f = l.split_whitespace();
                let id: [u8; 4] = hex_bytes(f.next()?)?.try_into().ok()?;
                let h = u64::from_str_radix(f.next()?, 16).ok()?;
                (h <= ADDR_MASK && f.next().is_none()).then_some((id, PenHash(h)))
            })
            .collect();
        PenHashes(entries)
    }

    pub fn load(path: &std::path::Path) -> PenHashes {
        std::fs::read_to_string(path).map(|t| PenHashes::parse(&t)).unwrap_or_default()
    }

    pub fn text(&self) -> String {
        self.0.iter().map(|(id, h)| format!("{} {:012x}\n", id.iter().map(|b| format!("{b:02x}")).collect::<String>(), h.0)).collect()
    }

    /// Written through a temporary file that only root can read.
    pub fn save(&self, path: &std::path::Path) -> std::io::Result<()> {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("tmp");
        let _ = std::fs::remove_file(&tmp);
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp)?;
        f.write_all(self.text().as_bytes())?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)
    }

    pub fn get(&self, pen_id: &[u8; 4]) -> Option<PenHash> {
        self.0.iter().find(|(id, _)| id == pen_id).map(|(_, h)| *h)
    }

    /// True when this changed the entries.
    pub fn insert(&mut self, pen_id: [u8; 4], h: PenHash) -> bool {
        match self.0.iter_mut().find(|(id, _)| *id == pen_id) {
            Some(e) if e.1 == h => false,
            Some(e) => {
                e.1 = h;
                true
            }
            None => {
                self.0.push((pen_id, h));
                true
            }
        }
    }

    fn has_hash(&self, h: &PenHash) -> bool {
        self.0.iter().any(|(_, x)| x == h)
    }
}

/// The prefix in `PREFIX_FILE`'s text: 32 hex digits, white space and colons ignored.
pub fn parse_prefix(text: &str) -> Option<[u8; 16]> {
    let digits: String = text.chars().filter(|c| !c.is_whitespace() && *c != ':').collect();
    hex_bytes(&digits)?.try_into().ok()
}

/// The addresses of the pen bonds BlueZ keeps for this host (/var/lib/bluetooth/<host>/<device>/info):
/// a static random address, a long-term key, and the pen's name, appearance (0x03C7) or
/// device ID (045E:0C0F).
pub fn pen_bonds(root: &crate::sys::Root, host: &Addr) -> Vec<Addr> {
    let dir = format!("/var/lib/bluetooth/{}", format_addr(host));
    root.list(&dir)
        .iter()
        .filter_map(|d| {
            let addr = parse_addr(d)?;
            let info = root.read(&format!("{dir}/{d}/info")).ok()?;
            let line = |k: &str| info.lines().any(|l| l.trim().eq_ignore_ascii_case(k));
            let static_random = line("AddressType=static");
            let bonded = info.lines().any(|l| l.trim() == "[LongTermKey]" || l.trim() == "[PeripheralLongTermKey]");
            let pen = line("Appearance=0x03c7")
                || info.lines().any(|l| l.trim().starts_with("Name=") && l.contains("Slim Pen"))
                || (line("Vendor=1118") && line("Product=3087"));
            (static_random && bonded && pen).then_some(addr)
        })
        .collect()
}

/// H of a seated pen learned from this host's pen bonds: a bond under the pen's pairing-mode
/// address is not one from a cover coupling, and one whose H another pen has is that pen's.
/// Ok only when exactly one bond is left; Err with how many were. A bond of another pen that
/// was never seen in the cover (one bonded in pairing mode) cannot be told apart, so what
/// this gives is kept only once the pen answered under it.
pub fn learn_hash(bonds: &[Addr], seen: &Seen, host: &Addr, known: &PenHashes) -> Result<PenHash, usize> {
    let pairing: Addr = std::array::from_fn(|i| seen.pen_addr[5 - i]);
    let left: Vec<PenHash> =
        bonds.iter().filter(|a| **a != pairing).filter_map(|a| PenHash::from_bond(a, host)).filter(|h| !known.has_hash(h)).collect();
    match left.as_slice() {
        [h] => Ok(*h),
        _ => Err(left.len()),
    }
}

/// What the coupling thread knows when the cover announces a pen.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SeatedPen {
    /// a manual request is waiting for this announcement
    pub requested: bool,
    /// the request asked to replace the bond without proof that it is dead
    pub forced: bool,
    /// BlueZ has a bond under the pen's coupling address; None when BlueZ did not answer
    /// (bluetoothd restarting): then nothing is known and nothing is done
    pub bonded: Option<bool>,
    /// BlueZ shows the device connected
    pub connected: bool,
    /// the link to this pen was dropped because encryption with the bond failed (the
    /// controller's "PIN or key missing"), a moment ago: the pen no longer has the key
    pub stale: bool,
    /// a coupling of this pen failed a moment ago
    pub failed_recently: bool,
    /// this pen was coupled a moment ago
    pub coupled_recently: bool,
    /// this pen was probed a moment ago
    pub probed_recently: bool,
    /// the cover was only asked again because the machine resumed
    pub after_resume: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Seated {
    /// no bond: run the coupling
    Couple,
    /// a bond that is known to be dead (or a forced request): drop it, then run the coupling
    Replace,
    /// a bond, not connected, nothing known against it: find out by connecting (the cover
    /// hands the pen a key the kernel never gets). A good bond encrypts the link and stays;
    /// nothing is removed on the way
    Probe,
    /// connected under its bond: it works
    Connected,
    /// leave the pen alone for now
    Skip,
}

/// What to do with a pen the cover announced. A bond is only ever given up when encryption
/// with it has failed (or on a forced request), never because the pen is not connected: a
/// healthy pen can sit in the slot unconnected, and a coupling that gets no answer after the
/// old bond is gone leaves the pen with none.
pub fn on_seated(p: SeatedPen) -> Seated {
    let Some(bonded) = p.bonded else { return Seated::Skip };
    if bonded && p.connected {
        return Seated::Connected;
    }
    if !p.requested && p.failed_recently {
        return Seated::Skip;
    }
    if !bonded {
        return Seated::Couple;
    }
    if p.stale || p.forced {
        return Seated::Replace;
    }
    if p.requested {
        return Seated::Probe;
    }
    if p.coupled_recently || p.probed_recently || p.after_resume {
        return Seated::Skip;
    }
    Seated::Probe
}

/// How long after a failed coupling of a pen the cover is asked again by itself: 1, 2, 5,
/// then every 15 minutes.
pub fn retry_after(failures: u32) -> std::time::Duration {
    let minutes = match failures {
        0 | 1 => 1,
        2 => 2,
        3 => 5,
        _ => 15,
    };
    std::time::Duration::from_secs(minutes * 60)
}

/// How a probe of a bonded pen ended.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Probe {
    /// connected and still up: the stored key encrypted the link
    Connected,
    /// connected, then the link was dropped because encryption failed: the bond is dead
    AuthFailure,
    /// no connection (or it went away for another reason): nothing is known
    NoAnswer,
}

/// What a probe means: the result to show, and whether the bond is now known to be dead.
pub fn after_probe(p: Probe) -> (&'static str, bool) {
    match p {
        Probe::Connected => ("connected", false),
        Probe::AuthFailure => ("stale-bond", true),
        Probe::NoAnswer => ("no-answer", false),
    }
}

/// The keys written to the cover (Output 111) that no pairing has used up yet, by coupling
/// address. The pen pairs with the key it was handed the time before, not the one written a
/// moment before the connection (measured: four couplings with a fresh key each failed with
/// "Confirm value failed", and repeating the same 16 bytes paired at once). So a key that was
/// written stays the key to use until a pairing succeeds with it: a coupling after a probe,
/// or after a failed attempt, writes the same bytes again.
#[derive(Default)]
pub struct PendingKeys(std::collections::HashMap<Addr, [u8; 16]>);

impl PendingKeys {
    /// The key to write for this pen now: the pending one, else `fresh()`, which becomes
    /// pending. The flag says whether it was pending already.
    pub fn key_for(&mut self, addr: &Addr, fresh: impl FnOnce() -> std::io::Result<[u8; 16]>) -> std::io::Result<([u8; 16], bool)> {
        if let Some(k) = self.0.get(addr) {
            return Ok((*k, true));
        }
        let k = fresh()?;
        self.0.insert(*addr, k);
        Ok((k, false))
    }

    /// A pairing went through with the pending key: the pen has used it up.
    pub fn paired(&mut self, addr: &Addr) {
        if let Some(mut k) = self.0.remove(addr) {
            k.fill(0);
        }
    }

    pub fn has(&self, addr: &Addr) -> bool {
        self.0.contains_key(addr)
    }
}

/// SHA-1 (FIPS 180-4); only for `PenHash::from_prefix`.
pub fn sha1(msg: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];
    let mut data = msg.to_vec();
    data.push(0x80);
    while data.len() % 64 != 56 {
        data.push(0);
    }
    data.extend(((msg.len() as u64) * 8).to_be_bytes());
    for block in data.chunks_exact(64) {
        let mut w = [0u32; 80];
        for (i, c) in block.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes(c.try_into().unwrap());
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i / 20 {
                0 => ((b & c) | (!b & d), 0x5A827999),
                1 => (b ^ c ^ d, 0x6ED9EBA1),
                2 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6u32),
            };
            let t = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = t;
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e]) {
            *x = x.wrapping_add(y);
        }
    }
    let mut out = [0u8; 20];
    for (o, x) in out.chunks_exact_mut(4).zip(h) {
        o.copy_from_slice(&x.to_be_bytes());
    }
    out
}

/// The report descriptor of the cover's 045E:09B0 node on this model (two FFF4 collections
/// after the keyboard), for fakes and tests.
pub fn fake_cover_pen_descriptor() -> Vec<u8> {
    const HEX: &str = "05010906a101855c1500250175019508050719e029e78102196d296f75019503810295058103c0\
        06f4ff090aa101090ba10285140902150025ff75089506820201090815002501750195018102090d1500250175019501\
        8102090e15002501750195018102750595018103090c15002564750895018102c0c0\
        06f4ff0901a1010907a1028554060fff0950150025ff7508950482020106f4ff0902150025ff7508950682020109081500\
        2501750195018102750795018101c00905a1028555060fff0950150025ff7508950492020106f4ff0902150025ff750895\
        069202010908150025017501950191027507950191010904150025ff75089508920201c00906a10285560903150025ff75\
        089506b2020109091500250175019501b10275079501b101c00913a102856e060fff0950150025ff750895048202010\
        6f4ff0902150025ff75089506820201090815002501750195018102091115002501750195018102091215002501750195\
        018102750595018101c00915a102856f060fff0950150025ff7508950492020106f4ff0902150025ff7508950692020109\
        08150025017501950191027507950191010914150025ff75089510920201c085730921150025ff75089502b1020916a102\
        857009101500250175019501b10209221500250175019501b10275069501b101c0c0";
    let digits: Vec<u8> = HEX.bytes().filter(u8::is_ascii_hexdigit).collect();
    digits.chunks_exact(2).map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hid::{FakeHid, HidNode};

    fn cover() -> (FakeHid, CoverPen) {
        let h = FakeHid::default();
        h.0.lock().unwrap().nodes.push(HidNode {
            name: "hidraw5".into(),
            bus: 0x19,
            vendor: 0x045E,
            product: 0x09B0,
            hid_name: "Microsoft Surface 045E:09B0".into(),
            descriptor: fake_cover_pen_descriptor(),
        });
        let c = find(&h).unwrap();
        (h, c)
    }

    #[test]
    fn sha1_vectors() {
        let hex = |b: [u8; 20]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
        assert_eq!(hex(sha1(b"abc")), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(hex(sha1(b"")), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(hex(sha1(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")), "84983e441c3bd26ebaae4aa1f95129e5e54670f1");
    }

    /// The coupling address is H XOR the host address with the top two bits set, and a bond
    /// gives H back. (H here is made up.)
    #[test]
    fn coupling_address_and_hash() {
        let h = PenHash(0x2A5A_5A5A_5A5A);
        let host = parse_addr("11:22:33:44:55:66").unwrap();
        // 0x2A5A5A5A5A5A ^ 0x112233445566 = 0x3B78691E0F3C, top two bits set
        assert_eq!(h.address(Some(&host)), bits_addr(0xFB78_691E_0F3C));
        // without a host: NOT H, top two bits set
        assert_eq!(h.address(None), bits_addr(0xD5A5_A5A5_A5A5));
        let addr = h.address(Some(&host));
        assert_eq!(PenHash::from_bond(&addr, &host), Some(h));
        // the same pen toward another host: another address, the same H
        let other: Addr = [0xA0, 0xB0, 0xC0, 0xD0, 0xE0, 0xF0];
        let there = h.address(Some(&other));
        assert_ne!(there, addr);
        assert_eq!(PenHash::from_bond(&there, &other), Some(h));
        // only a static random address comes from a cover coupling
        assert_eq!(PenHash::from_bond(&bits_addr(0x3B78_691E_0F3C), &host), None);

        let seen = Seen { pen_id: [1, 2, 3, 4], pen_addr: [0; 6], flag_0008: true, flag_0012: true };
        assert_eq!(seen_address(&seen, &host, &h), addr);
        assert_eq!(seen_address(&Seen { flag_0012: false, ..seen }, &host, &h), h.address(None));
    }

    /// With a made-up prefix: the low 46 bits of SHA-1(prefix, pen ID reversed) read
    /// little-endian.
    #[test]
    fn hash_from_a_prefix() {
        let prefix: [u8; 16] = std::array::from_fn(|i| i as u8);
        assert_eq!(PenHash::from_prefix(&prefix, &[1, 2, 3, 4]), PenHash(0x285e_ae10_9606));
        assert_eq!(parse_prefix("00010203 04050607\n08:09:0a:0b:0c:0d:0e:0f\n"), Some(prefix));
        assert_eq!(parse_prefix("0001"), None);
        assert_eq!(parse_prefix("zz0102030405060708090a0b0c0d0e0f"), None);
    }

    #[test]
    fn hashes_file() {
        let mut hs = PenHashes::default();
        assert!(hs.insert([1, 2, 3, 4], PenHash(0x1234)));
        assert!(!hs.insert([1, 2, 3, 4], PenHash(0x1234)));
        assert!(hs.insert([5, 6, 7, 8], PenHash(ADDR_MASK)));
        assert_eq!(hs.text(), "01020304 000000001234\n05060708 3fffffffffff\n");
        assert_eq!(PenHashes::parse(&hs.text()), hs);
        assert_eq!(PenHashes::parse("01020304 ffffffffffff\njunk\n0102 12\n"), PenHashes::default(), "out of range, junk");

        let t = crate::sys::FakeTree::new("pen-hashes");
        let path = t.root.p("/var/lib/sp9/pen-coupling");
        assert_eq!(PenHashes::load(&path), PenHashes::default());
        hs.save(&path).unwrap();
        assert_eq!(PenHashes::load(&path).get(&[5, 6, 7, 8]), Some(PenHash(ADDR_MASK)));
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    }

    #[test]
    fn hash_learned_from_a_bond() {
        let t = crate::sys::FakeTree::new("pen-bonds");
        let host = parse_addr("11:22:33:44:55:66").unwrap();
        let h = PenHash(0x2A5A_5A5A_5A5A);
        let coupled = h.address(Some(&host));
        let dir = |a: &Addr| format!("/var/lib/bluetooth/11:22:33:44:55:66/{}/info", format_addr(a));
        let pen = "[General]\nName=Surface Slim Pen 2\nAddressType=static\n\n[LongTermKey]\nAuthenticated=1\n";
        t.file(&dir(&coupled), pen);
        // the pen bonded in pairing mode: its own static address
        let pairing: Addr = [0xC1, 0xC2, 0xC3, 0xC4, 0xC5, 0xC6];
        t.file(&dir(&pairing), "[General]\nAppearance=0x03c7\nAddressType=static\n[LongTermKey]\n");
        // not a pen, not bonded, not static
        let mouse: Addr = [0xD1, 0xD2, 0xD3, 0xD4, 0xD5, 0xD6];
        t.file(&dir(&mouse), "[General]\nName=Mouse\nAddressType=static\n[LongTermKey]\n");
        let unbonded: Addr = [0xE1, 0xE2, 0xE3, 0xE4, 0xE5, 0xE6];
        t.file(&dir(&unbonded), "[General]\nName=Surface Slim Pen 2\nAddressType=static\n");
        let public: Addr = [0x21, 0x22, 0x23, 0x24, 0x25, 0x26];
        t.file(&dir(&public), "[General]\n[DeviceID]\nVendor=1118\nProduct=3087\n[General]\nAddressType=public\n[LongTermKey]\n");
        // another adapter's bonds
        let other_host = format_addr(&[0xA0, 0xB0, 0xC0, 0xD0, 0xE0, 0xF0]);
        t.file(&format!("/var/lib/bluetooth/{other_host}/{}/info", format_addr(&[0xF1, 0xF2, 0xF3, 0xF4, 0xF5, 0xF6])), pen);

        let bonds = pen_bonds(&t.root, &host);
        assert_eq!(bonds, vec![pairing, coupled]);
        let seen = Seen { pen_id: [1, 2, 3, 4], pen_addr: [0xC6, 0xC5, 0xC4, 0xC3, 0xC2, 0xC1], flag_0008: true, flag_0012: true };
        assert_eq!(learn_hash(&bonds, &seen, &host, &PenHashes::default()), Ok(h));
        // a second pen: the bond whose H the first one has is not its
        let mut known = PenHashes::default();
        known.insert([1, 2, 3, 4], h);
        let second = Seen { pen_id: [9, 9, 9, 9], pen_addr: [1; 6], ..seen.clone() };
        assert_eq!(learn_hash(&[coupled], &second, &host, &known), Err(0));
        assert_eq!(learn_hash(&[], &seen, &host, &PenHashes::default()), Err(0));
        let other = PenHash(0x0101_0101_0101).address(Some(&host));
        assert_eq!(learn_hash(&[coupled, other], &seen, &host, &PenHashes::default()), Err(2));
    }

    #[test]
    fn addresses_are_masked() {
        let a = parse_addr("11:22:33:44:55:66").unwrap();
        assert_eq!(mask_addr(&a), "XX:XX:XX:XX:XX:66");
        assert_eq!(format_addr(&a), "11:22:33:44:55:66");
        assert!(parse_addr("11:22:33:44:55").is_none() && parse_addr("11:22:33:44:55:66:77").is_none());
        assert!(parse_addr("1:22:33:44:55:66").is_none() && parse_addr("zz:22:33:44:55:66").is_none());
    }

    /// The bytes private coupling tests wrote by hand: Feature 112 = [112, 1], Feature 86 =
    /// [86, host LSB first, 0], Output 111 = [111, pen ID, pen address, flag & 1, key].
    #[test]
    fn reports_from_the_real_descriptor() {
        let (_, c) = cover();
        assert_eq!(c.node, "hidraw5");
        assert_eq!(c.capability_report().unwrap(), vec![112, 1]);
        let host = parse_addr("11:22:33:44:55:66").unwrap();
        assert_eq!(c.host_report(&host).unwrap(), vec![86, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11, 0]);
        assert_eq!(c.seen_len(), 12);

        let input = [110, 1, 2, 3, 4, 0xA6, 0xA5, 0xA4, 0xA3, 0xA2, 0xA1, 0x07];
        let seen = c.parse_seen(&input).unwrap();
        assert_eq!(seen, Seen { pen_id: [1, 2, 3, 4], pen_addr: [0xA6, 0xA5, 0xA4, 0xA3, 0xA2, 0xA1], flag_0008: true, flag_0012: true });
        // after one Output 111 the cover reports flags 05: 0008 and 0012 set, 0011 clear
        let again = c.parse_seen(&[110, 1, 2, 3, 4, 0xA6, 0xA5, 0xA4, 0xA3, 0xA2, 0xA1, 0x05]).unwrap();
        assert!(again.flag_0008 && again.flag_0012);
        assert!(!c.parse_seen(&[110, 1, 2, 3, 4, 0xA6, 0xA5, 0xA4, 0xA3, 0xA2, 0xA1, 0x02]).unwrap().flag_0012);

        let tk: [u8; 16] = std::array::from_fn(|i| 0x10 + i as u8);
        let mut want = vec![111, 1, 2, 3, 4, 0xA6, 0xA5, 0xA4, 0xA3, 0xA2, 0xA1, 0x01];
        want.extend(tk);
        assert_eq!(c.key_report(&seen, &tk).unwrap(), want);
        assert_eq!(c.key_report(&Seen { flag_0008: false, ..seen }, &tk).unwrap()[11], 0);
    }

    #[test]
    fn what_to_do_with_a_seated_pen() {
        let p = SeatedPen { bonded: Some(false), ..SeatedPen::default() };
        assert_eq!(on_seated(p), Seated::Couple, "no bond");
        // BlueZ did not answer: nothing is known, nothing is done, whoever asks (review item 1)
        let unknown = SeatedPen::default();
        assert_eq!(on_seated(unknown), Seated::Skip);
        assert_eq!(on_seated(SeatedPen { requested: true, forced: true, stale: true, ..unknown }), Seated::Skip);
        // not bonded but connected (a link without keys): couple on that link
        assert_eq!(on_seated(SeatedPen { connected: true, ..p }), Seated::Couple);
        assert_eq!(on_seated(SeatedPen { after_resume: true, ..p }), Seated::Couple);
        assert_eq!(on_seated(SeatedPen { failed_recently: true, ..p }), Seated::Skip);
        assert_eq!(on_seated(SeatedPen { failed_recently: true, requested: true, ..p }), Seated::Couple);

        let bonded = SeatedPen { bonded: Some(true), ..p };
        // connected: it works, whatever else is known and whoever asks
        let connected = SeatedPen { connected: true, ..bonded };
        assert_eq!(on_seated(connected), Seated::Connected);
        assert_eq!(on_seated(SeatedPen { requested: true, ..connected }), Seated::Connected);
        assert_eq!(on_seated(SeatedPen { requested: true, forced: true, stale: true, ..connected }), Seated::Connected);
        // not connected is not a reason to give the bond up: probe
        assert_eq!(on_seated(bonded), Seated::Probe);
        assert_eq!(on_seated(SeatedPen { requested: true, ..bonded }), Seated::Probe);
        assert_eq!(on_seated(SeatedPen { requested: true, coupled_recently: true, probed_recently: true, ..bonded }), Seated::Probe);
        for quiet in [
            SeatedPen { coupled_recently: true, ..bonded },
            SeatedPen { probed_recently: true, ..bonded },
            SeatedPen { after_resume: true, ..bonded },
        ] {
            assert_eq!(on_seated(quiet), Seated::Skip, "{quiet:?}");
        }
        // only a failed encryption (or a forced request) replaces a bond
        assert_eq!(on_seated(SeatedPen { stale: true, ..bonded }), Seated::Replace);
        assert_eq!(on_seated(SeatedPen { stale: true, probed_recently: true, ..bonded }), Seated::Replace);
        assert_eq!(on_seated(SeatedPen { stale: true, failed_recently: true, ..bonded }), Seated::Skip);
        assert_eq!(on_seated(SeatedPen { stale: true, failed_recently: true, requested: true, ..bonded }), Seated::Replace);
        assert_eq!(on_seated(SeatedPen { requested: true, forced: true, ..bonded }), Seated::Replace);
    }

    #[test]
    fn a_written_key_stays_until_a_pairing_used_it() {
        let a: Addr = [0xC1, 2, 3, 4, 5, 6];
        let b: Addr = [0xC1, 2, 3, 4, 5, 7];
        let mut keys = PendingKeys::default();
        assert!(!keys.has(&a));
        // a probe writes a key: it is pending
        assert_eq!(keys.key_for(&a, || Ok([1; 16])).unwrap(), ([1; 16], false));
        assert!(keys.has(&a));
        // the coupling after it, and every retry, write the same bytes; nothing new is made
        for _ in 0..3 {
            assert_eq!(keys.key_for(&a, || panic!("a pending key is reused")).unwrap(), ([1; 16], true));
        }
        // another pen has its own
        assert_eq!(keys.key_for(&b, || Ok([2; 16])).unwrap(), ([2; 16], false));
        // only a pairing clears it; the next coupling starts with a new key
        keys.paired(&a);
        assert!(!keys.has(&a) && keys.has(&b));
        assert_eq!(keys.key_for(&a, || Ok([3; 16])).unwrap(), ([3; 16], false));
        // no random data: nothing becomes pending
        keys.paired(&a);
        assert!(keys.key_for(&a, || Err(std::io::Error::other("no random data"))).is_err());
        assert!(!keys.has(&a));
    }

    #[test]
    fn retry_interval_grows() {
        let m = |n| retry_after(n).as_secs() / 60;
        assert_eq!([m(0), m(1), m(2), m(3), m(4), m(20)], [1, 1, 2, 5, 15, 15]);
    }

    #[test]
    fn what_a_probe_means() {
        assert_eq!(after_probe(Probe::Connected), ("connected", false));
        assert_eq!(after_probe(Probe::AuthFailure), ("stale-bond", true));
        assert_eq!(after_probe(Probe::NoAnswer), ("no-answer", false), "no answer keeps the bond");
    }

    #[test]
    fn other_reports_are_not_a_pen() {
        let (_, c) = cover();
        assert!(c.parse_seen(&[92, 0, 0]).is_none(), "keyboard report");
        assert!(c.parse_seen(&[84, 1, 2, 3, 4, 0, 0, 0, 0, 0, 0, 1]).is_none(), "Input 84");
        assert!(c.parse_seen(&[110, 1, 2, 3]).is_none(), "short");
        assert!(c.parse_seen(&[110, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]).is_none(), "no pen");
    }

    /// The descriptor alone does not make a node the cover.
    #[test]
    fn only_known_covers() {
        for (bus, vendor, product) in [(0x03, 0x045E, 0x09B0), (0x19, 0x1234, 0x09B0), (0x19, 0x045E, 0x09AF), (0x05, 0x045E, 0x09B0)] {
            let h = FakeHid::default();
            h.0.lock().unwrap().nodes.push(HidNode {
                name: "hidraw9".into(),
                bus,
                vendor,
                product,
                hid_name: "looks like a cover".into(),
                descriptor: fake_cover_pen_descriptor(),
            });
            assert!(find(&h).is_none(), "{bus:#x} {vendor:#x} {product:#x}");
        }
    }

    #[test]
    fn the_digitizer_is_not_the_cover() {
        let h = FakeHid::default();
        h.0.lock().unwrap().nodes.push(HidNode {
            name: "hidraw7".into(),
            bus: 1,
            vendor: 0x045E,
            product: crate::pen_feedback::DIGITIZER_PRODUCT,
            hid_name: "Intel Touch Host Controller".into(),
            descriptor: fake_cover_pen_descriptor(),
        });
        assert!(find(&h).is_none());
    }
}
