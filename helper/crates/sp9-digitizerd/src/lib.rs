// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
//! The parts of `sp9-digitizerd` around the processing library: configuration files, the
//! hidraw device, the uinput devices, and the replay of captures for verification.

pub mod conf;
pub mod find;
#[cfg(target_os = "linux")]
pub mod hidraw;
pub mod replay;
pub mod service;
#[cfg(target_os = "linux")]
pub mod uinput;
