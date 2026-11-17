// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
//! Performance: `platform_profile` (surface_platform_profile: SAM TMP and fan profile), the
//! package RAPL limits (PL1, PL2, PL1 time window) and the HWP energy/performance preference.
//!
//! The per-profile table and the bounds are the SMF policy of the Windows driver stack
//! (docs/research/control/power-thermal.md 2 and 4.3): PL1 node 51 range 5–35 W, PL2 node 52 up to
//! the 60 W of the highest slider column, tau 1–28 s (the slider values 1, 8, 28 s).

use crate::sys::{self, Root};
use serde::{Deserialize, Serialize};
use std::io;
use std::sync::{Arc, Mutex};

pub const PLATFORM_PROFILE: &str = "/sys/firmware/acpi/platform_profile";
pub const PLATFORM_PROFILE_CHOICES: &str = "/sys/firmware/acpi/platform_profile_choices";
pub const POWERCAP: &str = "/sys/class/powercap";
pub const CPUFREQ: &str = "/sys/devices/system/cpu/cpufreq";
pub const POWER_SUPPLY: &str = "/sys/class/power_supply";

/// The profiles of surface_platform_profile, lowest first.
pub const PROFILES: [&str; 4] = ["low-power", "balanced", "balanced-performance", "performance"];

pub const PL1_MIN_W: f64 = 5.0;
pub const PL1_MAX_W: f64 = 35.0;
pub const PL2_MAX_W: f64 = 60.0;
pub const TAU_MIN_S: f64 = 1.0;
pub const TAU_MAX_S: f64 = 28.0;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Limits {
    pub pl1_w: f64,
    pub pl2_w: f64,
    pub tau_s: f64,
    /// HWP energy/performance preference 0 (performance) ..255 (power)
    pub epp: u8,
}

impl Limits {
    pub const fn new(pl1_w: f64, pl2_w: f64, tau_s: f64, epp: u8) -> Self {
        Limits { pl1_w, pl2_w, tau_s, epp }
    }

    /// Inside the SMF bounds and PL2 >= PL1.
    pub fn validate(&self) -> Result<(), String> {
        let ok = |v: f64| v.is_finite();
        if !(ok(self.pl1_w) && ok(self.pl2_w) && ok(self.tau_s)) {
            return Err("limits must be finite numbers".into());
        }
        if !(PL1_MIN_W..=PL1_MAX_W).contains(&self.pl1_w) {
            return Err(format!("PL1 {} W outside {PL1_MIN_W}..{PL1_MAX_W} W", self.pl1_w));
        }
        if self.pl2_w < self.pl1_w || self.pl2_w > PL2_MAX_W {
            return Err(format!("PL2 {} W outside PL1..{PL2_MAX_W} W", self.pl2_w));
        }
        if !(TAU_MIN_S..=TAU_MAX_S).contains(&self.tau_s) {
            return Err(format!("tau {} s outside {TAU_MIN_S}..{TAU_MAX_S} s", self.tau_s));
        }
        Ok(())
    }
}

/// Windows' values for a profile on AC or battery (power-thermal.md 4.3). `low-power` is the
/// battery saver column (slider 7) on both.
pub fn default_limits(profile: &str, ac: bool) -> Option<Limits> {
    Some(match (profile, ac) {
        ("low-power", _) => Limits::new(10.0, 11.0, 1.0, 184),
        ("balanced", true) => Limits::new(28.0, 35.0, 28.0, 153),
        ("balanced", false) => Limits::new(13.0, 35.0, 8.0, 191),
        ("balanced-performance", true) => Limits::new(28.0, 45.0, 28.0, 115),
        ("balanced-performance", false) => Limits::new(28.0, 35.0, 28.0, 115),
        ("performance", true) => Limits::new(35.0, 60.0, 28.0, 64),
        ("performance", false) => Limits::new(30.0, 45.0, 28.0, 64),
        _ => return None,
    })
}

/// Key of an override in the configuration: "balanced.ac".
pub fn limits_key(profile: &str, ac: bool) -> String {
    format!("{profile}.{}", if ac { "ac" } else { "dc" })
}

/// One RAPL package zone. Values are as the kernel reports them.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct RaplZone {
    /// "intel-rapl:0" (MSR) or "intel-rapl-mmio:0" (MCHBAR)
    pub id: String,
    pub pl1_uw: Option<u64>,
    pub pl2_uw: Option<u64>,
    pub tau_us: Option<u64>,
    pub enabled: Option<bool>,
}

pub trait PowerBackend: Send + Sync {
    fn profile(&self) -> Option<String>;
    fn profile_choices(&self) -> Vec<String>;
    fn set_profile(&self, p: &str) -> io::Result<()>;
    /// Package zones (MSR first, then MMIO when the processor thermal device exposes it).
    fn rapl(&self) -> Vec<RaplZone>;
    /// Write PL1, PL2 and tau to every package zone; returns the zones written.
    fn set_rapl(&self, l: &Limits) -> io::Result<Vec<String>>;
    /// The distinct EPP values over all cpufreq policies.
    fn epp(&self) -> Vec<String>;
    /// Write the EPP to every policy; returns how many.
    fn set_epp(&self, v: u8) -> io::Result<usize>;
    /// A mains supply is online. None: no mains supply found.
    fn on_ac(&self) -> Option<bool>;
}

/// The effective package limits: the lowest over the zones (the CPU follows the lower of MSR
/// and MMIO, power-thermal.md 1.3).
pub fn effective(zones: &[RaplZone]) -> (Option<f64>, Option<f64>, Option<f64>) {
    let min = |f: &dyn Fn(&RaplZone) -> Option<u64>| zones.iter().filter_map(f).min();
    (min(&|z| z.pl1_uw).map(|v| v as f64 / 1e6), min(&|z| z.pl2_uw).map(|v| v as f64 / 1e6), min(&|z| z.tau_us).map(|v| v as f64 / 1e6))
}

pub struct SysfsPower {
    root: Root,
}

impl SysfsPower {
    pub fn new(root: Root) -> Self {
        SysfsPower { root }
    }

    fn zone_dirs(&self) -> Vec<String> {
        // top-level package zones only ("intel-rapl:0", not "intel-rapl:0:0")
        self.root
            .list(POWERCAP)
            .into_iter()
            .filter(|n| {
                let parts: Vec<&str> = n.split(':').collect();
                parts.len() == 2 && (parts[0] == "intel-rapl" || parts[0] == "intel-rapl-mmio")
            })
            .filter(|n| self.root.read_opt(&format!("{POWERCAP}/{n}/name")).is_some_and(|v| v.starts_with("package")))
            .collect()
    }

    /// Index of the constraint with this name ("long_term", "short_term").
    fn constraint(&self, zone: &str, name: &str) -> Option<u32> {
        (0..4).find(|i| self.root.read_opt(&format!("{POWERCAP}/{zone}/constraint_{i}_name")).as_deref() == Some(name))
    }
}

impl PowerBackend for SysfsPower {
    fn profile(&self) -> Option<String> {
        self.root.read_opt(PLATFORM_PROFILE)
    }

    fn profile_choices(&self) -> Vec<String> {
        self.root.read_opt(PLATFORM_PROFILE_CHOICES).map(|s| s.split_whitespace().map(String::from).collect()).unwrap_or_default()
    }

    fn set_profile(&self, p: &str) -> io::Result<()> {
        if !self.profile_choices().iter().any(|c| c == p) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("profile {p} not offered by the kernel")));
        }
        self.root.write(PLATFORM_PROFILE, p)
    }

    fn rapl(&self) -> Vec<RaplZone> {
        let mut zones: Vec<RaplZone> = self
            .zone_dirs()
            .into_iter()
            .map(|id| {
                let rd = |f: String| self.root.read_i64(&format!("{POWERCAP}/{id}/{f}")).map(|v| v.max(0) as u64);
                let lt = self.constraint(&id, "long_term");
                let st = self.constraint(&id, "short_term");
                RaplZone {
                    pl1_uw: lt.and_then(|i| rd(format!("constraint_{i}_power_limit_uw"))),
                    pl2_uw: st.and_then(|i| rd(format!("constraint_{i}_power_limit_uw"))),
                    tau_us: lt.and_then(|i| rd(format!("constraint_{i}_time_window_us"))),
                    enabled: self.root.read_i64(&format!("{POWERCAP}/{id}/enabled")).map(|v| v != 0),
                    id,
                }
            })
            .collect();
        zones.sort_by_key(|z| z.id.contains("mmio"));
        zones
    }

    fn set_rapl(&self, l: &Limits) -> io::Result<Vec<String>> {
        l.validate().map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        let mut done = Vec::new();
        let mut last_err = None;
        for z in self.zone_dirs() {
            let w = |c: Option<u32>, attr: &str, v: f64| -> io::Result<()> {
                let Some(i) = c else { return Ok(()) };
                self.root.write(&format!("{POWERCAP}/{z}/constraint_{i}_{attr}"), &format!("{}", (v * 1e6).round() as u64))
            };
            let lt = self.constraint(&z, "long_term");
            let st = self.constraint(&z, "short_term");
            let r = w(lt, "power_limit_uw", l.pl1_w)
                .and_then(|_| w(st, "power_limit_uw", l.pl2_w))
                .and_then(|_| w(lt, "time_window_us", l.tau_s));
            match r {
                Ok(()) => done.push(z),
                Err(e) => last_err = Some(e),
            }
        }
        match (done.is_empty(), last_err) {
            (true, Some(e)) => Err(e),
            (true, None) => Err(io::Error::new(io::ErrorKind::NotFound, "no RAPL package zone")),
            _ => Ok(done),
        }
    }

    fn epp(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .root
            .list(CPUFREQ)
            .into_iter()
            .filter(|p| p.starts_with("policy"))
            .filter_map(|p| self.root.read_opt(&format!("{CPUFREQ}/{p}/energy_performance_preference")))
            .collect();
        v.sort();
        v.dedup();
        v
    }

    fn set_epp(&self, val: u8) -> io::Result<usize> {
        let mut n = 0;
        for p in self.root.list(CPUFREQ).into_iter().filter(|p| p.starts_with("policy")) {
            let f = format!("{CPUFREQ}/{p}/energy_performance_preference");
            if self.root.exists(&f) {
                self.root.write(&f, &val.to_string())?;
                n += 1;
            }
        }
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::NotFound, "no energy_performance_preference (intel_pstate passive or no HWP)"));
        }
        Ok(n)
    }

    fn on_ac(&self) -> Option<bool> {
        let mut found = None;
        for s in self.root.list(POWER_SUPPLY) {
            if self.root.read_opt(&format!("{POWER_SUPPLY}/{s}/type")).as_deref() == Some("Mains") {
                let on = self.root.read_i64(&format!("{POWER_SUPPLY}/{s}/online")) == Some(1);
                found = Some(found.unwrap_or(false) || on);
            }
        }
        found
    }
}

#[derive(Clone, Debug)]
pub struct FakePowerState {
    pub profile: String,
    pub choices: Vec<String>,
    pub zones: Vec<RaplZone>,
    pub epp: u8,
    /// what another writer left in energy_performance_preference (a name, as
    /// power-profiles-daemon writes); read back until the next set_epp
    pub epp_other: Option<String>,
    /// set_epp fails (EBUSY) and writes nothing
    pub epp_fail: bool,
    pub ac: Option<bool>,
    /// every set_rapl call
    pub rapl_writes: Vec<Limits>,
    pub epp_writes: Vec<u8>,
}

impl Default for FakePowerState {
    fn default() -> Self {
        FakePowerState {
            profile: "balanced".into(),
            choices: PROFILES.iter().map(|s| s.to_string()).collect(),
            zones: vec![RaplZone {
                id: "intel-rapl:0".into(),
                pl1_uw: Some(35_000_000),
                pl2_uw: Some(60_000_000),
                tau_us: Some(27_983_872),
                enabled: Some(true),
            }],
            epp: 128,
            epp_other: None,
            epp_fail: false,
            ac: Some(true),
            rapl_writes: vec![],
            epp_writes: vec![],
        }
    }
}

/// In-memory power backend; clone it to keep a handle for assertions.
#[derive(Clone, Default)]
pub struct FakePower(pub Arc<Mutex<FakePowerState>>);

impl PowerBackend for FakePower {
    fn profile(&self) -> Option<String> {
        Some(self.0.lock().unwrap().profile.clone())
    }
    fn profile_choices(&self) -> Vec<String> {
        self.0.lock().unwrap().choices.clone()
    }
    fn set_profile(&self, p: &str) -> io::Result<()> {
        let mut s = self.0.lock().unwrap();
        if !s.choices.iter().any(|c| c == p) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "no such profile"));
        }
        s.profile = p.into();
        Ok(())
    }
    fn rapl(&self) -> Vec<RaplZone> {
        self.0.lock().unwrap().zones.clone()
    }
    fn set_rapl(&self, l: &Limits) -> io::Result<Vec<String>> {
        l.validate().map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        let mut s = self.0.lock().unwrap();
        s.rapl_writes.push(*l);
        for z in &mut s.zones {
            z.pl1_uw = Some((l.pl1_w * 1e6) as u64);
            z.pl2_uw = Some((l.pl2_w * 1e6) as u64);
            z.tau_us = Some((l.tau_s * 1e6) as u64);
        }
        Ok(s.zones.iter().map(|z| z.id.clone()).collect())
    }
    fn epp(&self) -> Vec<String> {
        let s = self.0.lock().unwrap();
        vec![s.epp_other.clone().unwrap_or_else(|| s.epp.to_string())]
    }
    fn set_epp(&self, v: u8) -> io::Result<usize> {
        let mut s = self.0.lock().unwrap();
        if s.epp_fail {
            return Err(io::Error::other("busy"));
        }
        s.epp = v;
        s.epp_other = None;
        s.epp_writes.push(v);
        Ok(12)
    }
    fn on_ac(&self) -> Option<bool> {
        self.0.lock().unwrap().ac
    }
}

/// The profile shown to people: the bracketed form some kernels use is reduced to the word.
pub fn normalize_profile(s: &str) -> String {
    sys::selected(s).unwrap_or_else(|| s.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sys::FakeTree;

    fn rapl_zone(t: &FakeTree, id: &str) {
        let d = format!("{POWERCAP}/{id}");
        t.file(&format!("{d}/name"), "package-0\n")
            .file(&format!("{d}/enabled"), "1\n")
            .file(&format!("{d}/constraint_0_name"), "long_term\n")
            .file(&format!("{d}/constraint_0_power_limit_uw"), "35000000\n")
            .file(&format!("{d}/constraint_0_time_window_us"), "27983872\n")
            .file(&format!("{d}/constraint_1_name"), "short_term\n")
            .file(&format!("{d}/constraint_1_power_limit_uw"), "60000000\n")
            .file(&format!("{d}/constraint_2_name"), "peak_power\n");
    }

    #[test]
    fn table_is_inside_bounds() {
        for p in PROFILES {
            for ac in [true, false] {
                default_limits(p, ac).unwrap().validate().unwrap();
            }
        }
        assert!(Limits::new(36.0, 60.0, 28.0, 0).validate().is_err());
        assert!(Limits::new(28.0, 20.0, 28.0, 0).validate().is_err());
        assert!(Limits::new(28.0, 61.0, 28.0, 0).validate().is_err());
        assert!(Limits::new(28.0, 35.0, 0.5, 0).validate().is_err());
        assert!(Limits::new(f64::NAN, 35.0, 1.0, 0).validate().is_err());
    }

    #[test]
    #[cfg_attr(windows, ignore = "':' in powercap zone names is not a valid Windows file name")]
    fn sysfs_rapl_read_write() {
        let t = FakeTree::new("rapl");
        rapl_zone(&t, "intel-rapl:0");
        rapl_zone(&t, "intel-rapl-mmio:0");
        t.file(&format!("{POWERCAP}/intel-rapl:0:0/name"), "core\n");
        t.file(&format!("{POWERCAP}/intel-rapl:1/name"), "psys\n");
        let p = SysfsPower::new(t.root.clone());
        let z = p.rapl();
        assert_eq!(z.iter().map(|z| z.id.as_str()).collect::<Vec<_>>(), ["intel-rapl:0", "intel-rapl-mmio:0"]);
        assert_eq!(z[0].pl1_uw, Some(35_000_000));
        assert_eq!(effective(&z), (Some(35.0), Some(60.0), Some(27.983872)));
        let done = p.set_rapl(&Limits::new(28.0, 45.0, 28.0, 115)).unwrap();
        assert_eq!(done.len(), 2);
        assert_eq!(t.read(&format!("{POWERCAP}/intel-rapl-mmio:0/constraint_0_power_limit_uw")), "28000000");
        assert_eq!(t.read(&format!("{POWERCAP}/intel-rapl:0/constraint_1_power_limit_uw")), "45000000");
        assert_eq!(t.read(&format!("{POWERCAP}/intel-rapl:0/constraint_0_time_window_us")), "28000000");
        // psys untouched
        assert!(!t.root.exists(&format!("{POWERCAP}/intel-rapl:1/constraint_0_power_limit_uw")));
        assert!(p.set_rapl(&Limits::new(40.0, 45.0, 28.0, 0)).is_err());
    }

    #[test]
    fn sysfs_profile_epp_ac() {
        let t = FakeTree::new("prof");
        t.file(PLATFORM_PROFILE, "balanced\n")
            .file(PLATFORM_PROFILE_CHOICES, "low-power balanced balanced-performance performance\n")
            .file(&format!("{CPUFREQ}/policy0/energy_performance_preference"), "balance_performance\n")
            .file(&format!("{CPUFREQ}/policy1/energy_performance_preference"), "balance_performance\n")
            .file(&format!("{POWER_SUPPLY}/ADP1/type"), "Mains\n")
            .file(&format!("{POWER_SUPPLY}/ADP1/online"), "0\n")
            .file(&format!("{POWER_SUPPLY}/BAT1/type"), "Battery\n");
        let p = SysfsPower::new(t.root.clone());
        assert_eq!(p.profile().as_deref(), Some("balanced"));
        p.set_profile("performance").unwrap();
        assert_eq!(t.read(PLATFORM_PROFILE), "performance");
        assert!(p.set_profile("turbo").is_err());
        assert_eq!(p.epp(), vec!["balance_performance".to_string()]);
        assert_eq!(p.set_epp(64).unwrap(), 2);
        assert_eq!(t.read(&format!("{CPUFREQ}/policy1/energy_performance_preference")), "64");
        assert_eq!(p.on_ac(), Some(false));
    }
}
