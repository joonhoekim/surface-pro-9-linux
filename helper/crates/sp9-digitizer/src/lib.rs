// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
//! Touch and pen processing for the digitizer of the Surface Pro 9 (Intel, `045E:0C52`).
//!
//! The device sends raw data: capacitive images for touch and, for the pen, windows of
//! complex antenna amplitudes at the pen's tones. This crate turns reports into input events
//! and does no I/O; `sp9-digitizerd` connects it to hidraw and uinput.
//!
//! - [`frame`]: the report parser
//! - [`touch`]: image → contacts
//! - [`pen`]: windows → pen samples, one stage per module
//! - [`output`]: contacts and samples → evdev events of the two virtual devices
//! - [`Digitizer`]: all of it behind `process(report, sink)`
//! - [`config`]: the typed configuration; [`hid`]: the report descriptor
//!
//! The processing derives from iptsd 3.1.0 with the pen patches of this repository
//! (userspace/iptsd), reduced to what this device and its configuration use. See README.md.

pub mod config;
pub mod control;
pub mod digitizer;
pub mod frame;
pub mod hid;
pub mod num;
pub mod output;
pub mod pen;
pub mod touch;

pub use config::{Config, Geometry};
pub use digitizer::Digitizer;
