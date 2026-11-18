// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
//! The D-Bus API end to end on a peer-to-peer connection (TCP on localhost, so it also runs
//! on the Windows development host) against the in-memory backends, polkit skipped.

use super::*;
use sp9_helper_core::hid::{FakeHid, HidNode};
use sp9_helper_core::sys::FakeTree;
use sp9_helper_core::{cover, display, input, pen, pen_feedback, perf, ssam};
use zbus::connection::Builder;
use zbus::zvariant::OwnedValue;

struct Rig {
    client: zbus::Connection,
    _server: zbus::Connection,
    power: perf::FakePower,
    ssam: ssam::FakeSsam,
    hid: FakeHid,
    leds: input::FakeLeds,
    tree: FakeTree,
}

fn rig() -> Rig {
    let tree = FakeTree::new("dbus");
    let mut b = Backends::fake();
    let power = perf::FakePower::default();
    let ssam = ssam::FakeSsam::sp9();
    let hid = FakeHid::default();
    let leds = input::FakeLeds::default();
    let disp = display::FakeDisplay::default();
    disp.0.lock().unwrap().panel_on = Some(true);
    {
        let mut h = hid.0.lock().unwrap();
        h.nodes.push(HidNode {
            name: "hidraw1".into(),
            bus: 0x19,
            vendor: 0x045E,
            product: 0x09AE,
            hid_name: "Surface Type Cover".into(),
            descriptor: input::fake_cover_descriptor(),
        });
        h.features.insert(("hidraw1".into(), 213), vec![213, 0, 3, 6, 12]);
        h.features.insert(("hidraw1".into(), 214), vec![214, 6]);
        h.nodes.push(HidNode {
            name: "hidraw7".into(),
            bus: 5,
            vendor: pen::PEN_VENDOR,
            product: pen::PEN_PRODUCT,
            hid_name: "Surface Pen".into(),
            descriptor: pen::fake_pen_descriptor(),
        });
    }
    b.power = Box::new(power.clone());
    b.ssam = Box::new(ssam.clone());
    b.hid = Box::new(hid.clone());
    b.leds = Box::new(leds.clone());
    b.display = Box::new(disp);
    let shared = Arc::new(Shared::new(b, tree.root.clone(), tree.root.p("/etc/sp9/helper.toml"), true));

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let client_stream = std::net::TcpStream::connect(addr).unwrap();
    let (server_stream, _) = listener.accept().unwrap();
    let guid = zbus::Guid::generate();
    let server = std::thread::spawn(move || {
        let bld = Builder::async_io_tcp_stream(server_stream).server(guid).unwrap().p2p().auth_mechanism(zbus::AuthMechanism::Anonymous);
        zbus::block_on(serve(bld, &shared).unwrap().build()).unwrap()
    });
    let client =
        zbus::block_on(Builder::async_io_tcp_stream(client_stream).p2p().auth_mechanism(zbus::AuthMechanism::Anonymous).build()).unwrap();
    let server = server.join().unwrap();
    Rig { client, _server: server, power, ssam, hid, leds, tree }
}

fn iface(obj: &str) -> String {
    if obj.is_empty() {
        BUS.to_string()
    } else {
        format!("{BUS}.{obj}")
    }
}

impl Rig {
    fn get(&self, path: &str, obj: &str, prop: &str) -> OwnedValue {
        let r = zbus::block_on(self.client.call_method(
            None::<&str>,
            path,
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &(iface(obj), prop),
        ))
        .unwrap();
        r.body().deserialize::<OwnedValue>().unwrap()
    }

    fn call<B>(&self, path: &str, obj: &str, method: &str, body: &B) -> zbus::Result<zbus::Message>
    where
        B: zbus::export::serde::Serialize + zbus::zvariant::DynamicType,
    {
        zbus::block_on(self.client.call_method(None::<&str>, path, Some(iface(obj).as_str()), method, body))
    }
}

fn err_name(r: zbus::Result<zbus::Message>) -> String {
    match r {
        Err(zbus::Error::MethodError(n, _, _)) => n.to_string(),
        other => format!("{other:?}"),
    }
}

#[test]
fn root_and_features() {
    let r = rig();
    assert_eq!(String::try_from(r.get(ROOT, "", "Device")).unwrap(), DEVICE);
    let f: Vec<String> = r.get(ROOT, "", "Features").try_into().unwrap();
    assert_eq!(f, ["Performance", "Battery", "Display", "Input", "Pen", "Diagnostics"]);
}

#[test]
fn performance() {
    let r = rig();
    r.call(P_PERFORMANCE, "Performance", "SetProfile", &("performance",)).unwrap();
    assert_eq!(r.power.0.lock().unwrap().profile, "performance");
    assert_eq!(String::try_from(r.get(P_PERFORMANCE, "Performance", "Profile")).unwrap(), "performance");
    assert!(err_name(r.call(P_PERFORMANCE, "Performance", "SetProfile", &("turbo",))).ends_with("InvalidArgs"));
    assert_eq!(f64::try_from(r.get(P_PERFORMANCE, "Performance", "TargetPl1W")).unwrap(), 35.0);

    r.call(P_PERFORMANCE, "Performance", "SetLimits", &("performance", "ac", 30.0f64, 50.0f64, 28.0f64, 64u8)).unwrap();
    assert_eq!(f64::try_from(r.get(P_PERFORMANCE, "Performance", "TargetPl1W")).unwrap(), 30.0);
    assert!(r.tree.read("/etc/sp9/helper.toml").contains("performance.ac"));
    let e = err_name(r.call(P_PERFORMANCE, "Performance", "SetLimits", &("performance", "ac", 40.0f64, 50.0f64, 28.0f64, 64u8)));
    assert!(e.ends_with("InvalidArgs"), "{e}");
    r.call(P_PERFORMANCE, "Performance", "ResetLimits", &("performance", "ac")).unwrap();
    assert_eq!(f64::try_from(r.get(P_PERFORMANCE, "Performance", "TargetPl1W")).unwrap(), 35.0);
}

#[test]
fn fan_request_needs_opt_in() {
    let r = rig();
    let e = err_name(r.call(P_PERFORMANCE, "Performance", "RequestFanRpm", &(3500.0f64,)));
    assert!(e.ends_with("NotSupported"), "{e}");
    assert!(r.ssam.0.lock().unwrap().sent.is_empty());
    r.call(P_PERFORMANCE, "Performance", "SetFanControlAllowed", &(true,)).unwrap();
    r.call(P_PERFORMANCE, "Performance", "RequestFanRpm", &(3500.0f64,)).unwrap();
    assert!(err_name(r.call(P_PERFORMANCE, "Performance", "RequestFanRpm", &(9000.0f64,))).ends_with("InvalidArgs"));
    assert!(err_name(r.call(P_PERFORMANCE, "Performance", "RequestFanRpm", &(1000.0f64,))).ends_with("InvalidArgs"));
    assert!(err_name(r.call(P_PERFORMANCE, "Performance", "RequestFanRpm", &(f64::NAN,))).ends_with("InvalidArgs"));
    assert!(err_name(r.call(P_PERFORMANCE, "Performance", "RequestFanRpm", &(-1.0f64,))).ends_with("InvalidArgs"));
    assert_eq!(f64::try_from(r.get(P_PERFORMANCE, "Performance", "FanRpmRequest")).unwrap(), 3500.0);
    assert_eq!(String::try_from(r.get(P_PERFORMANCE, "Performance", "FanRequestState")).unwrap(), "settling");
    r.call(P_PERFORMANCE, "Performance", "SetFanControlAllowed", &(false,)).unwrap();
    let sent = r.ssam.0.lock().unwrap().sent.clone();
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[0], (ssam::FAN_RPM_REQUEST, 3500u16.to_le_bytes().to_vec()));
    assert_eq!(sent[1].1, vec![0, 0], "opting out withdraws the request");
    assert_eq!(f64::try_from(r.get(P_PERFORMANCE, "Performance", "FanRpmRequest")).unwrap(), 0.0);
    assert_eq!(String::try_from(r.get(P_PERFORMANCE, "Performance", "FanRequestState")).unwrap(), "none");
}

#[test]
fn start_withdraws_a_request_left_in_the_sam() {
    let shared = |allow: bool, ssam: &ssam::FakeSsam| {
        let tree = FakeTree::new(if allow { "fanstart1" } else { "fanstart0" });
        tree.file("/etc/sp9/helper.toml", format!("[fan]\nallow_rpm_request = {allow}\n"));
        let mut b = Backends::fake();
        b.ssam = Box::new(ssam.clone());
        Shared::new(b, tree.root.clone(), tree.root.p("/etc/sp9/helper.toml"), true)
    };
    let off = ssam::FakeSsam::sp9();
    shared(false, &off).withdraw_stale_fan_request().unwrap();
    assert!(off.0.lock().unwrap().sent.is_empty(), "nothing is sent unless requests are allowed");
    let on = ssam::FakeSsam::sp9();
    shared(true, &on).withdraw_stale_fan_request().unwrap();
    assert_eq!(on.0.lock().unwrap().sent.clone(), vec![(ssam::FAN_RPM_REQUEST, vec![0, 0])]);
}

#[test]
fn battery_smart_charging() {
    let r = rig();
    assert!(bool::try_from(r.get(P_BATTERY, "Battery", "SmartChargingSupported")).unwrap());
    assert!(!bool::try_from(r.get(P_BATTERY, "Battery", "SmartChargingEngaged")).unwrap());
    let m = r.call(P_BATTERY, "Battery", "ReleaseSmartCharging", &()).unwrap();
    assert_eq!(m.body().deserialize::<String>().unwrap(), "not-engaged");
}

#[test]
fn input_and_pen() {
    let r = rig();
    assert_eq!(i32::try_from(r.get(P_INPUT, "Input", "KeyboardBacklight")).unwrap(), 6);
    r.call(P_INPUT, "Input", "SetKeyboardBacklight", &(12u32,)).unwrap();
    assert!(err_name(r.call(P_INPUT, "Input", "SetKeyboardBacklight", &(5u32,))).ends_with("InvalidArgs"));
    r.call(P_PEN, "Pen", "SetIntensity", &(200u32,)).unwrap();
    r.call(P_PEN, "Pen", "TestPulse", &()).unwrap();
    let w = r.hid.0.lock().unwrap().writes.clone();
    assert_eq!(w[0], ("hidraw1".to_string(), vec![213, 12]));
    assert_eq!(w[1], ("hidraw7".to_string(), vec![0x41, 0, 200, 3, 0, 0, 0]));
    // the drop-in is written even where iptsd cannot be restarted (no systemctl here)
    let _ = r.call(P_INPUT, "Input", "SetIptsd", &(20.0f64, 16.0f64, 0i32));
    assert!(r.tree.read(input::IPTSD_DROPIN).contains("ActivationThreshold = 20"));
    assert!(err_name(r.call(P_INPUT, "Input", "SetIptsd", &(10.0f64, 16.0f64, 0i32))).ends_with("InvalidArgs"));
}

#[test]
fn display_policy() {
    let r = rig();
    r.call(P_DISPLAY, "Display", "SetPsrPolicy", &("off", "psr1")).unwrap();
    assert_eq!(String::try_from(r.get(P_DISPLAY, "Display", "PsrAc")).unwrap(), "off");
    assert!(err_name(r.call(P_DISPLAY, "Display", "SetPsrPolicy", &("psr2", "off"))).ends_with("InvalidArgs"));
}

#[test]
fn keyboard_backlight_prefers_led() {
    let r = rig();
    assert_eq!(String::try_from(r.get(P_INPUT, "Input", "KeyboardBacklightBackend")).unwrap(), "hidraw:hidraw1");
    r.leds.0.lock().unwrap().push(input::FakeLed { name: "surface::kbd_backlight".into(), brightness: 1, max: 3 });
    assert_eq!(String::try_from(r.get(P_INPUT, "Input", "KeyboardBacklightBackend")).unwrap(), "led:surface::kbd_backlight");
    let levels: Vec<u32> = r.get(P_INPUT, "Input", "KeyboardBacklightLevels").try_into().unwrap();
    assert_eq!(levels, [0, 1, 2, 3]);
    r.call(P_INPUT, "Input", "SetKeyboardBacklight", &(2u32,)).unwrap();
    assert_eq!(i32::try_from(r.get(P_INPUT, "Input", "KeyboardBacklight")).unwrap(), 2);
    assert!(err_name(r.call(P_INPUT, "Input", "SetKeyboardBacklight", &(6u32,))).ends_with("InvalidArgs"));
    assert!(r.hid.0.lock().unwrap().writes.is_empty());
}

#[test]
fn pen_feedback_and_coupling_settings() {
    let r = rig();
    assert!(!bool::try_from(r.get(P_PEN, "Pen", "InkFeedback")).unwrap());
    assert!(bool::try_from(r.get(P_PEN, "Pen", "SpeedFeedback")).unwrap());
    assert_eq!(f64::try_from(r.get(P_PEN, "Pen", "SpeedFullScale")).unwrap(), 300.0);
    assert_eq!(f64::try_from(r.get(P_PEN, "Pen", "Strength")).unwrap(), 100.0);
    assert!(bool::try_from(r.get(P_PEN, "Pen", "EraserClick")).unwrap());
    assert!(!bool::try_from(r.get(P_PEN, "Pen", "DigitizerAvailable")).unwrap());
    assert!(!bool::try_from(r.get(P_PEN, "Pen", "CoverAvailable")).unwrap());
    {
        use sp9_helper_core::hid::build::*;
        let digitizer =
            cat(&[page(0xFF00), usage(0x01), collection(1), id(9), usage(0xC9), lmin(0), lmax(255), size(8), count(63), feature(2), end()]);
        let mut h = r.hid.0.lock().unwrap();
        let node = |name: &str, product, descriptor| HidNode {
            name: name.into(),
            bus: 1,
            vendor: 0x045E,
            product,
            hid_name: String::new(),
            descriptor,
        };
        h.nodes.push(node("hidraw8", pen_feedback::DIGITIZER_PRODUCT, digitizer));
        h.nodes.push(node("hidraw5", 0x09B0, cover::fake_cover_pen_descriptor()));
    }
    assert!(bool::try_from(r.get(P_PEN, "Pen", "DigitizerAvailable")).unwrap());
    assert!(bool::try_from(r.get(P_PEN, "Pen", "CoverAvailable")).unwrap());

    r.call(P_PEN, "Pen", "SetSpeedFeedback", &(false,)).unwrap();
    r.call(P_PEN, "Pen", "SetSpeedFullScale", &(200.0f64,)).unwrap();
    assert!(!bool::try_from(r.get(P_PEN, "Pen", "SpeedFeedback")).unwrap());
    assert_eq!(f64::try_from(r.get(P_PEN, "Pen", "SpeedFullScale")).unwrap(), 200.0);
    for bad in [10.0f64, 5000.0, f64::NAN] {
        assert!(err_name(r.call(P_PEN, "Pen", "SetSpeedFullScale", &(bad,))).ends_with("InvalidArgs"));
    }
    let toml = r.tree.read("/etc/sp9/helper.toml");
    assert!(toml.contains("speed_feedback = false") && toml.contains("full_scale_mm_s = 200.0"), "{toml}");
    r.call(P_PEN, "Pen", "SetSpeedFullScale", &(0.0f64,)).unwrap();
    assert_eq!(f64::try_from(r.get(P_PEN, "Pen", "SpeedFullScale")).unwrap(), 300.0);
    assert!(!r.tree.read("/etc/sp9/helper.toml").contains("full_scale"));

    // strength is the intensity byte: 100 % = 127, inside 2..254
    for (pct, want) in [(150.0f64, 191u32), (200.0, 254), (500.0, 254), (0.0, 2), (100.0, 127)] {
        r.call(P_PEN, "Pen", "SetStrength", &(pct,)).unwrap();
        assert_eq!(u32::try_from(r.get(P_PEN, "Pen", "Intensity")).unwrap(), want, "{pct} %");
    }
    assert_eq!(f64::try_from(r.get(P_PEN, "Pen", "Strength")).unwrap(), 100.0);
    assert_eq!(f64::try_from(r.get(P_PEN, "Pen", "SpeedFullScale")).unwrap(), 300.0, "strength leaves the speed scale alone");
    for bad in [-5.0f64, f64::NAN] {
        assert!(err_name(r.call(P_PEN, "Pen", "SetStrength", &(bad,))).ends_with("InvalidArgs"));
    }
    // the inking waveform: brush unless chosen
    assert_eq!(String::try_from(r.get(P_PEN, "Pen", "InkWaveform")).unwrap(), "brush");
    let choices: Vec<String> = r.get(P_PEN, "Pen", "InkWaveforms").try_into().unwrap();
    assert_eq!(choices, ["ink", "pencil", "marker", "chisel-marker", "brush", "sparkle"]);
    r.call(P_PEN, "Pen", "SetInkWaveform", &("pencil",)).unwrap();
    assert_eq!(String::try_from(r.get(P_PEN, "Pen", "InkWaveform")).unwrap(), "pencil");
    assert!(r.tree.read("/etc/sp9/helper.toml").contains("ink_waveform = \"pencil\""));
    for bad in ["eraser", "", "Brush"] {
        assert!(err_name(r.call(P_PEN, "Pen", "SetInkWaveform", &(bad,))).ends_with("InvalidArgs"), "{bad}");
    }
    // the dead zone: off unless set
    assert_eq!(f64::try_from(r.get(P_PEN, "Pen", "SpeedDeadZone")).unwrap(), 0.0);
    r.call(P_PEN, "Pen", "SetSpeedDeadZone", &(15.0f64,)).unwrap();
    assert_eq!(f64::try_from(r.get(P_PEN, "Pen", "SpeedDeadZone")).unwrap(), 15.0);
    assert!(r.tree.read("/etc/sp9/helper.toml").contains("speed_dead_zone_mm_s = 15.0"));
    for bad in [-1.0f64, 150.0, f64::NAN] {
        assert!(err_name(r.call(P_PEN, "Pen", "SetSpeedDeadZone", &(bad,))).ends_with("InvalidArgs"));
    }
    r.call(P_PEN, "Pen", "SetSpeedDeadZone", &(0.0f64,)).unwrap();
    r.call(P_PEN, "Pen", "SetEraserClick", &(false,)).unwrap();
    assert!(!bool::try_from(r.get(P_PEN, "Pen", "EraserClick")).unwrap());
    assert!(r.tree.read("/etc/sp9/helper.toml").contains("eraser_click = false"));

    assert!(!bool::try_from(r.get(P_PEN, "Pen", "AutoCouple")).unwrap());
    r.call(P_PEN, "Pen", "SetAutoCouple", &(true,)).unwrap();
    assert!(bool::try_from(r.get(P_PEN, "Pen", "AutoCouple")).unwrap());
    assert!(r.tree.read("/etc/sp9/helper.toml").contains("auto_couple = true"));
    assert_eq!(String::try_from(r.get(P_PEN, "Pen", "CoupleState")).unwrap(), "idle");
    assert_eq!(String::try_from(r.get(P_PEN, "Pen", "CoupleResult")).unwrap(), "");
    assert_eq!(String::try_from(r.get(P_PEN, "Pen", "CoupledAddress")).unwrap(), "");
    // no Bluetooth, no cover node to read in a test: the method refuses, nothing is written
    for method in ["Couple", "CoupleForce"] {
        let e = err_name(r.call(P_PEN, "Pen", method, &()));
        assert!(e.ends_with("NotSupported"), "{method}: {e}");
    }
    let h = r.hid.0.lock().unwrap();
    assert!(h.feature_writes.is_empty() && h.writes.is_empty());
}

#[test]
fn couple_request_reaches_the_thread_state() {
    let c = crate::couple::Couple::default();
    assert_eq!(c.status().state, crate::couple::STATE_IDLE);
    c.request(false);
    assert_eq!(c.status().state, crate::couple::STATE_WAITING);
}

/// The pen thread end to end: a FIFO stands in for iptsd's stylus event node.
#[cfg(target_os = "linux")]
#[test]
fn stylus_events_reach_the_digitizer_and_the_pen() {
    use std::io::Write;
    let tree = FakeTree::new("penwatch");
    tree.file("/etc/sp9/helper.toml", "[pen]\nink_feedback = true\n")
        .file("/sys/class/input/event9/device/name", "IPTSD Virtual Stylus 045E:0C52\n")
        .file(pen_feedback::LEGACY_FHID, "0x0143\n");
    let fifo = tree.root.p("/dev/input/event9");
    std::fs::create_dir_all(fifo.parent().unwrap()).unwrap();
    let c = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
    // SAFETY: a valid path.
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);

    let hid = FakeHid::default();
    {
        use sp9_helper_core::hid::build::*;
        let digitizer =
            cat(&[page(0xFF00), usage(0x01), collection(1), id(9), usage(0xC9), lmin(0), lmax(255), size(8), count(63), feature(2), end()]);
        let mut h = hid.0.lock().unwrap();
        let node = |name: &str, bus, product, descriptor| HidNode {
            name: name.into(),
            bus,
            vendor: 0x045E,
            product,
            hid_name: String::new(),
            descriptor,
        };
        h.nodes.push(node("hidraw8", 1, pen_feedback::DIGITIZER_PRODUCT, digitizer));
        h.nodes.push(node("hidraw7", 5, pen::PEN_PRODUCT, pen::fake_pen_descriptor()));
    }
    let mut b = Backends::fake();
    b.hid = Box::new(hid.clone());
    let mut shared = Shared::new(b, tree.root.clone(), tree.root.p("/etc/sp9/helper.toml"), true);
    shared.state_dir = tree.root.p("/var/lib/sp9");
    penwatch::spawn(Arc::new(shared));

    // blocks until the thread has the node open
    let mut w = std::fs::OpenOptions::new().write(true).open(&fifo).unwrap();
    let ev = |ty: u16, code: u16, value: i32| {
        let mut e = [0u8; 24];
        e[16..18].copy_from_slice(&ty.to_ne_bytes());
        e[18..20].copy_from_slice(&code.to_ne_bytes());
        e[20..24].copy_from_slice(&value.to_ne_bytes());
        e
    };
    let mut frame = |events: &[[u8; 24]]| {
        w.write_all(&events.concat()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(40));
    };
    let syn = ev(0, 0, 0);
    frame(&[ev(1, 0x140, 1), ev(3, 0, 100), ev(3, 1, 100), syn]); // pen in range
    frame(&[ev(1, 0x14a, 1), syn]); // tip down
    frame(&[ev(1, 0x14a, 0), syn]); // tip up
    frame(&[ev(1, 0x140, 0), syn]); // out of range
    frame(&[ev(1, 0x141, 1), syn]); // eraser end in range
    frame(&[ev(1, 0x14a, 1), syn]); // eraser down: one click
    frame(&[ev(1, 0x14a, 0), syn]); // eraser up: nothing
    std::thread::sleep(std::time::Duration::from_millis(200));

    use pen_feedback::PenState::{Eraser, Hover, Tip};
    let h = hid.0.lock().unwrap();
    let f: Vec<&Vec<u8>> = h.feature_writes.iter().map(|(n, d)| (assert_eq!(n, "hidraw8"), d).1).collect();
    assert_eq!(f.len(), 6, "{} feature writes", f.len());
    assert_eq!(f[0], &pen_feedback::display_states(0x143, 0).to_vec(), "the FastHostId of the standalone daemon, first");
    for (k, state) in [Hover, Tip(0), Hover, Eraser, Hover].into_iter().enumerate() {
        assert_eq!(f[k + 1], &pen_feedback::pen_feedback(k as u8, 0x143, state).to_vec(), "frame {k}");
    }
    let pen_writes: Vec<Vec<u8>> = h.writes.iter().map(|(n, d)| (assert_eq!(n, "hidraw7"), d.clone()).1).collect();
    assert_eq!(
        pen_writes,
        [vec![0x41, 0, 0x7F, 13, 0, 0, 0], vec![0x41, 0, 0xFE, 2, 0, 0, 0], vec![0x41, 0, 0x7F, 3, 0, 0, 0]],
        "brush start (fixed ordinal: the fake pen has no list), stop, eraser click"
    );
    assert_eq!(tree.read("/var/lib/sp9/pen-fast-host-id"), "0x0143");
}
