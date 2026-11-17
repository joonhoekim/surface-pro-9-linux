// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
//! Device model of the Surface Pro 9 (Intel, `Surface_Pro_9_2038`) for `sp9-helperd`.
//!
//! Every feature sits behind a small backend trait with an implementation on the kernel
//! interface (sysfs, hidraw, the SSAM character device) and a fake for tests. The sysfs
//! implementations take a [`sys::Root`], so they also run against a fake tree
//! (`SP9_SYSFS_ROOT`). Policies (profile limits, skin guard, PSR, touch off) live in
//! [`engine`] and only talk to the traits. No D-Bus here.
//!
//! Where each value comes from: docs/research/control/*.md; the feature table: docs/helper-reference.md.

pub mod battery;
pub mod config;
pub mod display;
pub mod engine;
pub mod hid;
pub mod input;
pub mod pen;
pub mod perf;
pub mod ssam;
pub mod sys;
pub mod thermal;

pub use config::Config;

/// The backends of one device. `system()` drives the machine, `fake()` keeps everything in
/// memory.
pub struct Backends {
    pub power: Box<dyn perf::PowerBackend>,
    pub thermal: Box<dyn thermal::ThermalBackend>,
    pub ssam: Box<dyn ssam::Ssam>,
    pub battery: Box<dyn battery::BatteryBackend>,
    pub display: Box<dyn display::DisplayBackend>,
    pub hid: Box<dyn hid::HidBackend>,
    pub input: Box<dyn input::InputBackend>,
    pub leds: Box<dyn input::LedBackend>,
}

impl Backends {
    /// The kernel interfaces under `root` ("/" on the device).
    pub fn system(root: sys::Root) -> Self {
        Backends {
            power: Box::new(perf::SysfsPower::new(root.clone())),
            thermal: Box::new(thermal::SysfsThermal::new(root.clone())),
            ssam: Box::new(ssam::Cdev::new(root.p(ssam::CDEV))),
            battery: Box::new(battery::SysfsBattery::new(root.clone())),
            display: Box::new(display::SysfsDisplay::new(root.clone())),
            hid: Box::new(hid::Hidraw::new(root.clone())),
            input: Box::new(input::SysfsInput::new(root.clone())),
            leds: Box::new(input::SysfsLeds::new(root)),
        }
    }

    /// In-memory fakes with SP9-like defaults (see each fake).
    pub fn fake() -> Self {
        Backends {
            power: Box::new(perf::FakePower::default()),
            thermal: Box::new(thermal::FakeThermal::default()),
            ssam: Box::new(ssam::FakeSsam::sp9()),
            battery: Box::new(battery::FakeBattery::default()),
            display: Box::new(display::FakeDisplay::default()),
            hid: Box::new(hid::FakeHid::default()),
            input: Box::new(input::FakeInput::default()),
            leds: Box::new(input::FakeLeds::default()),
        }
    }
}
