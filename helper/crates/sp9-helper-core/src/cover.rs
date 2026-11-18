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
use crate::pen_feedback::DIGITIZER_PRODUCT;

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

/// The cover's hidraw node with the pen-bonding collection.
pub fn find(h: &dyn HidBackend) -> Option<CoverPen> {
    h.nodes().into_iter().filter(|n| n.product != DIGITIZER_PRODUCT).find_map(|n| {
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

/// The constant of Windows' SurfacePenBleLcAddrAdaptationDriver (.rdata).
const COUPLED_CONST: [u8; 16] = [0xf8, 0x76, 0xaf, 0x01, 0x2e, 0x1c, 0x28, 0x40, 0x91, 0x8f, 0x6f, 0x60, 0x5e, 0x6b, 0x1f, 0xd6];
const ADDR_MASK: u64 = (1 << 46) - 1;
const ADDR_STATIC: u64 = 0xC000_0000_0000;

/// The static random address the pen uses toward the host it is coupled with through the
/// cover slot (as userspace/pen/surface_pen/col07.py `coupled_address`): SHA-1 of the driver
/// constant and the pen ID reversed, the low 46 bits of its first 8 bytes read little-endian,
/// the top two bits set; XOR the host address, or inverted for the variant without a host
/// (flag 0012 clear).
pub fn coupled_address(pen_id: &[u8; 4], host: Option<&Addr>) -> Addr {
    let mut msg = COUPLED_CONST.to_vec();
    msg.extend(pen_id.iter().rev());
    let h = sha1(&msg);
    let mut q = (u64::from_le_bytes(h[..8].try_into().unwrap()) & ADDR_MASK) | ADDR_STATIC;
    q = match host {
        Some(a) => q ^ a.iter().fold(0u64, |v, b| (v << 8) | *b as u64),
        None => !q,
    };
    q = (q & ADDR_MASK) | ADDR_STATIC;
    q.to_be_bytes()[2..].try_into().unwrap()
}

/// The address a seen pen will take connections on from this host.
pub fn seen_address(seen: &Seen, host: &Addr) -> Addr {
    coupled_address(&seen.pen_id, seen.flag_0012.then_some(host))
}

/// What the coupling thread knows when the cover announces a pen.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SeatedPen {
    /// a manual request is waiting for this announcement
    pub requested: bool,
    /// the request asked to replace the bond without proof that it is dead
    pub forced: bool,
    /// BlueZ has a bond under the pen's coupling address
    pub bonded: bool,
    /// BlueZ shows the device connected
    pub connected: bool,
    /// encryption with that bond failed since it was made: the pen no longer has the key
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
    if p.bonded && p.connected {
        return Seated::Connected;
    }
    if !p.requested && p.failed_recently {
        return Seated::Skip;
    }
    if !p.bonded {
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

/// SHA-1 (FIPS 180-4); only for the address derivation above.
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

    /// userspace/pen/tests/test_reports.py test_coupled_address (checked against a pen
    /// coupled by Windows).
    #[test]
    fn coupled_address_vectors() {
        let pid = [1, 2, 3, 4];
        let host = parse_addr("11:22:33:44:55:66").unwrap();
        assert_eq!(format_addr(&coupled_address(&pid, Some(&host))), "ED:08:D9:17:E1:E7");
        assert_eq!(format_addr(&coupled_address(&pid, None)), "C3:D5:15:AC:4B:7E");
        assert_eq!(coupled_address(&pid, None)[0] >> 6, 3, "static random");
        let seen = Seen { pen_id: pid, pen_addr: [0; 6], flag_0008: true, flag_0012: true };
        assert_eq!(format_addr(&seen_address(&seen, &host)), "ED:08:D9:17:E1:E7");
        assert_eq!(format_addr(&seen_address(&Seen { flag_0012: false, ..seen }, &host)), "C3:D5:15:AC:4B:7E");
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
        let p = SeatedPen::default();
        assert_eq!(on_seated(p), Seated::Couple, "no bond");
        assert_eq!(on_seated(SeatedPen { after_resume: true, ..p }), Seated::Couple);
        assert_eq!(on_seated(SeatedPen { failed_recently: true, ..p }), Seated::Skip);
        assert_eq!(on_seated(SeatedPen { failed_recently: true, requested: true, ..p }), Seated::Couple);

        let bonded = SeatedPen { bonded: true, ..p };
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

    #[test]
    fn the_digitizer_is_not_the_cover() {
        let h = FakeHid::default();
        h.0.lock().unwrap().nodes.push(HidNode {
            name: "hidraw7".into(),
            bus: 1,
            vendor: 0x045E,
            product: DIGITIZER_PRODUCT,
            hid_name: "Intel Touch Host Controller".into(),
            descriptor: fake_cover_pen_descriptor(),
        });
        assert!(find(&h).is_none());
    }
}
