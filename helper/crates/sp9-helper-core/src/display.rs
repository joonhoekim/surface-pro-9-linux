// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
//! Display: only what the desktop does not own (docs/research/control/display.md).
//!
//! - PSR by power source. Windows turns PSR off on AC (`PsrDisableInAc=1`) and keeps PSR1 on
//!   battery. `i915.enable_psr` is a boot parameter (userspace/platform cmdline sets 1); at run
//!   time the only switch is debugfs `i915_edp_psr_debug` (`I915_PSR_DEBUG_DEFAULT` 0x00,
//!   `DISABLE` 0x01, `FORCE_PSR1` 0x03). Policy "default" never writes.
//! - The panel's own colour space as an ICC profile from the factory calibration in UEFI
//!   (`Type_1`/`Type_2`), made by tools/sdc-blob.py.
//! - Whether the panel is on (DRM connector `dpms`), for the touch-off policy.

use crate::sys::Root;
use serde::{Deserialize, Serialize};
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub const DEBUGFS_DRI: &str = "/sys/kernel/debug/dri";
pub const ENABLE_PSR_PARAM: &str = "/sys/module/i915/parameters/enable_psr";
pub const DRM: &str = "/sys/class/drm";
pub const EFIVARS: &str = "/sys/firmware/efi/efivars";
pub const CAL_MEASUREMENT: &str = "Type_1-0c3b397f-4162-4ed2-9aaa-5baf400e3aaf";
pub const CAL_CONFIG: &str = "Type_2-0c3b397f-4162-4ed2-9aaa-5baf400e3aaf";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum PsrMode {
    /// leave the driver alone (writes 0x00 once if the helper wrote something before)
    #[default]
    Default,
    Off,
    Psr1,
}

impl PsrMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "default" => Some(PsrMode::Default),
            "off" => Some(PsrMode::Off),
            "psr1" => Some(PsrMode::Psr1),
            _ => None,
        }
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            PsrMode::Default => "default",
            PsrMode::Off => "off",
            PsrMode::Psr1 => "psr1",
        }
    }
    /// The `i915_edp_psr_debug` value.
    pub fn debug_value(&self) -> u32 {
        match self {
            PsrMode::Default => 0x00,
            PsrMode::Off => 0x01,
            PsrMode::Psr1 => 0x03,
        }
    }
}

pub trait DisplayBackend: Send + Sync {
    /// `i915.enable_psr` as booted (-1 auto, 0 off, 1 PSR1, 2 PSR2).
    fn enable_psr_param(&self) -> Option<i32>;
    /// The "PSR mode" line of `i915_edp_psr_status` (root, debugfs mounted).
    fn psr_status(&self) -> Option<String>;
    fn set_psr_debug(&self, v: u32) -> io::Result<()>;
    /// The internal panel's DRM connector is on. None: no eDP connector.
    fn panel_on(&self) -> Option<bool>;
    /// Paths of the calibration variables (measurement, configuration) when present.
    fn calibration(&self) -> Option<(PathBuf, Option<PathBuf>)>;
}

pub struct SysfsDisplay {
    root: Root,
}

impl SysfsDisplay {
    pub fn new(root: Root) -> Self {
        SysfsDisplay { root }
    }

    fn debugfs_file(&self, name: &str) -> Option<String> {
        self.root.list(DEBUGFS_DRI).into_iter().map(|d| format!("{DEBUGFS_DRI}/{d}/{name}")).find(|p| self.root.exists(p))
    }
}

impl DisplayBackend for SysfsDisplay {
    fn enable_psr_param(&self) -> Option<i32> {
        self.root.read_i64(ENABLE_PSR_PARAM).map(|v| v as i32)
    }

    fn psr_status(&self) -> Option<String> {
        let f = self.debugfs_file("i915_edp_psr_status")?;
        let s = self.root.read_opt(&f)?;
        let line = s.lines().find(|l| l.starts_with("PSR mode")).or_else(|| s.lines().next())?;
        Some(line.trim().to_string())
    }

    fn set_psr_debug(&self, v: u32) -> io::Result<()> {
        let f = self
            .debugfs_file("i915_edp_psr_debug")
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no i915_edp_psr_debug (debugfs not mounted or no i915 eDP)"))?;
        self.root.write(&f, &format!("{v:#x}"))
    }

    fn panel_on(&self) -> Option<bool> {
        let c = self.root.list(DRM).into_iter().find(|c| c.contains("-eDP-"))?;
        let dpms = self.root.read_opt(&format!("{DRM}/{c}/dpms"))?;
        Some(dpms == "On")
    }

    fn calibration(&self) -> Option<(PathBuf, Option<PathBuf>)> {
        let m = self.root.p(&format!("{EFIVARS}/{CAL_MEASUREMENT}"));
        let c = self.root.p(&format!("{EFIVARS}/{CAL_CONFIG}"));
        m.exists().then(|| (m, c.exists().then_some(c)))
    }
}

/// The command that writes the panel ICC profile: `python3 -I sdc-blob.py T1 [T2] --icc OUT`.
pub fn icc_command(python: &str, script: &str, cal: &(PathBuf, Option<PathBuf>), out: &str) -> Vec<String> {
    let mut v = vec![python.to_string(), "-I".into(), script.to_string(), cal.0.display().to_string()];
    if let Some(c) = &cal.1 {
        v.push(c.display().to_string());
    }
    v.extend(["--icc".to_string(), out.to_string()]);
    v
}

#[derive(Clone, Debug, Default)]
pub struct FakeDisplayState {
    pub enable_psr: Option<i32>,
    pub psr_writes: Vec<u32>,
    pub panel_on: Option<bool>,
    pub calibration: Option<(PathBuf, Option<PathBuf>)>,
}

#[derive(Clone, Default)]
pub struct FakeDisplay(pub Arc<Mutex<FakeDisplayState>>);

impl DisplayBackend for FakeDisplay {
    fn enable_psr_param(&self) -> Option<i32> {
        self.0.lock().unwrap().enable_psr
    }
    fn psr_status(&self) -> Option<String> {
        None
    }
    fn set_psr_debug(&self, v: u32) -> io::Result<()> {
        self.0.lock().unwrap().psr_writes.push(v);
        Ok(())
    }
    fn panel_on(&self) -> Option<bool> {
        self.0.lock().unwrap().panel_on
    }
    fn calibration(&self) -> Option<(PathBuf, Option<PathBuf>)> {
        self.0.lock().unwrap().calibration.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sys::FakeTree;

    #[test]
    fn sysfs_display() {
        let t = FakeTree::new("disp");
        t.file(ENABLE_PSR_PARAM, "1\n")
            .file(&format!("{DEBUGFS_DRI}/0/i915_edp_psr_debug"), "0\n")
            .file(&format!("{DEBUGFS_DRI}/0/i915_edp_psr_status"), "Sink support: PSR = yes\nPSR mode: PSR1 enabled\n")
            .file(&format!("{DRM}/card1-eDP-1/dpms"), "Off\n")
            .file(&format!("{EFIVARS}/{CAL_MEASUREMENT}"), [7u8, 0, 0, 0, 1]);
        let d = SysfsDisplay::new(t.root.clone());
        assert_eq!(d.enable_psr_param(), Some(1));
        assert_eq!(d.psr_status().as_deref(), Some("PSR mode: PSR1 enabled"));
        d.set_psr_debug(PsrMode::Off.debug_value()).unwrap();
        assert_eq!(t.read(&format!("{DEBUGFS_DRI}/0/i915_edp_psr_debug")), "0x1");
        assert_eq!(d.panel_on(), Some(false));
        let cal = d.calibration().unwrap();
        assert!(cal.1.is_none());
        let cmd = icc_command("python3", "/x/sdc-blob.py", &cal, "/var/lib/sp9/display/panel.icc");
        assert_eq!(cmd[0..3], ["python3", "-I", "/x/sdc-blob.py"]);
        assert_eq!(cmd.last().unwrap(), "/var/lib/sp9/display/panel.icc");
    }

    #[test]
    fn psr_mode_names() {
        for m in [PsrMode::Default, PsrMode::Off, PsrMode::Psr1] {
            assert_eq!(PsrMode::parse(m.as_str()), Some(m));
        }
        assert_eq!(PsrMode::parse("psr2"), None);
    }
}
