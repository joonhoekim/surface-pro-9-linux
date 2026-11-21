// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
//! Temperatures and fan speed from the SAM hwmon drivers, and the skin-temperature guard.
//!
//! - `surface_temp` (hwmon name `surface_thermal`; channel n is SAM TMP instance n, 11 on this
//!   device) and `surface_fan` (FAN 0x01, read only): docs/research/gaps/sam-platform.md 1.5.
//!   The labels come from the firmware and do not identify a channel: it answers `I_RTS1`–
//!   `I_RTS8` for 1–8 and `I_RTS1`–`I_RTS3` again for 9–11, where ACPI names VTS1–VTS3.
//! - Whether a fan rpm request (`ssam::fan_rpm_request`) is being followed is judged from the
//!   `surface_fan` reading, because the SAM accepts the request either way.
//! - The guard imitates the Windows skin temperature control on VTS1 where it was measured
//!   (docs/research/control/power-thermal.md 2, 4.2): Better Performance 47 °C → PL1 23 W, Best
//!   Performance 48 °C → 25.5 W, on AC. Other profiles have no guard. **Experimental**: the
//!   controller constants are not from Windows and nothing has been measured on Linux.

use crate::perf::{Limits, PL1_MIN_W};
use crate::sys::Root;
use std::sync::{Arc, Mutex};

pub const HWMON: &str = "/sys/class/hwmon";
/// hwmon names of the SAM temperature driver (the name has changed between versions).
pub const TEMP_NAMES: [&str; 2] = ["surface_temp", "surface_thermal"];
pub const FAN_NAME: &str = "surface_fan";
/// The skin sensor Windows regulates on (VTS1), as a hwmon channel. VTS1 is SAM TMP instance 9
/// on the Windows side three times over: ACPI `TZ09` (`_UID` 9, `_STR` "VTS1"), the power
/// tracker event name `I_VTS_SAM_9`, and "VTS offset configuration failed. Instance 9". Its
/// label cannot select it: `I_RTS1` is also the label of temp1, another sensor (59.6 °C when
/// temp9 read 48.7 °C, CPU load on AC).
pub const SKIN_SENSOR: &str = "temp9";

/// Name of a channel in `ThermalBackend::temps`: "temp9 (I_RTS1)", or "temp9" without a label.
pub fn channel_name(n: u32, label: Option<&str>) -> String {
    match label {
        Some(l) => format!("temp{n} ({l})"),
        None => format!("temp{n}"),
    }
}

/// Whether `sensor` (a channel "temp9", or a label) selects the channel called `name`.
fn selects(name: &str, sensor: &str) -> bool {
    match name.split_once(" (") {
        Some((chan, label)) => chan == sensor || label.strip_suffix(')') == Some(sensor),
        None => name == sensor,
    }
}

pub trait ThermalBackend: Send + Sync {
    /// (`channel_name`, °C) of every SAM temperature channel, in channel order.
    fn temps(&self) -> Vec<(String, f64)>;
    fn fan_rpm(&self) -> Option<u32>;
    /// `sensor` is a channel ("temp9") or a label; a label shared by several channels gives
    /// the lowest one.
    fn skin_c(&self, sensor: &str) -> Option<f64> {
        self.temps().into_iter().find(|(name, _)| selects(name, sensor)).map(|(_, c)| c)
    }
}

pub struct SysfsThermal {
    root: Root,
}

impl SysfsThermal {
    pub fn new(root: Root) -> Self {
        SysfsThermal { root }
    }

    fn hwmon_named(&self, names: &[&str]) -> Vec<String> {
        self.root
            .list(HWMON)
            .into_iter()
            .filter(|h| self.root.read_opt(&format!("{HWMON}/{h}/name")).is_some_and(|n| names.contains(&n.as_str())))
            .collect()
    }
}

impl ThermalBackend for SysfsThermal {
    fn temps(&self) -> Vec<(String, f64)> {
        let mut out = Vec::new();
        for h in self.hwmon_named(&TEMP_NAMES) {
            let mut chans: Vec<u32> = self
                .root
                .list(&format!("{HWMON}/{h}"))
                .iter()
                .filter_map(|f| f.strip_prefix("temp")?.strip_suffix("_input")?.parse().ok())
                .collect();
            chans.sort();
            for n in chans {
                let Some(m) = self.root.read_i64(&format!("{HWMON}/{h}/temp{n}_input")) else { continue };
                let label = self.root.read_opt(&format!("{HWMON}/{h}/temp{n}_label"));
                out.push((channel_name(n, label.as_deref()), m as f64 / 1000.0));
            }
        }
        out
    }

    fn fan_rpm(&self) -> Option<u32> {
        let h = self.hwmon_named(&[FAN_NAME]).into_iter().next()?;
        self.root.read_i64(&format!("{HWMON}/{h}/fan1_input")).map(|v| v.max(0) as u32)
    }
}

#[derive(Clone, Debug, Default)]
pub struct FakeThermalState {
    pub temps: Vec<(String, f64)>,
    pub fan: Option<u32>,
}

#[derive(Clone, Default)]
pub struct FakeThermal(pub Arc<Mutex<FakeThermalState>>);

impl FakeThermal {
    pub fn set_skin(&self, c: f64) {
        let mut s = self.0.lock().unwrap();
        s.temps.retain(|(name, _)| !selects(name, SKIN_SENSOR));
        s.temps.push((channel_name(9, Some("I_RTS1")), c));
    }
}

impl ThermalBackend for FakeThermal {
    fn temps(&self) -> Vec<(String, f64)> {
        self.0.lock().unwrap().temps.clone()
    }
    fn fan_rpm(&self) -> Option<u32> {
        self.0.lock().unwrap().fan
    }
}

/// A request is judged this long after it was sent: a stopped fan took about 30 s to reach
/// 3992 rpm for 4000, and readings 22–25 s after a request were still rising.
pub const FAN_SETTLE_S: f64 = 30.0;
/// The fan counts as following the request within this fraction of it. Settled readings were
/// within 2 % (2977 for 3000, 4569 for 4600, 5958 for 6000, 7407 for 7500); an ignored request
/// was 15 % or more away (about 5950 rpm against 7000 and 2000).
pub const FAN_TOLERANCE: f64 = 0.10;

/// What became of the fan rpm request, as far as the fan speed shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FanRequestState {
    /// no request in force
    #[default]
    None,
    /// sent less than `FAN_SETTLE_S` ago
    Settling,
    /// the fan runs at the requested speed
    Honoured,
    /// The fan does not run at the requested speed: the SAM is driving it itself, or the
    /// kernel cancelled the request on resume.
    NotHonoured,
    /// no `surface_fan` reading to judge by
    Unknown,
}

impl FanRequestState {
    pub fn as_str(&self) -> &'static str {
        match self {
            FanRequestState::None => "none",
            FanRequestState::Settling => "settling",
            FanRequestState::Honoured => "honoured",
            FanRequestState::NotHonoured => "not-honoured",
            FanRequestState::Unknown => "unknown",
        }
    }
}

/// State of a request of `request` rpm (0 = none) sent `age_s` ago, given the fan speed now.
/// A SAM that happens to drive the fan at the requested speed is indistinguishable from one
/// following the request, and reads as `Honoured`.
pub fn fan_request_state(request: u16, age_s: f64, rpm: Option<u32>) -> FanRequestState {
    if request == 0 {
        return FanRequestState::None;
    }
    if age_s < FAN_SETTLE_S {
        return FanRequestState::Settling;
    }
    match rpm {
        None => FanRequestState::Unknown,
        Some(r) if (r as f64 - request as f64).abs() <= FAN_TOLERANCE * request as f64 => FanRequestState::Honoured,
        Some(_) => FanRequestState::NotHonoured,
    }
}

/// Skin set point (°C) and the first PL1 when the guard engages (W), by profile: the values
/// measured on Windows (power-thermal.md 2, 3). None: not measured, no guard for that profile.
pub fn guard_params(profile: &str) -> Option<(f64, f64)> {
    match profile {
        "balanced-performance" => Some((47.0, 23.0)),
        "performance" => Some((48.0, 25.5)),
        _ => None,
    }
}
/// Proportional and integral gains of the PL1 loop (not from Windows).
pub const KP_W_PER_C: f64 = 1.5;
pub const KI_W_PER_C_S: f64 = 0.1;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SkinGuard {
    pub engaged: bool,
    integral: f64,
    /// PL1 the loop wants (W) while engaged
    pub pl1_w: f64,
}

impl SkinGuard {
    /// One control step: `table` are the limits of the profile, `t` the skin temperature,
    /// `dt` seconds since the last step. Returns the limits to apply (the table unchanged for a
    /// profile without measured guard values).
    pub fn step(&mut self, profile: &str, table: Limits, t: f64, dt: f64) -> Limits {
        let Some((sp, first)) = guard_params(profile) else {
            *self = SkinGuard::default();
            return table;
        };
        let first = first.min(table.pl1_w);
        let err = sp - t; // negative when too hot
        if !self.engaged && t >= sp {
            self.engaged = true;
            self.integral = 0.0;
        }
        if self.engaged {
            let i_max = (table.pl1_w - first) / KI_W_PER_C_S;
            let i_min = (PL1_MIN_W - first) / KI_W_PER_C_S;
            self.integral = (self.integral + err * dt).clamp(i_min.min(0.0), i_max.max(0.0));
            self.pl1_w = (first + KP_W_PER_C * err + KI_W_PER_C_S * self.integral).clamp(PL1_MIN_W, table.pl1_w);
            if t < sp - 1.0 && self.pl1_w >= table.pl1_w {
                self.engaged = false;
                self.integral = 0.0;
            }
        }
        let mut out = table;
        if self.engaged {
            out.pl1_w = self.pl1_w.min(table.pl1_w);
        }
        // round to 0.1 W so small loop movements do not cause a write every second
        out.pl1_w = (out.pl1_w * 10.0).round() / 10.0;
        out.pl2_w = (out.pl2_w * 10.0).round() / 10.0;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::perf::default_limits;
    use crate::sys::FakeTree;

    #[test]
    fn hwmon_scan() {
        let t = FakeTree::new("hwmon");
        // the labels this device reports: 9 repeats 1
        t.file(&format!("{HWMON}/hwmon3/name"), "surface_thermal\n")
            .file(&format!("{HWMON}/hwmon3/temp1_input"), "59600\n")
            .file(&format!("{HWMON}/hwmon3/temp1_label"), "I_RTS1\n")
            .file(&format!("{HWMON}/hwmon3/temp2_input"), "55800\n")
            .file(&format!("{HWMON}/hwmon3/temp9_input"), "46500\n")
            .file(&format!("{HWMON}/hwmon3/temp9_label"), "I_RTS1\n")
            .file(&format!("{HWMON}/hwmon3/temp10_input"), "45800\n")
            .file(&format!("{HWMON}/hwmon3/temp10_label"), "I_RTS2\n")
            .file(&format!("{HWMON}/hwmon4/name"), "surface_fan\n")
            .file(&format!("{HWMON}/hwmon4/fan1_input"), "3200\n")
            .file(&format!("{HWMON}/hwmon5/name"), "coretemp\n")
            .file(&format!("{HWMON}/hwmon5/temp1_input"), "90000\n");
        let th = SysfsThermal::new(t.root.clone());
        let names: Vec<String> = th.temps().into_iter().map(|t| t.0).collect();
        assert_eq!(names, ["temp1 (I_RTS1)", "temp2", "temp9 (I_RTS1)", "temp10 (I_RTS2)"]);
        assert_eq!(th.skin_c(SKIN_SENSOR), Some(46.5));
        // a channel without a label, a label, and a label two channels share (lowest wins)
        assert_eq!(th.skin_c("temp2"), Some(55.8));
        assert_eq!(th.skin_c("I_RTS2"), Some(45.8));
        assert_eq!(th.skin_c("I_RTS1"), Some(59.6));
        // "temp1" must not select temp10, and the old default label is not on this device
        assert_eq!(th.skin_c("temp1"), Some(59.6));
        assert_eq!(th.skin_c("VTS1"), None);
        assert_eq!(th.fan_rpm(), Some(3200));
    }

    #[test]
    fn fan_request_judged_after_settling() {
        use FanRequestState::*;
        assert_eq!(fan_request_state(0, 100.0, Some(3000)), None);
        assert_eq!(fan_request_state(4000, 0.0, Some(0)), Settling);
        assert_eq!(fan_request_state(4000, FAN_SETTLE_S - 1.0, Some(3992)), Settling);
        // the measured pairs
        for (req, rpm) in [(3000, 2977), (4600, 4569), (5000, 4988), (6000, 5958), (7500, 7407)] {
            assert_eq!(fan_request_state(req, FAN_SETTLE_S, Some(rpm)), Honoured, "{req}");
        }
        // the SAM driving the fan at about 5950 rpm, and a request cancelled on resume
        assert_eq!(fan_request_state(7000, 40.0, Some(5950)), NotHonoured);
        assert_eq!(fan_request_state(2000, 40.0, Some(5950)), NotHonoured);
        assert_eq!(fan_request_state(4000, 40.0, Some(0)), NotHonoured);
        assert_eq!(fan_request_state(4000, 40.0, Option::None), Unknown);
    }

    #[test]
    fn guard_cold_does_nothing() {
        let table = default_limits("performance", true).unwrap();
        let mut g = SkinGuard::default();
        let out = g.step("performance", table, 40.0, 1.0);
        assert_eq!(out, table);
        assert!(!g.engaged);
    }

    #[test]
    fn guard_engages_and_recovers() {
        let table = default_limits("performance", true).unwrap();
        let mut g = SkinGuard::default();
        let out = g.step("performance", table, 48.2, 1.0);
        assert!(g.engaged);
        assert!(out.pl1_w <= 25.5 && out.pl1_w > 20.0, "{out:?}");
        // stays hot: PL1 keeps falling, never below the floor
        let mut last = out.pl1_w;
        for _ in 0..600 {
            last = g.step("performance", table, 48.5, 1.0).pl1_w;
        }
        assert!(last < out.pl1_w);
        assert!(last >= PL1_MIN_W);
        // cools down: back to the table
        let mut out = table;
        for _ in 0..2000 {
            out = g.step("performance", table, 44.0, 1.0);
        }
        assert!(!g.engaged);
        assert_eq!(out, table);
    }

    #[test]
    fn guard_only_where_measured() {
        // balanced: no measured set point, the table passes through even when hot
        let table = default_limits("balanced", true).unwrap();
        let mut g = SkinGuard::default();
        assert_eq!(g.step("balanced", table, 55.0, 1.0), table);
        assert!(!g.engaged);
        // balanced-performance engages at 47 °C and starts at 23 W; PL2 is left alone
        let table = default_limits("balanced-performance", true).unwrap();
        let o = g.step("balanced-performance", table, 47.1, 1.0);
        assert!(g.engaged);
        assert!(o.pl1_w <= 23.0 && o.pl1_w > 20.0, "{o:?}");
        assert_eq!(o.pl2_w, table.pl2_w);
    }
}
