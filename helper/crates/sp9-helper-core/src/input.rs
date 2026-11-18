// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
//! Input (docs/research/control/input.md 4):
//!
//! - Type Cover keyboard backlight: an LED class device `*::kbd_backlight` when the kernel
//!   has one (brightness 0..max_brightness, key presses via brightness_hw_changed). Otherwise
//!   the hidraw node: keyboard collection 000C:0007 has Output 213 with 000C:007B (Set Level,
//!   0..12), Feature 213 with the supported levels (000C:0517, this cover `00 03 06 0C`),
//!   Feature 214 with the current level and Input 213 with the backlight key (000C:0515; the
//!   firmware changes the level by itself, typecover.md 6.1).
//! - Touch off while the panel is off: Windows drops input while the display is off; Linux has
//!   no such policy. The helper sets the input core's `inhibited` on iptsd's virtual
//!   touchscreen while the eDP connector is off.
//! - iptsd thresholds: a drop-in in /etc/iptsd.d loaded after the platform preset.

use crate::hid::{self, HidBackend, Kind};
use crate::sys::{self, Root};
use serde::{Deserialize, Serialize};
use std::io;
use std::sync::{Arc, Mutex};

pub const BACKLIGHT_SET_LEVEL: u32 = hid::usage(0x0C, 0x7B);
pub const BACKLIGHT_LEVEL_LIST: u32 = hid::usage(0x0C, 0x517);
/// Input 213: the backlight key
pub const BACKLIGHT_KEY: u32 = hid::usage(0x0C, 0x515);
/// What this cover reported on Windows when Feature 213 cannot be read.
pub const DEFAULT_LEVELS: [u8; 4] = [0, 3, 6, 12];

pub struct Kbd {
    pub node: String,
    desc: hid::Descriptor,
    out_id: u8,
}

/// The hidraw node with a keyboard backlight Set Level output.
pub fn find_kbd(h: &dyn HidBackend) -> Option<Kbd> {
    h.nodes().into_iter().find_map(|n| {
        let desc = hid::parse(&n.descriptor).ok()?;
        let out_id = desc.find(Kind::Output, BACKLIGHT_SET_LEVEL, None)?.report_id;
        Some(Kbd { node: n.name, desc, out_id })
    })
}

impl Kbd {
    fn feature(&self, h: &dyn HidBackend, u: u32) -> Option<(hid::Field, Vec<u8>)> {
        let f = self.desc.find(Kind::Feature, u, None)?.clone();
        let data = h.get_feature(&self.node, f.report_id, self.desc.report_len(Kind::Feature, f.report_id)).ok()?;
        Some((f, data))
    }

    /// Supported levels, ascending, 0 included.
    pub fn levels(&self, h: &dyn HidBackend) -> Vec<u8> {
        let mut v: Vec<u8> = match self.feature(h, BACKLIGHT_LEVEL_LIST) {
            Some((f, data)) => (0..f.count).filter_map(|k| f.read(&data, self.desc.skip(), k)).map(|x| x as u8).collect(),
            None => DEFAULT_LEVELS.to_vec(),
        };
        v.push(0);
        v.sort();
        v.dedup();
        if v.len() < 2 {
            return DEFAULT_LEVELS.to_vec();
        }
        v
    }

    pub fn level(&self, h: &dyn HidBackend) -> Option<u8> {
        let (f, data) = self.feature(h, BACKLIGHT_SET_LEVEL)?;
        f.read(&data, self.desc.skip(), 0).map(|x| x as u8)
    }

    pub fn set_level(&self, h: &dyn HidBackend, level: u8) -> io::Result<()> {
        if !self.levels(h).contains(&level) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("level {level} not supported by the cover")));
        }
        let r = self
            .desc
            .pack(Kind::Output, self.out_id, &[(BACKLIGHT_SET_LEVEL, level as i64)])
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        h.write_output(&self.node, &r)
    }

    /// An input report from the node is a backlight key press.
    pub fn is_key_report(&self, report: &[u8]) -> bool {
        let Some(f) = self.desc.find(Kind::Input, BACKLIGHT_KEY, None) else { return false };
        if self.desc.uses_ids && report.first() != Some(&f.report_id) {
            return false;
        }
        f.read(report, self.desc.skip(), 0).is_some_and(|v| v != 0)
    }
}

pub const SYS_LEDS: &str = "/sys/class/leds";
pub const KBD_LED_SUFFIX: &str = "::kbd_backlight";

/// LED class devices named `*::kbd_backlight`.
pub trait LedBackend: Send + Sync {
    fn kbd_leds(&self) -> Vec<String>;
    /// brightness, max_brightness
    fn attr(&self, name: &str, attr: &str) -> Option<u32>;
    fn set_brightness(&self, name: &str, v: u32) -> io::Result<()>;
}

pub struct SysfsLeds {
    root: Root,
}

impl SysfsLeds {
    pub fn new(root: Root) -> Self {
        SysfsLeds { root }
    }
}

fn led_name_ok(name: &str) -> bool {
    !name.contains('/') && name.ends_with(KBD_LED_SUFFIX)
}

impl LedBackend for SysfsLeds {
    fn kbd_leds(&self) -> Vec<String> {
        self.root.list(SYS_LEDS).into_iter().filter(|n| led_name_ok(n)).collect()
    }
    fn attr(&self, name: &str, attr: &str) -> Option<u32> {
        if !led_name_ok(name) {
            return None;
        }
        self.root.read_i64(&format!("{SYS_LEDS}/{name}/{attr}")).and_then(|v| u32::try_from(v).ok())
    }
    fn set_brightness(&self, name: &str, v: u32) -> io::Result<()> {
        if !led_name_ok(name) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "bad LED name"));
        }
        self.root.write(&format!("{SYS_LEDS}/{name}/brightness"), &v.to_string())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FakeLed {
    pub name: String,
    pub brightness: u32,
    pub max: u32,
}

#[derive(Clone, Default)]
pub struct FakeLeds(pub Arc<Mutex<Vec<FakeLed>>>);

impl LedBackend for FakeLeds {
    fn kbd_leds(&self) -> Vec<String> {
        self.0.lock().unwrap().iter().map(|l| l.name.clone()).filter(|n| led_name_ok(n)).collect()
    }
    fn attr(&self, name: &str, attr: &str) -> Option<u32> {
        let s = self.0.lock().unwrap();
        let l = s.iter().find(|l| l.name == name)?;
        match attr {
            "brightness" => Some(l.brightness),
            "max_brightness" => Some(l.max),
            _ => None,
        }
    }
    fn set_brightness(&self, name: &str, v: u32) -> io::Result<()> {
        let mut s = self.0.lock().unwrap();
        let l = s.iter_mut().find(|l| l.name == name).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no LED"))?;
        l.brightness = v.min(l.max);
        Ok(())
    }
}

/// The keyboard backlight: the kernel's LED when there is one, else the cover's hidraw node.
pub enum KbdBacklight {
    Led { name: String, max: u32 },
    Hid(Kbd),
}

pub fn find_backlight(leds: &dyn LedBackend, h: &dyn HidBackend) -> Option<KbdBacklight> {
    leds.kbd_leds()
        .into_iter()
        .find_map(|name| {
            let max = leds.attr(&name, "max_brightness").filter(|m| *m > 0)?;
            Some(KbdBacklight::Led { name, max })
        })
        .or_else(|| find_kbd(h).map(KbdBacklight::Hid))
}

impl KbdBacklight {
    /// "led:<name>" or "hidraw:<node>"
    pub fn backend(&self) -> String {
        match self {
            KbdBacklight::Led { name, .. } => format!("led:{name}"),
            KbdBacklight::Hid(k) => format!("hidraw:{}", k.node),
        }
    }

    /// The values `set_level` takes, ascending, 0 first: 0..=max_brightness for the LED, the
    /// cover's levels (0 3 6 12) for hidraw.
    pub fn levels(&self, h: &dyn HidBackend) -> Vec<u32> {
        match self {
            KbdBacklight::Led { max, .. } => (0..=*max).collect(),
            KbdBacklight::Hid(k) => k.levels(h).into_iter().map(u32::from).collect(),
        }
    }

    pub fn level(&self, leds: &dyn LedBackend, h: &dyn HidBackend) -> Option<u32> {
        match self {
            KbdBacklight::Led { name, .. } => leds.attr(name, "brightness"),
            KbdBacklight::Hid(k) => k.level(h).map(u32::from),
        }
    }

    pub fn set_level(&self, leds: &dyn LedBackend, h: &dyn HidBackend, v: u32) -> io::Result<()> {
        match self {
            KbdBacklight::Led { name, max } => {
                if v > *max {
                    return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("level {v} above max_brightness {max}")));
                }
                leds.set_brightness(name, v)
            }
            KbdBacklight::Hid(k) => {
                let v = u8::try_from(v).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, format!("level {v} not supported")))?;
                k.set_level(h, v)
            }
        }
    }
}

pub const SYS_INPUT: &str = "/sys/class/input";
/// iptsd's uinput device for touch (iptsd, src/apps/daemon/touch.hpp).
pub const TOUCHSCREEN_NAME: &str = "IPTSD Virtual Touchscreen";

#[derive(Clone, Debug, PartialEq)]
pub struct InputDev {
    /// "input17"
    pub id: String,
    pub name: String,
    pub inhibited: Option<bool>,
}

pub trait InputBackend: Send + Sync {
    fn devices(&self) -> Vec<InputDev>;
    fn set_inhibited(&self, id: &str, on: bool) -> io::Result<()>;
}

pub fn touchscreens(i: &dyn InputBackend) -> Vec<InputDev> {
    i.devices().into_iter().filter(|d| d.name.starts_with(TOUCHSCREEN_NAME)).collect()
}

pub struct SysfsInput {
    root: Root,
}

impl SysfsInput {
    pub fn new(root: Root) -> Self {
        SysfsInput { root }
    }
}

impl InputBackend for SysfsInput {
    fn devices(&self) -> Vec<InputDev> {
        self.root
            .list(SYS_INPUT)
            .into_iter()
            .filter(|n| n.starts_with("input"))
            .filter_map(|n| {
                let name = self.root.read_opt(&format!("{SYS_INPUT}/{n}/name"))?;
                let inhibited = self.root.read_i64(&format!("{SYS_INPUT}/{n}/inhibited")).map(|v| v != 0);
                Some(InputDev { id: n, name, inhibited })
            })
            .collect()
    }

    fn set_inhibited(&self, id: &str, on: bool) -> io::Result<()> {
        if id.contains('/') || !id.starts_with("input") {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "bad input device"));
        }
        self.root.write(&format!("{SYS_INPUT}/{id}/inhibited"), if on { "1" } else { "0" })
    }
}

#[derive(Clone, Debug, Default)]
pub struct FakeInputState {
    pub devices: Vec<InputDev>,
}

#[derive(Clone, Default)]
pub struct FakeInput(pub Arc<Mutex<FakeInputState>>);

impl InputBackend for FakeInput {
    fn devices(&self) -> Vec<InputDev> {
        self.0.lock().unwrap().devices.clone()
    }
    fn set_inhibited(&self, id: &str, on: bool) -> io::Result<()> {
        let mut s = self.0.lock().unwrap();
        let d = s.devices.iter_mut().find(|d| d.id == id).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no device"))?;
        d.inhibited = Some(on);
        Ok(())
    }
}

pub const IPTSD_DROPIN: &str = "/etc/iptsd.d/zz-sp9-helper.conf";

/// iptsd values the helper sets on top of the preset (userspace/platform/iptsd: 24/20,
/// DisableOnStylus = true). None = the preset's value.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Iptsd {
    pub activation_threshold: Option<f64>,
    pub deactivation_threshold: Option<f64>,
    pub disable_on_stylus: Option<bool>,
}

impl Iptsd {
    pub fn is_empty(&self) -> bool {
        *self == Iptsd::default()
    }

    pub fn validate(&self) -> Result<(), String> {
        let ok = |v: f64| v.is_finite() && (1.0..=100.0).contains(&v);
        for v in [self.activation_threshold, self.deactivation_threshold].into_iter().flatten() {
            if !ok(v) {
                return Err(format!("threshold {v} outside 1..100"));
            }
        }
        if let (Some(a), Some(d)) = (self.activation_threshold, self.deactivation_threshold) {
            if d > a {
                return Err("the deactivation threshold must not exceed the activation threshold".into());
            }
        }
        Ok(())
    }

    /// The sections of the drop-in for these values; empty when nothing differs from the preset.
    fn sections(&self) -> String {
        let mut s = String::new();
        if let Some(b) = self.disable_on_stylus {
            s.push_str(&format!("\n[Touchscreen]\nDisableOnStylus = {b}\n"));
        }
        if self.activation_threshold.is_some() || self.deactivation_threshold.is_some() {
            s.push_str("\n[Contacts]\n");
            if let Some(a) = self.activation_threshold {
                s.push_str(&format!("ActivationThreshold = {a}\n"));
            }
            if let Some(d) = self.deactivation_threshold {
                s.push_str(&format!("DeactivationThreshold = {d}\n"));
            }
        }
        s
    }

    /// The drop-in's text: these values and the pen's zero-force contact settings.
    pub fn render(&self, zero_force: &ZeroForce) -> String {
        format!(
            "# Written by sp9-helperd (sp9-ctl iptsd, sp9-ctl pen zero-force). Loaded after the platform preset.\n\
             [Device]\nVendor = 0x045E\nProduct = 0x0C52\n{}{}",
            self.sections(),
            zero_force.section()
        )
    }

    /// Write the drop-in. Returns whether the file changed (iptsd is to be restarted then).
    pub fn apply(&self, zero_force: &ZeroForce, root: &Root) -> io::Result<bool> {
        let p = root.p(IPTSD_DROPIN);
        let text = self.render(zero_force);
        if std::fs::read_to_string(&p).ok().as_deref() == Some(text.as_str()) {
            return Ok(false);
        }
        sys::write_atomic(&p, text.as_bytes()).map(|_| true)
    }
}

/// Zero-force contact of the pen (the iptsd patch of this repository, like Windows' ZFI): the
/// pen counts as touching when its tip signal reaches a fraction of the level it has while
/// writing with force, even when it reports no force. Without it small, light handwriting
/// loses strokes. Keys of iptsd's `[DFT]` section; an iptsd without the patch never asks for
/// them (its loader reads the keys it knows by name).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ZeroForce {
    pub enabled: bool,
    /// contact begins at or above this percentage of the reference level
    pub enter_percent: f64,
    /// contact ends below this percentage
    pub exit_percent: f64,
    /// pressure reported while in contact without force, percent of the pressure range
    pub floor_percent: f64,
    /// the reference level; 0 = iptsd learns it from strokes written with force after each start
    pub reference: f64,
}

impl Default for ZeroForce {
    /// The owner's values, chosen by feel on this device with the Slim Pen 2: at 20 / 10 no
    /// strokes were lost.
    fn default() -> Self {
        ZeroForce { enabled: true, enter_percent: 20.0, exit_percent: 10.0, floor_percent: 4.0, reference: 0.0 }
    }
}

impl ZeroForce {
    pub const ENTER_MAX: f64 = 150.0;
    pub const FLOOR_MAX: f64 = 50.0;

    pub fn validate(&self) -> Result<(), String> {
        let (enter, exit) = (self.enter_percent, self.exit_percent);
        if !(enter.is_finite() && exit.is_finite() && 1.0 <= exit && exit < enter && enter <= Self::ENTER_MAX) {
            return Err(format!("zero-force thresholds {enter} / {exit}: need 1 <= exit < enter <= 150 (percent)"));
        }
        if !(self.floor_percent.is_finite() && (0.0..=Self::FLOOR_MAX).contains(&self.floor_percent)) {
            return Err(format!("pressure floor {} outside 0..50 percent", self.floor_percent));
        }
        if !(self.reference.is_finite() && self.reference >= 0.0) {
            return Err(format!("zero-force reference {}: 0 (learn) or a positive level", self.reference));
        }
        Ok(())
    }

    /// The `[DFT]` section. Every key is written, also when the feature is off, so the file
    /// says what is in force whatever the preset holds.
    fn section(&self) -> String {
        format!(
            "\n[DFT]\nZeroForceContact = {}\nZeroForceEnter = {}\nZeroForceExit = {}\nContactMinPressure = {}\nZeroForceReference = {}\n",
            self.enabled,
            self.enter_percent / 100.0,
            self.exit_percent / 100.0,
            self.floor_percent / 100.0,
            self.reference
        )
    }
}

/// A Type Cover keyboard collection like the SP9's (for fakes and tests): Feature 213 four
/// 8-bit levels, Feature 214 current level, Output 213 Set Level.
pub fn fake_cover_descriptor() -> Vec<u8> {
    use hid::build::*;
    cat(&[
        page(0x0C),
        usage(0x07),
        collection(1),
        id(213),
        lmin(0),
        lmax(1),
        size(1),
        count(1),
        usage(0x515),
        input(2),
        size(7),
        input(1),
        lmin(0),
        lmax(255),
        size(8),
        count(4),
        usage(0x517),
        feature(2),
        lmin(0),
        lmax(12),
        size(8),
        count(1),
        usage(0x7B),
        output(2),
        id(214),
        usage(0x7B),
        feature(2),
        end(),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hid::{FakeHid, HidNode};
    use crate::sys::FakeTree;

    fn cover() -> FakeHid {
        let f = FakeHid::default();
        {
            let mut s = f.0.lock().unwrap();
            s.nodes.push(HidNode {
                name: "hidraw1".into(),
                bus: 0x19,
                vendor: 0x045E,
                product: 0x09AE,
                hid_name: "Surface Type Cover".into(),
                descriptor: fake_cover_descriptor(),
            });
            s.features.insert(("hidraw1".into(), 213), vec![213, 0, 3, 6, 12]);
            s.features.insert(("hidraw1".into(), 214), vec![214, 6]);
        }
        f
    }

    #[test]
    fn keyboard_backlight() {
        let h = cover();
        let k = find_kbd(&h).unwrap();
        assert_eq!(k.levels(&h), vec![0, 3, 6, 12]);
        assert_eq!(k.level(&h), Some(6));
        k.set_level(&h, 12).unwrap();
        k.set_level(&h, 0).unwrap();
        assert!(k.set_level(&h, 5).is_err());
        let w = h.0.lock().unwrap().writes.clone();
        // report 213: 1 bit input pad is input-only; the output report is ID + 1 byte
        assert_eq!(w, vec![("hidraw1".to_string(), vec![213, 12]), ("hidraw1".to_string(), vec![213, 0])]);
    }

    #[test]
    fn backlight_key_report() {
        let h = cover();
        let k = find_kbd(&h).unwrap();
        assert!(k.is_key_report(&[213, 0x01]));
        assert!(!k.is_key_report(&[213, 0x00]));
        assert!(!k.is_key_report(&[1, 0x01]));
        // the firmware stepped the level: Feature 214 read again
        h.0.lock().unwrap().features.insert(("hidraw1".into(), 214), vec![214, 12]);
        assert_eq!(k.level(&h), Some(12));
    }

    #[test]
    fn led_preferred_over_hidraw() {
        let h = cover();
        let leds = FakeLeds::default();
        let b = find_backlight(&leds, &h).unwrap();
        assert_eq!(b.backend(), "hidraw:hidraw1");
        assert_eq!(b.levels(&h), vec![0, 3, 6, 12]);
        leds.0.lock().unwrap().push(FakeLed { name: "surface_kbd::kbd_backlight".into(), brightness: 2, max: 3 });
        let b = find_backlight(&leds, &h).unwrap();
        assert_eq!(b.backend(), "led:surface_kbd::kbd_backlight");
        assert_eq!((b.levels(&h), b.level(&leds, &h)), (vec![0, 1, 2, 3], Some(2)));
        b.set_level(&leds, &h, 3).unwrap();
        assert!(b.set_level(&leds, &h, 4).is_err());
        assert_eq!(b.level(&leds, &h), Some(3));
        assert!(h.0.lock().unwrap().writes.is_empty(), "no hidraw write with the LED");
    }

    #[test]
    #[cfg_attr(windows, ignore = "':' in LED names is not a valid Windows file name")]
    fn sysfs_leds() {
        let t = FakeTree::new("leds");
        t.file(&format!("{SYS_LEDS}/input3::capslock/brightness"), "0\n")
            .file(&format!("{SYS_LEDS}/surface::kbd_backlight/brightness"), "1\n")
            .file(&format!("{SYS_LEDS}/surface::kbd_backlight/max_brightness"), "3\n");
        let l = SysfsLeds::new(t.root.clone());
        assert_eq!(l.kbd_leds(), ["surface::kbd_backlight"]);
        let b = find_backlight(&l, &FakeHid::default()).unwrap();
        b.set_level(&l, &FakeHid::default(), 2).unwrap();
        assert_eq!(t.read(&format!("{SYS_LEDS}/surface::kbd_backlight/brightness")), "2");
        assert!(l.set_brightness("../x", 1).is_err());
    }

    #[test]
    fn no_cover() {
        assert!(find_kbd(&FakeHid::default()).is_none());
    }

    #[test]
    fn iptsd_dropin() {
        let t = FakeTree::new("iptsd");
        let zf = ZeroForce::default();
        // the defaults: the pen's zero-force contact, nothing over the preset's thresholds
        assert!(Iptsd::default().apply(&zf, &t.root).unwrap());
        assert_eq!(
            t.read(IPTSD_DROPIN),
            "# Written by sp9-helperd (sp9-ctl iptsd, sp9-ctl pen zero-force). Loaded after the platform preset.\n\
             [Device]\nVendor = 0x045E\nProduct = 0x0C52\n\n\
             [DFT]\nZeroForceContact = true\nZeroForceEnter = 0.2\nZeroForceExit = 0.1\nContactMinPressure = 0.04\nZeroForceReference = 0"
        );
        assert!(!Iptsd::default().apply(&zf, &t.root).unwrap(), "nothing changed: no restart");
        let mut c = Iptsd { activation_threshold: Some(22.0), deactivation_threshold: Some(18.0), disable_on_stylus: Some(false) };
        c.validate().unwrap();
        assert!(c.apply(&zf, &t.root).unwrap());
        let text = t.read(IPTSD_DROPIN);
        assert!(
            text.contains("[Touchscreen]\nDisableOnStylus = false")
                && text.contains("[Contacts]\nActivationThreshold = 22\nDeactivationThreshold = 18")
        );
        assert!(text.contains("[DFT]\nZeroForceContact = true"));
        // switched off, other thresholds, a fixed reference: all keys stay in the file
        let off = ZeroForce { enabled: false, enter_percent: 85.0, exit_percent: 75.0, floor_percent: 0.0, reference: 1234.5 };
        assert!(c.apply(&off, &t.root).unwrap());
        let text = t.read(IPTSD_DROPIN);
        assert!(text.ends_with("[DFT]\nZeroForceContact = false\nZeroForceEnter = 0.85\nZeroForceExit = 0.75\nContactMinPressure = 0\nZeroForceReference = 1234.5"), "{text}");
        assert!(!c.apply(&off, &t.root).unwrap());
        c.deactivation_threshold = Some(30.0);
        assert!(c.validate().is_err());
    }

    #[test]
    fn zero_force_bounds() {
        let ok = |enter, exit, floor, reference| {
            ZeroForce { enabled: true, enter_percent: enter, exit_percent: exit, floor_percent: floor, reference }.validate()
        };
        ZeroForce::default().validate().unwrap();
        ok(150.0, 1.0, 50.0, 0.0).unwrap();
        ok(60.0, 45.0, 0.0, 800.0).unwrap();
        for (enter, exit) in [(10.0, 10.0), (10.0, 20.0), (151.0, 10.0), (20.0, 0.5), (f64::NAN, 10.0), (20.0, f64::NAN)] {
            assert!(ok(enter, exit, 4.0, 0.0).is_err(), "{enter} / {exit}");
        }
        for floor in [-1.0, 50.1, f64::NAN] {
            assert!(ok(20.0, 10.0, floor, 0.0).is_err(), "floor {floor}");
        }
        for reference in [-1.0, f64::INFINITY, f64::NAN] {
            assert!(ok(20.0, 10.0, 4.0, reference).is_err(), "reference {reference}");
        }
    }

    #[test]
    fn sysfs_inhibit() {
        let t = FakeTree::new("input");
        t.file(&format!("{SYS_INPUT}/input5/name"), "IPTSD Virtual Touchscreen 045E:0C52\n")
            .file(&format!("{SYS_INPUT}/input5/inhibited"), "0\n")
            .file(&format!("{SYS_INPUT}/input6/name"), "IPTSD Virtual Stylus 045E:0C52\n")
            .file(&format!("{SYS_INPUT}/event5/dev"), "13:69\n");
        let i = SysfsInput::new(t.root.clone());
        let ts = touchscreens(&i);
        assert_eq!(ts.len(), 1);
        i.set_inhibited(&ts[0].id, true).unwrap();
        assert_eq!(t.read(&format!("{SYS_INPUT}/input5/inhibited")), "1");
        assert!(i.set_inhibited("../x", true).is_err());
    }
}
