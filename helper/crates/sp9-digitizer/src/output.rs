// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
// Derived from iptsd (src/apps/daemon/touch.hpp, stylus.hpp), GPL-2.0-or-later,
// Copyright (C) the iptsd authors.
//! The two virtual input devices: what they look like ([`DeviceSpec`]) and the evdev events
//! that contacts and pen samples turn into. The events go to a [`Sink`]; nothing here does I/O.
//!
//! Names, axes and properties are those of iptsd's devices, which libinput quirks and the
//! helper match on.

use crate::config::{Config, Geometry, PRODUCT, VENDOR};
use crate::num;
use crate::pen::PenSample;
use crate::touch::Contact;
use std::collections::BTreeSet;
use std::f64::consts::{FRAC_PI_4, PI};

pub const EV_SYN: u16 = 0x00;
pub const EV_KEY: u16 = 0x01;
pub const EV_ABS: u16 = 0x03;
pub const SYN_REPORT: u16 = 0;

pub const BTN_TOOL_PEN: u16 = 0x140;
pub const BTN_TOOL_RUBBER: u16 = 0x141;
pub const BTN_TOUCH: u16 = 0x14a;
pub const BTN_STYLUS: u16 = 0x14b;

pub const ABS_X: u16 = 0x00;
pub const ABS_Y: u16 = 0x01;
pub const ABS_PRESSURE: u16 = 0x18;
pub const ABS_TILT_X: u16 = 0x1a;
pub const ABS_TILT_Y: u16 = 0x1b;
pub const ABS_MISC: u16 = 0x28;
pub const ABS_MT_SLOT: u16 = 0x2f;
pub const ABS_MT_TOUCH_MAJOR: u16 = 0x30;
pub const ABS_MT_TOUCH_MINOR: u16 = 0x31;
pub const ABS_MT_ORIENTATION: u16 = 0x34;
pub const ABS_MT_POSITION_X: u16 = 0x35;
pub const ABS_MT_POSITION_Y: u16 = 0x36;
pub const ABS_MT_TRACKING_ID: u16 = 0x39;

pub const INPUT_PROP_POINTER: u16 = 0x00;
pub const INPUT_PROP_DIRECT: u16 = 0x01;

pub const BUS_VIRTUAL: u16 = 0x06;

/// Slots of the touchscreen.
const MAX_CONTACTS: i32 = 16;
/// Range of the touchscreen axes, and of its contact sizes (the diagonal of that range).
const TOUCH_MAX_X: i32 = 9600;
const TOUCH_MAX_Y: i32 = 7200;
const TOUCH_DIAGONAL: i32 = 12000;
/// Range of the pressure axis.
const MAX_PRESSURE: i32 = 4096;
/// Tilt in hundredths of a degree.
const MAX_TILT: i32 = 9000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Device {
    Touchscreen,
    Stylus,
}

impl Device {
    /// The word in the device name.
    pub fn label(self) -> &'static str {
        match self {
            Device::Touchscreen => "Touchscreen",
            Device::Stylus => "Stylus",
        }
    }
}

/// Receives what the processing produces.
pub trait Sink {
    /// One event of a packet.
    fn event(&mut self, device: Device, kind: u16, code: u16, value: i32);
    /// The end of a packet (`SYN_REPORT`).
    fn sync(&mut self, device: Device);
    /// The pen sample a packet of the stylus is made from (diagnostics).
    fn pen_sample(&mut self, _sample: &PenSample) {}
    /// The contacts of an image, before they become events (diagnostics).
    fn contacts(&mut self, _contacts: &[Contact]) {}
}

/// One step of setting up a uinput device, in the order they are issued.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Setup {
    Event(u16),
    Key(u16),
    Property(u16),
    Abs { code: u16, min: i32, max: i32, resolution: i32 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceSpec {
    pub device: Device,
    pub name: String,
    pub bus: u16,
    pub vendor: u16,
    pub product: u16,
    pub version: u16,
    pub setup: Vec<Setup>,
}

/// The names of the two devices up to the vendor and product ID that follows them. libinput
/// quirks and the helper find the devices by these.
pub const TOUCHSCREEN_NAME: &str = "IPTSD Virtual Touchscreen";
pub const STYLUS_NAME: &str = "IPTSD Virtual Stylus";

fn device_name(device: Device) -> String {
    let name = match device {
        Device::Touchscreen => TOUCHSCREEN_NAME,
        Device::Stylus => STYLUS_NAME,
    };
    format!("{name} {VENDOR:04X}:{PRODUCT:04X}")
}

fn abs(code: u16, min: i32, max: i32, resolution: i32) -> Setup {
    Setup::Abs { code, min, max, resolution }
}

/// The touchscreen: multi-touch slots with position, orientation and size, and a single-touch
/// position. Resolutions are units per millimeter.
pub fn touchscreen_spec(geometry: &Geometry) -> DeviceSpec {
    let diagonal = geometry.width_cm.hypot(geometry.height_cm);
    let res_x = (f64::from(TOUCH_MAX_X) / (geometry.width_cm * 10.0)).round() as i32;
    let res_y = (f64::from(TOUCH_MAX_Y) / (geometry.height_cm * 10.0)).round() as i32;
    let res_d = (f64::from(TOUCH_DIAGONAL) / (diagonal * 10.0)).round() as i32;

    DeviceSpec {
        device: Device::Touchscreen,
        name: device_name(Device::Touchscreen),
        bus: BUS_VIRTUAL,
        vendor: VENDOR,
        product: PRODUCT,
        version: 0,
        setup: vec![
            Setup::Event(EV_ABS),
            Setup::Event(EV_KEY),
            Setup::Key(BTN_TOUCH),
            Setup::Property(INPUT_PROP_DIRECT),
            abs(ABS_MT_SLOT, 0, MAX_CONTACTS, 0),
            abs(ABS_MT_TRACKING_ID, 0, MAX_CONTACTS, 0),
            abs(ABS_MT_POSITION_X, 0, TOUCH_MAX_X, res_x),
            abs(ABS_MT_POSITION_Y, 0, TOUCH_MAX_Y, res_y),
            abs(ABS_MT_ORIENTATION, 0, 180, 0),
            abs(ABS_MT_TOUCH_MAJOR, 0, TOUCH_DIAGONAL, res_d),
            abs(ABS_MT_TOUCH_MINOR, 0, TOUCH_DIAGONAL, res_d),
            abs(ABS_X, 0, TOUCH_MAX_X, res_x),
            abs(ABS_Y, 0, TOUCH_MAX_Y, res_y),
        ],
    }
}

/// The range of the stylus axes: the size of the screen in units of `1 / axis_resolution` mm.
fn stylus_range(geometry: &Geometry, axis_resolution: u32) -> (i32, i32) {
    let res = f64::from(axis_resolution);
    ((geometry.width_cm * 10.0 * res).round() as i32, (geometry.height_cm * 10.0 * res).round() as i32)
}

/// The stylus: position, pressure, tilt, pen and eraser tools, one button.
pub fn stylus_spec(geometry: &Geometry, axis_resolution: u32) -> DeviceSpec {
    let (max_x, max_y) = stylus_range(geometry, axis_resolution);
    // Units per radian.
    let res_tilt = (18000.0 / PI).round() as i32;

    DeviceSpec {
        device: Device::Stylus,
        name: device_name(Device::Stylus),
        bus: BUS_VIRTUAL,
        vendor: VENDOR,
        product: PRODUCT,
        version: 0,
        setup: vec![
            Setup::Event(EV_KEY),
            Setup::Event(EV_ABS),
            Setup::Property(INPUT_PROP_DIRECT),
            Setup::Property(INPUT_PROP_POINTER),
            Setup::Key(BTN_TOUCH),
            Setup::Key(BTN_STYLUS),
            Setup::Key(BTN_TOOL_PEN),
            Setup::Key(BTN_TOOL_RUBBER),
            abs(ABS_X, 0, max_x, axis_resolution as i32),
            abs(ABS_Y, 0, max_y, axis_resolution as i32),
            abs(ABS_PRESSURE, 0, MAX_PRESSURE, 0),
            abs(ABS_TILT_X, -MAX_TILT, MAX_TILT, res_tilt),
            abs(ABS_TILT_Y, -MAX_TILT, MAX_TILT, res_tilt),
            abs(ABS_MISC, 0, i32::from(u16::MAX), 0),
        ],
    }
}

/// Pen samples to events of the stylus device.
#[derive(Clone, Debug)]
pub struct StylusOutput {
    max_x: i32,
    max_y: i32,
    /// The pen is in range and being reported.
    active: bool,
    /// Whether the last sample was of the eraser end.
    eraser: bool,
    /// Whether the last packet had the tool touching.
    contact: bool,
}

impl StylusOutput {
    pub fn new(config: &Config, geometry: &Geometry) -> StylusOutput {
        let (max_x, max_y) = stylus_range(geometry, config.pen.axis_resolution);
        StylusOutput { max_x, max_y, active: false, eraser: false, contact: false }
    }

    pub fn active(&self) -> bool {
        self.active
    }

    pub fn update(&mut self, s: &PenSample, sink: &mut dyn Sink) {
        const D: Device = Device::Stylus;

        // Switching tools within one packet confuses clients: a pen that is in range goes
        // out of range for the packet in which its tool changes. One that comes into range
        // starts with the tool it has.
        let was_active = self.active;
        self.active = s.proximity && !(self.active && self.eraser != s.eraser);

        // A tool that comes into range touching, or leaves range touching, does so in two
        // packets: clients keep the tool's cursor when contact and range change at once.
        if self.active && !was_active && s.contact {
            sink.event(D, EV_KEY, BTN_TOUCH, 0);
            sink.event(D, EV_KEY, BTN_TOOL_PEN, i32::from(!s.eraser));
            sink.event(D, EV_KEY, BTN_TOOL_RUBBER, i32::from(s.eraser));
            sink.event(D, EV_ABS, ABS_X, (s.x * f64::from(self.max_x)).round() as i32);
            sink.event(D, EV_ABS, ABS_Y, (s.y * f64::from(self.max_y)).round() as i32);
            sink.event(D, EV_ABS, ABS_PRESSURE, 0);
            sink.sync(D);
        }
        if !self.active && self.contact {
            sink.event(D, EV_KEY, BTN_TOUCH, 0);
            sink.event(D, EV_ABS, ABS_PRESSURE, 0);
            sink.sync(D);
        }
        self.contact = self.active && s.contact;

        if self.active {
            let (tilt_x, tilt_y) = tilt_xy(s.altitude, s.azimuth);
            sink.event(D, EV_KEY, BTN_TOUCH, i32::from(s.contact));
            sink.event(D, EV_KEY, BTN_TOOL_PEN, i32::from(!s.eraser));
            sink.event(D, EV_KEY, BTN_TOOL_RUBBER, i32::from(s.eraser));
            sink.event(D, EV_KEY, BTN_STYLUS, i32::from(s.button));
            sink.event(D, EV_ABS, ABS_X, (s.x * f64::from(self.max_x)).round() as i32);
            sink.event(D, EV_ABS, ABS_Y, (s.y * f64::from(self.max_y)).round() as i32);
            sink.event(D, EV_ABS, ABS_PRESSURE, (s.pressure * f64::from(MAX_PRESSURE)).round() as i32);
            sink.event(D, EV_ABS, ABS_MISC, 0);
            sink.event(D, EV_ABS, ABS_TILT_X, tilt_x);
            sink.event(D, EV_ABS, ABS_TILT_Y, tilt_y);
        } else {
            sink.event(D, EV_KEY, BTN_TOUCH, 0);
            sink.event(D, EV_KEY, BTN_TOOL_PEN, 0);
            sink.event(D, EV_KEY, BTN_TOOL_RUBBER, 0);
            sink.event(D, EV_KEY, BTN_STYLUS, 0);
        }

        self.eraser = s.eraser;
        sink.sync(D);
    }
}

/// The tilt along the two screen axes, in hundredths of a degree.
fn tilt_xy(altitude: f64, azimuth: f64) -> (i32, i32) {
    if altitude <= 0.0 {
        return (0, 0);
    }
    let (sin_alt, cos_alt) = (altitude.sin(), altitude.cos());
    let (sin_azm, cos_azm) = (azimuth.sin(), azimuth.cos());

    let atan_x = cos_alt.atan2(sin_alt * cos_azm);
    let atan_y = cos_alt.atan2(sin_alt * sin_azm);

    (MAX_TILT - (atan_x * 4500.0 / FRAC_PI_4).round() as i32, (atan_y * 4500.0 / FRAC_PI_4).round() as i32 - MAX_TILT)
}

/// Contacts to events of the touchscreen device.
#[derive(Clone, Debug)]
pub struct TouchOutput {
    disable_on_palm: bool,
    /// How far outside of the screen a contact may be, as fractions of its width and height.
    overshoot: (f64, f64),
    /// The slots of the reported contacts of the current and of the previous image.
    current: BTreeSet<usize>,
    previous: BTreeSet<usize>,
    /// Contacts that were on the screen while touch was blocked (the pen was there): they
    /// are not reported until they are lifted.
    rejected: BTreeSet<usize>,
    /// The slot that also feeds the single-touch axes, and whether it is reported as down.
    single: usize,
    single_down: bool,
    scratch: Vec<Contact>,
}

impl TouchOutput {
    /// Takes the settings that can change while running.
    pub fn set_config(&mut self, config: &Config, geometry: &Geometry) {
        self.disable_on_palm = config.touch.disable_on_palm;
        self.overshoot = (config.touch.overshoot_cm / geometry.width_cm, config.touch.overshoot_cm / geometry.height_cm);
    }

    pub fn new(config: &Config, geometry: &Geometry) -> TouchOutput {
        TouchOutput {
            disable_on_palm: config.touch.disable_on_palm,
            overshoot: (config.touch.overshoot_cm / geometry.width_cm, config.touch.overshoot_cm / geometry.height_cm),
            current: BTreeSet::new(),
            previous: BTreeSet::new(),
            rejected: BTreeSet::new(),
            single: 0,
            single_down: false,
            scratch: Vec::new(),
        }
    }

    /// The pen arrived: lifts everything. The contacts that are down stay rejected.
    pub fn block(&mut self, sink: &mut dyn Sink) {
        self.lift_all(sink);
        sink.sync(Device::Touchscreen);
        self.rejected.extend(self.current.iter().chain(&self.previous));
        self.current.clear();
        self.previous.clear();
    }

    /// Takes the contacts of an image. While `blocked` nothing is reported and every
    /// contact is remembered as rejected; a rejected contact comes back only as a new one,
    /// after it was lifted.
    pub fn update(&mut self, contacts: &[Contact], blocked: bool, sink: &mut dyn Sink) {
        if blocked {
            self.rejected.clear();
            self.rejected.extend(contacts.iter().map(|c| c.id));
            return;
        }
        self.rejected.retain(|id| contacts.iter().any(|c| c.id == *id));

        self.scratch.clear();
        self.scratch.extend(contacts.iter().filter(|c| !self.rejected.contains(&c.id)).copied());
        let contacts = std::mem::take(&mut self.scratch);

        std::mem::swap(&mut self.current, &mut self.previous);
        self.current.clear();
        self.current.extend(contacts.iter().map(|c| c.id));

        if self.disable_on_palm && contacts.iter().any(|c| !c.valid) {
            self.lift_all(sink);
        } else {
            self.report(&contacts, sink);
        }
        sink.sync(Device::Touchscreen);
        self.scratch = contacts;
    }

    /// Whether a contact is reported with a position: a finger, on the screen or within the
    /// overshoot around it.
    fn on_screen(&self, c: &Contact) -> bool {
        let (ox, oy) = self.overshoot;
        c.valid && c.x >= -ox && c.x <= ox + 1.0 && c.y >= -oy && c.y <= oy + 1.0
    }

    fn report(&mut self, contacts: &[Contact], sink: &mut dyn Sink) {
        const D: Device = Device::Touchscreen;

        // The single-touch axes follow one contact while it is there, then the first of
        // the others: the button stays down as long as any finger is reported. An unstable
        // image of that contact keeps what was reported last.
        let follows = |c: &&Contact| c.id == self.single && (self.on_screen(c) || (!c.stable && self.single_down));
        if !contacts.iter().any(|c| follows(&c)) {
            if let Some(c) = contacts.iter().find(|c| c.stable && self.on_screen(c)) {
                self.single = c.id;
            }
        }

        let mut single_reported = false;

        for c in contacts {
            // An unstable contact keeps what was reported for it last.
            if !c.stable {
                single_reported |= c.id == self.single && self.single_down;
                continue;
            }

            if self.on_screen(c) {
                let slot = c.id as i32;
                sink.event(D, EV_ABS, ABS_MT_SLOT, slot);
                sink.event(D, EV_ABS, ABS_MT_TRACKING_ID, slot);
                let (x, y) = touch_position(c);
                sink.event(D, EV_ABS, ABS_MT_POSITION_X, x);
                sink.event(D, EV_ABS, ABS_MT_POSITION_Y, y);
                sink.event(D, EV_ABS, ABS_MT_ORIENTATION, (c.orientation * 180.0).round() as i32);
                sink.event(D, EV_ABS, ABS_MT_TOUCH_MAJOR, (c.major() * f64::from(TOUCH_DIAGONAL)).round() as i32);
                sink.event(D, EV_ABS, ABS_MT_TOUCH_MINOR, (c.minor() * f64::from(TOUCH_DIAGONAL)).round() as i32);

                if c.id == self.single {
                    sink.event(D, EV_KEY, BTN_TOUCH, 1);
                    sink.event(D, EV_ABS, ABS_X, x);
                    sink.event(D, EV_ABS, ABS_Y, y);
                    single_reported = true;
                }
            } else {
                // A palm, or too far outside of the screen.
                lift_slot(c.id, sink);
            }
        }

        // The contacts of the previous image that are gone.
        for &id in self.previous.difference(&self.current) {
            lift_slot(id, sink);
        }

        self.single_down = single_reported;
        if !single_reported {
            sink.event(D, EV_KEY, BTN_TOUCH, 0);
        }
    }

    fn lift_all(&mut self, sink: &mut dyn Sink) {
        for &id in self.current.iter().chain(&self.previous) {
            sink.event(Device::Touchscreen, EV_ABS, ABS_MT_SLOT, id as i32);
            lift_slot(id, sink);
        }
        sink.event(Device::Touchscreen, EV_KEY, BTN_TOUCH, 0);
        self.single_down = false;
    }
}

fn lift_slot(id: usize, sink: &mut dyn Sink) {
    sink.event(Device::Touchscreen, EV_ABS, ABS_MT_SLOT, id as i32);
    sink.event(Device::Touchscreen, EV_ABS, ABS_MT_TRACKING_ID, -1);
}

fn touch_position(c: &Contact) -> (i32, i32) {
    (
        (num::clamp(c.x, 0.0, 1.0) * f64::from(TOUCH_MAX_X)).round() as i32,
        (num::clamp(c.y, 0.0, 1.0) * f64::from(TOUCH_MAX_Y)).round() as i32,
    )
}

#[cfg(test)]
pub(crate) mod testsink {
    use super::*;

    /// Collects packets as text: `<device initial> type:code:value ...`.
    #[derive(Default)]
    pub struct Packets {
        pending: std::collections::HashMap<Device, String>,
        pub packets: Vec<String>,
        pub samples: Vec<PenSample>,
        pub contact_counts: Vec<usize>,
    }

    impl Sink for Packets {
        fn event(&mut self, device: Device, kind: u16, code: u16, value: i32) {
            self.pending.entry(device).or_default().push_str(&format!(" {kind}:{code}:{value}"));
        }
        fn sync(&mut self, device: Device) {
            let events = self.pending.remove(&device).unwrap_or_default();
            self.packets.push(format!("{}{events}", &device.label()[..1]));
        }
        fn pen_sample(&mut self, sample: &PenSample) {
            self.samples.push(*sample);
        }
        fn contacts(&mut self, contacts: &[Contact]) {
            self.contact_counts.push(contacts.len());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testsink::Packets;
    use super::*;

    fn contact(id: usize, x: f64, y: f64) -> Contact {
        Contact { id, x, y, size: [0.02, 0.03], orientation: 0.25, stable: true, valid: true }
    }

    #[test]
    fn specs_match_the_devices_of_iptsd() {
        let t = touchscreen_spec(&Geometry::SP9);
        assert_eq!(t.name, "IPTSD Virtual Touchscreen 045E:0C52");
        assert!(t.setup.contains(&abs(ABS_MT_POSITION_X, 0, 9600, 35)));
        assert!(t.setup.contains(&abs(ABS_MT_POSITION_Y, 0, 7200, 39)));
        assert!(t.setup.contains(&abs(ABS_MT_TOUCH_MAJOR, 0, 12000, 36)));

        let s = stylus_spec(&Geometry::SP9, 100);
        assert_eq!(s.name, "IPTSD Virtual Stylus 045E:0C52");
        assert!(s.setup.contains(&abs(ABS_X, 0, 27389, 100)));
        assert!(s.setup.contains(&abs(ABS_Y, 0, 18259, 100)));
        assert!(s.setup.contains(&abs(ABS_TILT_X, -9000, 9000, 5730)));
    }

    #[test]
    fn stylus_packets() {
        let mut out = StylusOutput::new(&Config::default(), &Geometry::SP9);
        let mut sink = Packets::default();
        let mut s = PenSample { proximity: true, contact: true, x: 0.5, y: 0.25, pressure: 0.5, ..PenSample::default() };
        out.update(&s, &mut sink);
        // Coming into range touching: the tool first, the contact in the next packet.
        assert_eq!(sink.packets[0], "S 1:330:0 1:320:1 1:321:0 3:0:13695 3:1:4565 3:24:0");
        assert_eq!(sink.packets[1], "S 1:330:1 1:320:1 1:321:0 1:331:0 3:0:13695 3:1:4565 3:24:2048 3:40:0 3:26:0 3:27:0");
        // The tool changes while in range: the contact ends, one packet out of range, then the eraser.
        s.eraser = true;
        out.update(&s, &mut sink);
        assert_eq!(sink.packets[2], "S 1:330:0 3:24:0");
        assert_eq!(sink.packets[3], "S 1:330:0 1:320:0 1:321:0 1:331:0");
        assert!(!out.active());
        out.update(&s, &mut sink);
        assert!(sink.packets[4].starts_with("S 1:330:0 1:320:0 1:321:1"));
        assert!(sink.packets[5].starts_with("S 1:330:1 1:320:0 1:321:1"));
        s.proximity = false;
        out.update(&s, &mut sink);
        assert_eq!(sink.packets[6], "S 1:330:0 3:24:0");
        assert_eq!(sink.packets[7], "S 1:330:0 1:320:0 1:321:0 1:331:0");
        // Coming into range with the other end: announced as that at once.
        s.proximity = true;
        s.eraser = false;
        out.update(&s, &mut sink);
        assert!(sink.packets[9].starts_with("S 1:330:1 1:320:1 1:321:0"));
    }

    #[test]
    fn tilt_axes() {
        assert_eq!(tilt_xy(0.0, 1.0), (0, 0));
        // 30 degrees towards +X.
        assert_eq!(tilt_xy(PI / 6.0, 0.0), (3000, 0));
        // 30 degrees towards the top of the screen (azimuth counts counterclockwise).
        assert_eq!(tilt_xy(PI / 6.0, PI / 2.0), (0, -3000));
    }

    #[test]
    fn touch_packets() {
        let mut out = TouchOutput::new(&Config::default(), &Geometry::SP9);
        let mut sink = Packets::default();
        out.update(&[contact(0, 0.5, 0.5), contact(1, 0.25, 1.01)], false, &mut sink);
        assert_eq!(
            sink.packets[0],
            "T 3:47:0 3:57:0 3:53:4800 3:54:3600 3:52:45 3:48:360 3:49:240 1:330:1 3:0:4800 3:1:3600 \
             3:47:1 3:57:1 3:53:2400 3:54:7200 3:52:45 3:48:360 3:49:240"
        );
        // The first one is lifted: its slot is released, and the single-touch axes go over
        // to the other contact without a release.
        out.update(&[contact(1, 0.25, 0.5)], false, &mut sink);
        assert_eq!(
            sink.packets[1],
            "T 3:47:1 3:57:1 3:53:2400 3:54:3600 3:52:45 3:48:360 3:49:240 1:330:1 3:0:2400 3:1:3600 3:47:0 3:57:-1"
        );
        out.update(&[contact(1, 0.25, 0.5)], false, &mut sink);
        assert!(sink.packets[2].ends_with("1:330:1 3:0:2400 3:1:3600"));

        // An unstable frame of the contact on the single-touch axes changes nothing.
        out.update(&[Contact { stable: false, ..contact(1, 0.3, 0.5) }], false, &mut sink);
        assert_eq!(sink.packets.pop().unwrap(), "T");

        // A palm and a contact far outside release their slots; an unstable contact is skipped.
        let palm = Contact { valid: false, ..contact(1, 0.25, 0.5) };
        let outside = contact(2, 0.5, 1.2);
        let unstable = Contact { stable: false, ..contact(3, 0.1, 0.1) };
        out.update(&[palm, outside, unstable], false, &mut sink);
        assert_eq!(sink.packets[3], "T 3:47:1 3:57:-1 3:47:2 3:57:-1 1:330:0");

        out.block(&mut sink);
        assert_eq!(sink.packets[4], "T 3:47:1 3:47:1 3:57:-1 3:47:2 3:47:2 3:57:-1 3:47:3 3:47:3 3:57:-1 3:47:1 3:47:1 3:57:-1 1:330:0");
        out.update(&[contact(0, 0.5, 0.5)], true, &mut sink);
        assert_eq!(sink.packets.len(), 5);
        // The contact that was there while touch was blocked stays out until it is lifted;
        // another one is reported.
        out.update(&[contact(0, 0.5, 0.5), contact(4, 0.75, 0.5)], false, &mut sink);
        assert_eq!(sink.packets[5], "T 3:47:4 3:57:4 3:53:7200 3:54:3600 3:52:45 3:48:360 3:49:240 1:330:1 3:0:7200 3:1:3600");
        out.update(&[contact(4, 0.75, 0.5)], false, &mut sink);
        out.update(&[contact(0, 0.5, 0.5), contact(4, 0.75, 0.5)], false, &mut sink);
        assert!(sink.packets[7].starts_with("T 3:47:0 3:57:0 3:53:4800"));
    }
}
