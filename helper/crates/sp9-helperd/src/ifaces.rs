// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
//! The D-Bus objects of `io.github.joonhoekim.OpenDeviceHelper1` on this device (see
//! docs/helper-reference.md "D-Bus API"). Property getters read the device live; methods
//! check polkit, act, persist the setting and emit PropertiesChanged (properties
//! invalidated, clients read again).

use crate::{jobs, polkit, Shared, DEVICE, VERSION};
use sp9_helper_core::display::PsrMode;
use sp9_helper_core::input::{self, Iptsd};
use sp9_helper_core::perf::{self, Limits, PROFILES};
use sp9_helper_core::{config, cover, engine, pen, pen_feedback, ssam};
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;
use zbus::fdo;
use zbus::interface;
use zbus::message::Header;
use zbus::names::InterfaceName;
use zbus::object_server::SignalEmitter;

pub const ROOT: &str = "/io/github/joonhoekim/OpenDeviceHelper1";
pub const P_PERFORMANCE: &str = "/io/github/joonhoekim/OpenDeviceHelper1/Performance";
pub const P_BATTERY: &str = "/io/github/joonhoekim/OpenDeviceHelper1/Battery";
pub const P_DISPLAY: &str = "/io/github/joonhoekim/OpenDeviceHelper1/Display";
pub const P_INPUT: &str = "/io/github/joonhoekim/OpenDeviceHelper1/Input";
pub const P_PEN: &str = "/io/github/joonhoekim/OpenDeviceHelper1/Pen";
pub const P_DIAG: &str = "/io/github/joonhoekim/OpenDeviceHelper1/Diagnostics";

fn failed(e: impl ToString) -> fdo::Error {
    fdo::Error::Failed(e.to_string())
}
fn invalid(e: impl ToString) -> fdo::Error {
    fdo::Error::InvalidArgs(e.to_string())
}
fn persist(r: std::io::Result<()>) -> fdo::Result<()> {
    r.map_err(|e| failed(format!("saving the settings: {e}")))
}

/// Emit PropertiesChanged with the given properties invalidated (clients read again).
pub async fn invalidate(em: &SignalEmitter<'_>, iface: &str, props: &[&str]) {
    if let Ok(name) = InterfaceName::try_from(iface) {
        let _ = fdo::Properties::properties_changed(em, name, HashMap::new(), Cow::Borrowed(props)).await;
    }
}

/// Every object exposes `snapshot()` (its state-like values as text) for the poller.
pub trait Snapshot {
    const IFACE: &'static str;
    const PROPS: &'static [&'static str];
    fn snapshot(&self) -> String;
}

// ------------------------------------------------------------------ root

pub struct Helper(pub Arc<Shared>);

impl Snapshot for Helper {
    const IFACE: &'static str = "io.github.joonhoekim.OpenDeviceHelper1";
    const PROPS: &'static [&'static str] = &["Device", "Model", "Version", "Features"];
    fn snapshot(&self) -> String {
        self.0.features.join(" ")
    }
}

#[interface(name = "io.github.joonhoekim.OpenDeviceHelper1")]
impl Helper {
    /// "surface-pro-9": which device's objects these are
    #[zbus(property)]
    fn device(&self) -> String {
        DEVICE.into()
    }
    /// DMI product SKU ("Surface_Pro_9_2038")
    #[zbus(property)]
    fn model(&self) -> String {
        self.0.root.read_opt("/sys/class/dmi/id/product_sku").unwrap_or_default()
    }
    #[zbus(property)]
    fn version(&self) -> String {
        VERSION.into()
    }
    /// exported objects under this path
    #[zbus(property)]
    fn features(&self) -> Vec<String> {
        self.0.features.clone()
    }
    /// Read /etc/sp9/helper.toml again and apply it; returns the problems found.
    async fn reload(&self, #[zbus(header)] hdr: Header<'_>, #[zbus(connection)] conn: &zbus::Connection) -> fdo::Result<Vec<String>> {
        polkit::check(conn, &hdr, "reload", self.0.no_polkit).await?;
        let w = self.0.reload();
        let s = self.0.clone();
        blocking::unblock(move || apply_iptsd(&s)).await.map_err(failed)?;
        Ok(w)
    }
}

// ------------------------------------------------------------------ Performance

pub struct Performance(pub Arc<Shared>);

impl Performance {
    fn table(&self) -> Vec<(String, String, f64, f64, f64, u8)> {
        let c = self.0.cfg();
        let mut v = Vec::new();
        for p in PROFILES {
            for ac in [true, false] {
                if let Some(l) = c.performance.limits_for(p, ac) {
                    v.push((p.to_string(), if ac { "ac" } else { "dc" }.to_string(), l.pl1_w, l.pl2_w, l.tau_s, l.epp));
                }
            }
        }
        v
    }

    fn target(&self) -> Option<Limits> {
        let ac = self.0.b.power.on_ac().unwrap_or(true);
        let p = perf::normalize_profile(&self.0.b.power.profile()?);
        engine::target(&self.0.cfg(), &p, ac)
    }
}

impl Snapshot for Performance {
    const IFACE: &'static str = "io.github.joonhoekim.OpenDeviceHelper1.Performance";
    const PROPS: &'static [&'static str] = &[
        "Profile",
        "ProfileChoices",
        "OnAc",
        "FollowProfile",
        "EppEnabled",
        "Pl1W",
        "Pl2W",
        "TauS",
        "RaplZones",
        "TargetPl1W",
        "TargetPl2W",
        "TargetTauS",
        "TargetEpp",
        "Epp",
        "LimitTable",
        "SkinGuard",
        "SkinGuardActive",
        "SkinSensor",
        "SkinTempC",
        "Temperatures",
        "FanRpm",
        "FanControlAllowed",
        "FanRpmRequest",
        "FanRequestState",
    ];
    fn snapshot(&self) -> String {
        let c = self.0.cfg();
        let rt = self.0.rt.lock().unwrap();
        format!(
            "{:?} {:?} {:?} {:?} {:?} {} {} {} {}",
            self.0.b.power.profile(),
            self.0.b.power.on_ac(),
            rt.applied,
            self.0.b.power.epp(),
            c.performance,
            rt.guard_active,
            c.fan.allow_rpm_request,
            rt.fan_request,
            rt.fan_state.as_str()
        )
    }
}

#[interface(name = "io.github.joonhoekim.OpenDeviceHelper1.Performance")]
impl Performance {
    /// platform_profile (low-power, balanced, balanced-performance, performance)
    #[zbus(property)]
    fn profile(&self) -> String {
        self.0.b.power.profile().map(|p| perf::normalize_profile(&p)).unwrap_or_default()
    }
    #[zbus(property)]
    fn profile_choices(&self) -> Vec<String> {
        self.0.b.power.profile_choices()
    }
    #[zbus(property)]
    fn on_ac(&self) -> bool {
        self.0.b.power.on_ac().unwrap_or(true)
    }
    /// the helper applies the profile's limits on every profile or power source change
    #[zbus(property)]
    fn follow_profile(&self) -> bool {
        self.0.cfg().performance.follow_profile
    }
    #[zbus(property)]
    fn epp_enabled(&self) -> bool {
        self.0.cfg().performance.epp
    }
    /// effective package PL1 (lowest over the RAPL zones), W; -1 unknown
    #[zbus(property)]
    fn pl1_w(&self) -> f64 {
        perf::effective(&self.0.b.power.rapl()).0.unwrap_or(-1.0)
    }
    #[zbus(property)]
    fn pl2_w(&self) -> f64 {
        perf::effective(&self.0.b.power.rapl()).1.unwrap_or(-1.0)
    }
    #[zbus(property)]
    fn tau_s(&self) -> f64 {
        perf::effective(&self.0.b.power.rapl()).2.unwrap_or(-1.0)
    }
    /// "intel-rapl:0 35.0/60.0 W 28.0 s" per package zone (MSR, then MMIO)
    #[zbus(property)]
    fn rapl_zones(&self) -> Vec<String> {
        self.0
            .b
            .power
            .rapl()
            .iter()
            .map(|z| {
                let w = |v: Option<u64>| v.map(|v| format!("{:.1}", v as f64 / 1e6)).unwrap_or_else(|| "?".into());
                format!("{} {}/{} W {} s", z.id, w(z.pl1_uw), w(z.pl2_uw), w(z.tau_us))
            })
            .collect()
    }
    /// the table's limits for the current profile and power source (before the skin guard)
    #[zbus(property)]
    fn target_pl1_w(&self) -> f64 {
        self.target().map(|l| l.pl1_w).unwrap_or(-1.0)
    }
    #[zbus(property)]
    fn target_pl2_w(&self) -> f64 {
        self.target().map(|l| l.pl2_w).unwrap_or(-1.0)
    }
    #[zbus(property)]
    fn target_tau_s(&self) -> f64 {
        self.target().map(|l| l.tau_s).unwrap_or(-1.0)
    }
    /// -1 unknown
    #[zbus(property)]
    fn target_epp(&self) -> i32 {
        self.target().map(|l| l.epp as i32).unwrap_or(-1)
    }
    /// energy_performance_preference values now (distinct over the CPUs)
    #[zbus(property)]
    fn epp(&self) -> Vec<String> {
        self.0.b.power.epp()
    }
    /// (profile, "ac"|"dc", PL1 W, PL2 W, tau s, EPP) in force, overrides included
    #[zbus(property)]
    fn limit_table(&self) -> Vec<(String, String, f64, f64, f64, u8)> {
        self.table()
    }
    /// experimental: lower PL1/PL2 by the skin temperature
    #[zbus(property)]
    fn skin_guard(&self) -> bool {
        self.0.cfg().performance.skin_guard
    }
    /// the guard is holding a limit below the table now
    #[zbus(property)]
    fn skin_guard_active(&self) -> bool {
        self.0.rt.lock().unwrap().guard_active
    }
    #[zbus(property)]
    fn skin_sensor(&self) -> String {
        self.0.cfg().performance.skin_sensor
    }
    /// °C, NaN unknown
    #[zbus(property)]
    fn skin_temp_c(&self) -> f64 {
        let label = self.0.cfg().performance.skin_sensor;
        self.0.b.thermal.skin_c(&label).unwrap_or(f64::NAN)
    }
    /// (label, °C) of every SAM sensor
    #[zbus(property)]
    fn temperatures(&self) -> Vec<(String, f64)> {
        self.0.b.thermal.temps()
    }
    /// surface_fan rpm, -1 unknown
    #[zbus(property)]
    fn fan_rpm(&self) -> i32 {
        self.0.b.thermal.fan_rpm().map(|v| v as i32).unwrap_or(-1)
    }
    /// experimental FAN 0x0b rpm request allowed (opt-in)
    #[zbus(property)]
    fn fan_control_allowed(&self) -> bool {
        self.0.cfg().fan.allow_rpm_request
    }
    /// The rpm this daemon requested last, 0 none. Not proof that the fan follows it: see
    /// FanRequestState.
    #[zbus(property)]
    fn fan_rpm_request(&self) -> f64 {
        self.0.rt.lock().unwrap().fan_request as f64
    }
    /// "none"; "settling" (sent less than 30 s ago); "honoured" (FanRpm within 10 % of the
    /// request); "not-honoured" (it is not: the firmware is driving the fan itself, or a resume
    /// cancelled the request); "unknown" (no FanRpm)
    #[zbus(property)]
    fn fan_request_state(&self) -> String {
        self.0.rt.lock().unwrap().fan_state.as_str().into()
    }

    async fn set_profile(
        &self,
        profile: String,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] em: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        polkit::check(conn, &hdr, "performance", self.0.no_polkit).await?;
        self.0.b.power.set_profile(&profile).map_err(invalid)?;
        invalidate(&em, Self::IFACE, Self::PROPS).await;
        Ok(())
    }

    async fn set_follow_profile(
        &self,
        on: bool,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] em: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        polkit::check(conn, &hdr, "performance", self.0.no_polkit).await?;
        persist(self.0.update(|c| c.performance.follow_profile = on))?;
        self.0.rt.lock().unwrap().reset();
        invalidate(&em, Self::IFACE, Self::PROPS).await;
        Ok(())
    }

    async fn set_epp_enabled(
        &self,
        on: bool,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] em: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        polkit::check(conn, &hdr, "performance", self.0.no_polkit).await?;
        persist(self.0.update(|c| c.performance.epp = on))?;
        self.0.rt.lock().unwrap().reset();
        invalidate(&em, Self::IFACE, Self::PROPS).await;
        Ok(())
    }

    /// Override the table for one profile and power source ("ac" or "dc"). Bounds: PL1 5–35 W,
    /// PL1 <= PL2 <= 60 W, tau 1–28 s.
    #[allow(clippy::too_many_arguments)]
    async fn set_limits(
        &self,
        profile: String,
        power: String,
        pl1_w: f64,
        pl2_w: f64,
        tau_s: f64,
        epp: u8,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] em: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        polkit::check(conn, &hdr, "performance-limits", self.0.no_polkit).await?;
        let ac = power_arg(&power)?;
        if !PROFILES.contains(&profile.as_str()) {
            return Err(invalid(format!("unknown profile {profile}")));
        }
        let l = Limits::new(pl1_w, pl2_w, tau_s, epp);
        l.validate().map_err(invalid)?;
        persist(self.0.update(|c| {
            c.performance.limits.insert(perf::limits_key(&profile, ac), l);
        }))?;
        self.0.rt.lock().unwrap().reset();
        invalidate(&em, Self::IFACE, Self::PROPS).await;
        Ok(())
    }

    /// Back to Windows' values for one profile and power source.
    async fn reset_limits(
        &self,
        profile: String,
        power: String,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] em: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        polkit::check(conn, &hdr, "performance-limits", self.0.no_polkit).await?;
        let ac = power_arg(&power)?;
        persist(self.0.update(|c| {
            c.performance.limits.remove(&perf::limits_key(&profile, ac));
        }))?;
        self.0.rt.lock().unwrap().reset();
        invalidate(&em, Self::IFACE, Self::PROPS).await;
        Ok(())
    }

    async fn set_skin_guard(
        &self,
        on: bool,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] em: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        polkit::check(conn, &hdr, "skin-guard", self.0.no_polkit).await?;
        persist(self.0.update(|c| c.performance.skin_guard = on))?;
        invalidate(&em, Self::IFACE, Self::PROPS).await;
        Ok(())
    }

    /// Opt in to (or out of) the experimental fan rpm request. Off withdraws a request in force.
    async fn set_fan_control_allowed(
        &self,
        on: bool,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] em: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        polkit::check(conn, &hdr, "fan-control", self.0.no_polkit).await?;
        if !on && self.0.rt.lock().unwrap().fan_request != 0 {
            ssam::fan_rpm_request(&*self.0.b.ssam, 0).map_err(failed)?;
            self.0.rt.lock().unwrap().set_fan_request(0);
        }
        persist(self.0.update(|c| c.fan.allow_rpm_request = on))?;
        invalidate(&em, Self::IFACE, Self::PROPS).await;
        Ok(())
    }

    /// Experimental: SAM FAN CID 0x0b with this rpm (1600–7500, rounded to a whole number), 0
    /// withdraws. The firmware follows it only while it is not driving the fan itself, a
    /// suspend cancels it, and a restart of the daemon withdraws it: FanRequestState reports
    /// what the fan does. Needs SetFanControlAllowed(true) first.
    async fn request_fan_rpm(
        &self,
        rpm: f64,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] em: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        polkit::check(conn, &hdr, "fan-control", self.0.no_polkit).await?;
        if !self.0.cfg().fan.allow_rpm_request {
            return Err(fdo::Error::NotSupported("fan rpm requests are off (SetFanControlAllowed)".into()));
        }
        // `as u16` would turn NaN into 0 (a release) and saturate what is out of range
        if !(rpm.is_finite() && (0.0..=f64::from(u16::MAX)).contains(&rpm)) {
            return Err(invalid(format!("rpm must be 0 or {}..{}", ssam::FAN_RPM_MIN, ssam::FAN_RPM_MAX)));
        }
        let rpm = rpm.round() as u16;
        ssam::fan_rpm_request(&*self.0.b.ssam, rpm).map_err(|e| match e.kind() {
            std::io::ErrorKind::InvalidInput => invalid(e),
            _ => failed(e),
        })?;
        self.0.rt.lock().unwrap().set_fan_request(rpm);
        invalidate(&em, Self::IFACE, Self::PROPS).await;
        Ok(())
    }
}

fn power_arg(s: &str) -> fdo::Result<bool> {
    match s {
        "ac" => Ok(true),
        "dc" => Ok(false),
        _ => Err(invalid("power must be \"ac\" or \"dc\"")),
    }
}

// ------------------------------------------------------------------ Battery

pub struct Battery(pub Arc<Shared>);

impl Snapshot for Battery {
    const IFACE: &'static str = "io.github.joonhoekim.OpenDeviceHelper1.Battery";
    const PROPS: &'static [&'static str] = &[
        "Present",
        "Status",
        "Capacity",
        "CycleCount",
        "HealthPercent",
        "EnergyFullWh",
        "EnergyFullDesignWh",
        "PowerW",
        "VoltageV",
        "AcOnline",
        "AdapterW",
        "SmartChargingAvailable",
        "SmartChargingSupported",
        "SmartChargingEngaged",
        "SmartChargingStatus",
        "SmartChargingCountdownS",
        "UefiBatteryLimit",
    ];
    fn snapshot(&self) -> String {
        let b = self.0.b.battery.battery();
        format!(
            "{:?} {:?} {:?} {:?}",
            b.as_ref().map(|b| (&b.status, b.capacity, b.cycle_count)),
            self.0.b.battery.adapter(),
            self.0.smart_charging().map(|s| (s.supported, s.engaged)),
            self.0.b.battery.uefi_battery_limit()
        )
    }
}

fn wh(v: Option<u64>) -> f64 {
    v.map(|v| v as f64 / 1e6).unwrap_or(-1.0)
}

#[interface(name = "io.github.joonhoekim.OpenDeviceHelper1.Battery")]
impl Battery {
    #[zbus(property)]
    fn present(&self) -> bool {
        self.0.b.battery.battery().is_some()
    }
    #[zbus(property)]
    fn status(&self) -> String {
        self.0.b.battery.battery().map(|b| b.status).unwrap_or_else(|| "Unknown".into())
    }
    /// %, -1 unknown
    #[zbus(property)]
    fn capacity(&self) -> i32 {
        self.0.b.battery.battery().map(|b| b.capacity).unwrap_or(-1)
    }
    #[zbus(property)]
    fn cycle_count(&self) -> i32 {
        self.0.b.battery.battery().map(|b| b.cycle_count).unwrap_or(-1)
    }
    /// full charge capacity in % of the design capacity, -1 unknown
    #[zbus(property)]
    fn health_percent(&self) -> f64 {
        self.0.b.battery.battery().map(|b| b.health_pct()).unwrap_or(-1.0)
    }
    #[zbus(property)]
    fn energy_full_wh(&self) -> f64 {
        wh(self.0.b.battery.battery().and_then(|b| b.energy_full_uwh))
    }
    #[zbus(property)]
    fn energy_full_design_wh(&self) -> f64 {
        wh(self.0.b.battery.battery().and_then(|b| b.energy_full_design_uwh))
    }
    #[zbus(property)]
    fn power_w(&self) -> f64 {
        wh(self.0.b.battery.battery().and_then(|b| b.power_uw))
    }
    #[zbus(property)]
    fn voltage_v(&self) -> f64 {
        wh(self.0.b.battery.battery().and_then(|b| b.voltage_uv))
    }
    #[zbus(property)]
    fn ac_online(&self) -> bool {
        self.0.b.battery.adapter().is_some_and(|a| a.online)
    }
    /// adapter rating (input_power_limit, kernel patch 0010), W; -1 unknown
    #[zbus(property)]
    fn adapter_w(&self) -> f64 {
        wh(self.0.b.battery.adapter().and_then(|a| a.input_power_limit_uw))
    }
    /// SAM requests possible (/dev/surface/aggregator)
    #[zbus(property)]
    fn smart_charging_available(&self) -> bool {
        self.0.smart_charging().is_ok()
    }
    /// SAM policy bit 0x10
    #[zbus(property)]
    fn smart_charging_supported(&self) -> bool {
        self.0.smart_charging().is_ok_and(|s| s.supported)
    }
    /// holding at about 80 % now (SAM 0x41 & 0x0440)
    #[zbus(property)]
    fn smart_charging_engaged(&self) -> bool {
        self.0.smart_charging().is_ok_and(|s| s.engaged)
    }
    /// raw SAM 0x41 value
    #[zbus(property)]
    fn smart_charging_status(&self) -> u16 {
        self.0.smart_charging().map(|s| s.status_raw).unwrap_or(0)
    }
    #[zbus(property)]
    fn smart_charging_countdown_s(&self) -> u32 {
        self.0.smart_charging().map(|s| s.countdown_s).unwrap_or(0)
    }
    /// UEFI "Battery Limit" (50 %): "on", "off", "unknown". Read only.
    #[zbus(property)]
    fn uefi_battery_limit(&self) -> String {
        match self.0.b.battery.uefi_battery_limit() {
            Some(true) => "on",
            Some(false) => "off",
            None => "unknown",
        }
        .into()
    }

    /// Charge to 100 % once (SAM 0x43). Returns "released", "counting-down" (cannot release
    /// yet), "not-engaged" or "other:N". There is no way to turn it back on; the firmware
    /// engages it again by itself.
    async fn release_smart_charging(
        &self,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] em: SignalEmitter<'_>,
    ) -> fdo::Result<String> {
        polkit::check(conn, &hdr, "smart-charging", self.0.no_polkit).await?;
        if !self.0.b.ssam.available() {
            return Err(fdo::Error::NotSupported(format!("{} missing", ssam::CDEV)));
        }
        let r = ssam::release_smart_charging(&*self.0.b.ssam).map_err(failed)?;
        self.0.forget_smart_charging();
        invalidate(&em, Self::IFACE, Self::PROPS).await;
        Ok(r.as_str())
    }
}

// ------------------------------------------------------------------ Display

pub struct Display(pub Arc<Shared>);

impl Display {
    fn icc_file(&self) -> std::path::PathBuf {
        self.0.state_dir.join("display").join("panel-native.icc")
    }
}

impl Snapshot for Display {
    const IFACE: &'static str = "io.github.joonhoekim.OpenDeviceHelper1.Display";
    const PROPS: &'static [&'static str] =
        &["EnablePsr", "PsrStatus", "PsrAc", "PsrDc", "PsrWritten", "PanelOn", "CalibrationAvailable", "IccPath"];
    fn snapshot(&self) -> String {
        format!(
            "{:?} {:?} {:?} {}",
            self.0.cfg().display,
            self.0.b.display.panel_on(),
            self.0.rt.lock().unwrap().psr_written,
            self.icc_file().exists()
        )
    }
}

#[interface(name = "io.github.joonhoekim.OpenDeviceHelper1.Display")]
impl Display {
    /// i915.enable_psr as booted (-1 auto, 0 off, 1 PSR1, 2 PSR2); -2 unknown
    #[zbus(property)]
    fn enable_psr(&self) -> i32 {
        self.0.b.display.enable_psr_param().unwrap_or(-2)
    }
    /// "PSR mode" line of i915_edp_psr_status ("" without debugfs)
    #[zbus(property)]
    fn psr_status(&self) -> String {
        self.0.b.display.psr_status().unwrap_or_default()
    }
    /// policy on AC: "default" (helper does not touch PSR), "off", "psr1"
    #[zbus(property)]
    fn psr_ac(&self) -> String {
        self.0.cfg().display.psr_ac.as_str().into()
    }
    #[zbus(property)]
    fn psr_dc(&self) -> String {
        self.0.cfg().display.psr_dc.as_str().into()
    }
    /// last value written to i915_edp_psr_debug, -1 none
    #[zbus(property)]
    fn psr_written(&self) -> i32 {
        self.0.rt.lock().unwrap().psr_written.map(|v| v as i32).unwrap_or(-1)
    }
    #[zbus(property)]
    fn panel_on(&self) -> bool {
        self.0.b.display.panel_on().unwrap_or(true)
    }
    /// UEFI Type_1 calibration present
    #[zbus(property)]
    fn calibration_available(&self) -> bool {
        self.0.b.display.calibration().is_some()
    }
    /// the generated profile, "" none yet
    #[zbus(property)]
    fn icc_path(&self) -> String {
        let p = self.icc_file();
        if p.exists() {
            p.display().to_string()
        } else {
            String::new()
        }
    }

    /// PSR policy per power source ("default", "off", "psr1"); applied through debugfs on
    /// every power source change.
    async fn set_psr_policy(
        &self,
        ac: String,
        dc: String,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] em: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        polkit::check(conn, &hdr, "display-psr", self.0.no_polkit).await?;
        let a = PsrMode::parse(&ac).ok_or_else(|| invalid(format!("bad PSR mode {ac}")))?;
        let d = PsrMode::parse(&dc).ok_or_else(|| invalid(format!("bad PSR mode {dc}")))?;
        persist(self.0.update(|c| {
            c.display.psr_ac = a;
            c.display.psr_dc = d;
        }))?;
        invalidate(&em, Self::IFACE, Self::PROPS).await;
        Ok(())
    }

    /// Write the panel's native ICC profile from the factory calibration; returns its path.
    async fn generate_icc(
        &self,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] em: SignalEmitter<'_>,
    ) -> fdo::Result<String> {
        polkit::check(conn, &hdr, "display-icc", self.0.no_polkit).await?;
        let cal = self.0.b.display.calibration().ok_or_else(|| fdo::Error::NotSupported("no UEFI panel calibration".into()))?;
        let (py, data, state) = (self.0.python.clone(), self.0.data_dir.clone(), self.0.state_dir.clone());
        let p = blocking::unblock(move || jobs::icc(&py, &data, &state, &cal)).await.map_err(failed)?;
        invalidate(&em, Self::IFACE, Self::PROPS).await;
        Ok(p.display().to_string())
    }
}

// ------------------------------------------------------------------ Input

pub struct Input(pub Arc<Shared>);

/// Makes the touch and pen settings take effect. The drop-in is written in any case (whoever
/// processes touch reads it when it starts). A running sp9-digitizerd takes the values over
/// D-Bus and is never restarted; without it, iptsd is restarted when the file changed.
pub(crate) fn apply_iptsd(s: &Shared) -> Result<(), String> {
    let input = s.cfg().input;
    let changed = input.iptsd.apply(&input.zero_force, input.pen_prediction_ms, &s.root).map_err(|e| format!("iptsd drop-in: {e}"))?;
    if s.hardware && jobs::push_digitizer_blocking(&input.iptsd, &input.zero_force, input.pen_prediction_ms)? {
        return Ok(());
    }
    if changed {
        jobs::restart_iptsd_blocking()?;
    }
    Ok(())
}

impl Input {
    fn kbd(&self) -> Option<input::KbdBacklight> {
        input::find_backlight(&*self.0.b.leds, &*self.0.b.hid)
    }
}

impl Snapshot for Input {
    const IFACE: &'static str = "io.github.joonhoekim.OpenDeviceHelper1.Input";
    const PROPS: &'static [&'static str] = &[
        "KeyboardBacklightAvailable",
        "KeyboardBacklightBackend",
        "KeyboardBacklight",
        "KeyboardBacklightLevels",
        "TouchOffWhenDisplayOff",
        "TouchInhibited",
        "IptsdActivationThreshold",
        "IptsdDeactivationThreshold",
        "IptsdDisableOnStylus",
        "ZeroForceContact",
        "ZeroForceEnter",
        "ZeroForceExit",
        "ContactMinPressure",
        "ZeroForceReference",
        "PenPredictionMs",
    ];
    fn snapshot(&self) -> String {
        // the LED level is a cheap sysfs read; hidraw level changes come from kbdwatch
        let kbd = self.kbd().map(|k| match &k {
            input::KbdBacklight::Led { name, .. } => format!("{} {:?}", k.backend(), self.0.b.leds.attr(name, "brightness")),
            input::KbdBacklight::Hid(_) => k.backend(),
        });
        format!("{:?} {:?} {:?}", kbd, self.0.cfg().input, self.0.rt.lock().unwrap().touch_inhibited)
    }
}

#[interface(name = "io.github.joonhoekim.OpenDeviceHelper1.Input")]
impl Input {
    /// a keyboard backlight LED or a Type Cover with the backlight collection is present
    #[zbus(property)]
    fn keyboard_backlight_available(&self) -> bool {
        self.kbd().is_some()
    }
    /// "led:<name>", "hidraw:<node>", "" none
    #[zbus(property)]
    fn keyboard_backlight_backend(&self) -> String {
        self.kbd().map(|k| k.backend()).unwrap_or_default()
    }
    /// current level (LED brightness or Feature 214), -1 unknown
    #[zbus(property)]
    fn keyboard_backlight(&self) -> i32 {
        self.kbd().and_then(|k| k.level(&*self.0.b.leds, &*self.0.b.hid)).map(|v| v as i32).unwrap_or(-1)
    }
    /// the values SetKeyboardBacklight takes, 0 first (LED 0..max_brightness, hidraw Feature 213)
    #[zbus(property)]
    fn keyboard_backlight_levels(&self) -> Vec<u32> {
        self.kbd().map(|k| k.levels(&*self.0.b.hid)).unwrap_or_default()
    }
    #[zbus(property)]
    fn touch_off_when_display_off(&self) -> bool {
        self.0.cfg().input.touch_off_when_display_off
    }
    /// the helper is holding the touchscreen inhibited now
    #[zbus(property)]
    fn touch_inhibited(&self) -> bool {
        !self.0.rt.lock().unwrap().touch_inhibited.is_empty()
    }
    /// -1 = the preset's value
    #[zbus(property)]
    fn iptsd_activation_threshold(&self) -> f64 {
        self.0.cfg().input.iptsd.activation_threshold.unwrap_or(-1.0)
    }
    #[zbus(property)]
    fn iptsd_deactivation_threshold(&self) -> f64 {
        self.0.cfg().input.iptsd.deactivation_threshold.unwrap_or(-1.0)
    }
    /// -1 preset, 0 off, 1 on
    #[zbus(property)]
    fn iptsd_disable_on_stylus(&self) -> i32 {
        self.0.cfg().input.iptsd.disable_on_stylus.map(i32::from).unwrap_or(-1)
    }

    async fn set_keyboard_backlight(
        &self,
        level: u32,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] em: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        polkit::check(conn, &hdr, "keyboard-backlight", self.0.no_polkit).await?;
        let k = self.kbd().ok_or_else(|| fdo::Error::NotSupported("no keyboard backlight".into()))?;
        k.set_level(&*self.0.b.leds, &*self.0.b.hid, level).map_err(invalid)?;
        invalidate(&em, Self::IFACE, Self::PROPS).await;
        Ok(())
    }

    async fn set_touch_off_when_display_off(
        &self,
        on: bool,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] em: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        polkit::check(conn, &hdr, "touch", self.0.no_polkit).await?;
        persist(self.0.update(|c| c.input.touch_off_when_display_off = on))?;
        invalidate(&em, Self::IFACE, Self::PROPS).await;
        Ok(())
    }

    /// the pen counts as touching without force once its tip signal is strong enough (iptsd)
    #[zbus(property)]
    fn zero_force_contact(&self) -> bool {
        self.0.cfg().input.zero_force.enabled
    }
    /// contact begins at this percentage of the tip signal of writing with force
    #[zbus(property)]
    fn zero_force_enter(&self) -> f64 {
        self.0.cfg().input.zero_force.enter_percent
    }
    /// contact ends below this percentage
    #[zbus(property)]
    fn zero_force_exit(&self) -> f64 {
        self.0.cfg().input.zero_force.exit_percent
    }
    /// pressure reported while in contact without force, percent of the range
    #[zbus(property)]
    fn contact_min_pressure(&self) -> f64 {
        self.0.cfg().input.zero_force.floor_percent
    }
    /// the reference level, 0 = learnt by iptsd from strokes written with force
    #[zbus(property)]
    fn zero_force_reference(&self) -> f64 {
        self.0.cfg().input.zero_force.reference
    }

    /// sp9-digitizerd reports the pen where it will be this many milliseconds from now; 0 = off
    #[zbus(property)]
    fn pen_prediction_ms(&self) -> f64 {
        self.0.cfg().input.pen_prediction_ms
    }

    /// The pen's prediction horizon in milliseconds (0 = off, at most 30). Only
    /// sp9-digitizerd predicts: it takes the value while it runs, and reads it from
    /// /etc/iptsd.d/zz-sp9-helper.conf when it starts. iptsd does not know the key; it is
    /// restarted when the file changed, like for the other settings of the file.
    async fn set_pen_prediction(
        &self,
        ms: f64,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] em: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        polkit::check(conn, &hdr, "touch", self.0.no_polkit).await?;
        input::validate_prediction(ms).map_err(invalid)?;
        persist(self.0.update(|c| c.input.pen_prediction_ms = ms))?;
        let s = self.0.clone();
        blocking::unblock(move || apply_iptsd(&s)).await.map_err(failed)?;
        invalidate(&em, Self::IFACE, Self::PROPS).await;
        Ok(())
    }

    /// The pen's zero-force contact in iptsd: on or off, enter and exit thresholds in percent
    /// of the reference (1 <= exit < enter <= 150), the pressure floor in percent (0..50) and
    /// the reference level (0 = learn). Writes /etc/iptsd.d/zz-sp9-helper.conf and restarts
    /// iptsd when the file changed.
    #[allow(clippy::too_many_arguments)]
    async fn set_zero_force(
        &self,
        enabled: bool,
        enter: f64,
        exit: f64,
        floor: f64,
        reference: f64,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] em: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        polkit::check(conn, &hdr, "touch", self.0.no_polkit).await?;
        let z = input::ZeroForce { enabled, enter_percent: enter, exit_percent: exit, floor_percent: floor, reference };
        z.validate().map_err(invalid)?;
        persist(self.0.update(|c| c.input.zero_force = z))?;
        let s = self.0.clone();
        blocking::unblock(move || apply_iptsd(&s)).await.map_err(failed)?;
        invalidate(&em, Self::IFACE, Self::PROPS).await;
        Ok(())
    }

    /// iptsd contact thresholds and DisableOnStylus on top of the preset: a threshold < 0 and
    /// disable_on_stylus < 0 mean "the preset's value". Writes /etc/iptsd.d/zz-sp9-helper.conf
    /// and restarts iptsd.
    async fn set_iptsd(
        &self,
        activation: f64,
        deactivation: f64,
        disable_on_stylus: i32,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] em: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        polkit::check(conn, &hdr, "touch", self.0.no_polkit).await?;
        let opt = |v: f64| (v >= 0.0).then_some(v);
        let i = Iptsd {
            activation_threshold: opt(activation),
            deactivation_threshold: opt(deactivation),
            disable_on_stylus: (disable_on_stylus >= 0).then_some(disable_on_stylus != 0),
        };
        i.validate().map_err(invalid)?;
        persist(self.0.update(|c| c.input.iptsd = i))?;
        let s = self.0.clone();
        blocking::unblock(move || apply_iptsd(&s)).await.map_err(failed)?;
        invalidate(&em, Self::IFACE, Self::PROPS).await;
        Ok(())
    }
}

// ------------------------------------------------------------------ Pen

pub struct Pen(pub Arc<Shared>);

impl Snapshot for Pen {
    const IFACE: &'static str = "io.github.joonhoekim.OpenDeviceHelper1.Pen";
    const PROPS: &'static [&'static str] = &[
        "Available",
        "InkFeedback",
        "Intensity",
        "Strength",
        "EraserClick",
        "InkWaveform",
        "InkWaveforms",
        "SpeedFeedback",
        "SpeedFullScale",
        "SpeedDeadZone",
        "DigitizerAvailable",
        "AutoCouple",
        "CoverAvailable",
        "CoupleState",
        "CoupleResult",
        "CoupledAddress",
    ];
    fn snapshot(&self) -> String {
        let h = &*self.0.b.hid;
        format!(
            "{} {} {} {:?} {:?}",
            pen::find(h).is_some(),
            pen_feedback::find_digitizer(h).is_some(),
            cover::find(h).is_some(),
            self.0.cfg().pen,
            self.0.couple.status()
        )
    }
}

#[interface(name = "io.github.joonhoekim.OpenDeviceHelper1.Pen")]
impl Pen {
    /// the pen is connected and has a haptic output report
    #[zbus(property)]
    fn available(&self) -> bool {
        pen::find(&*self.0.b.hid).is_some()
    }
    /// pen haptics as a whole: inking haptics and, with SpeedFeedback, the digitizer feedback
    #[zbus(property)]
    fn ink_feedback(&self) -> bool {
        self.0.cfg().pen.ink_feedback
    }
    /// intensity byte of the inking start, 2..254 (what Windows sends)
    #[zbus(property)]
    fn intensity(&self) -> u32 {
        self.0.cfg().pen.intensity as u32
    }
    /// the intensity in percent of Windows' default (127 = 100)
    #[zbus(property)]
    fn strength(&self) -> f64 {
        pen::intensity_to_strength(self.0.cfg().pen.intensity)
    }
    /// one click when the eraser end touches down
    #[zbus(property)]
    fn eraser_click(&self) -> bool {
        self.0.cfg().pen.eraser_click
    }
    /// the continuous waveform started on tip down: one of InkWaveforms
    #[zbus(property)]
    fn ink_waveform(&self) -> String {
        self.0.cfg().pen.waveform().name().into()
    }
    /// what SetInkWaveform takes
    #[zbus(property)]
    fn ink_waveforms(&self) -> Vec<String> {
        pen::Waveform::ALL.iter().map(|w| w.name().to_string()).collect()
    }
    /// pen speed in mm/s below which the speed value is 0; 0 = no dead zone
    #[zbus(property)]
    fn speed_dead_zone(&self) -> f64 {
        self.0.cfg().pen.speed_dead_zone_mm_s
    }
    /// feed the pen speed back to the digitizer while InkFeedback is on
    #[zbus(property)]
    fn speed_feedback(&self) -> bool {
        self.0.cfg().pen.speed_feedback
    }
    /// pen speed in mm/s that maps to the largest speed value
    #[zbus(property)]
    fn speed_full_scale(&self) -> f64 {
        self.0.cfg().pen.full_scale()
    }
    /// the digitizer's hidraw node with feature report 9 is there
    #[zbus(property)]
    fn digitizer_available(&self) -> bool {
        pen_feedback::find_digitizer(&*self.0.b.hid).is_some()
    }
    /// couple a pen put into the cover slot
    #[zbus(property)]
    fn auto_couple(&self) -> bool {
        self.0.cfg().pen.auto_couple
    }
    /// a cover with the pen-bonding collection is attached
    #[zbus(property)]
    fn cover_available(&self) -> bool {
        cover::find(&*self.0.b.hid).is_some()
    }
    /// "idle", "waiting-for-pen", "coupling"
    #[zbus(property)]
    fn couple_state(&self) -> String {
        self.0.couple.status().state
    }
    /// result of the last coupling attempt, "" none
    #[zbus(property)]
    fn couple_result(&self) -> String {
        self.0.couple.status().result
    }
    /// coupling address of the last pen the cover announced, masked ("XX:XX:XX:XX:XX:B5")
    #[zbus(property)]
    fn coupled_address(&self) -> String {
        self.0.couple.status().address
    }

    async fn set_ink_feedback(
        &self,
        on: bool,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] em: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        polkit::check(conn, &hdr, "pen", self.0.no_polkit).await?;
        persist(self.0.update(|c| c.pen.ink_feedback = on))?;
        invalidate(&em, Self::IFACE, Self::PROPS).await;
        Ok(())
    }

    async fn set_intensity(
        &self,
        intensity: u32,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] em: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        polkit::check(conn, &hdr, "pen", self.0.no_polkit).await?;
        let (lo, hi) = pen::INTENSITY_RANGE;
        if !(lo as u32..=hi as u32).contains(&intensity) {
            return Err(invalid("intensity must be 2..254"));
        }
        persist(self.0.update(|c| c.pen.intensity = intensity as u8))?;
        invalidate(&em, Self::IFACE, Self::PROPS).await;
        Ok(())
    }

    async fn set_speed_feedback(
        &self,
        on: bool,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] em: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        polkit::check(conn, &hdr, "pen", self.0.no_polkit).await?;
        persist(self.0.update(|c| c.pen.speed_feedback = on))?;
        invalidate(&em, Self::IFACE, Self::PROPS).await;
        Ok(())
    }

    /// Strength in percent of Windows' default intensity (100 = 127); the result stays inside
    /// what Windows sends (intensity 2..254, about 2..200 %).
    async fn set_strength(
        &self,
        percent: f64,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] em: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        polkit::check(conn, &hdr, "pen", self.0.no_polkit).await?;
        let intensity =
            pen::strength_to_intensity(percent).filter(|_| percent >= 0.0).ok_or_else(|| invalid("strength must be a percentage"))?;
        persist(self.0.update(|c| c.pen.intensity = intensity))?;
        invalidate(&em, Self::IFACE, Self::PROPS).await;
        Ok(())
    }

    async fn set_eraser_click(
        &self,
        on: bool,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] em: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        polkit::check(conn, &hdr, "pen", self.0.no_polkit).await?;
        persist(self.0.update(|c| c.pen.eraser_click = on))?;
        invalidate(&em, Self::IFACE, Self::PROPS).await;
        Ok(())
    }

    /// The continuous waveform of the inking haptics: ink, pencil, marker, chisel-marker,
    /// brush, sparkle.
    async fn set_ink_waveform(
        &self,
        name: String,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] em: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        polkit::check(conn, &hdr, "pen", self.0.no_polkit).await?;
        let w = pen::Waveform::from_name(&name).ok_or_else(|| invalid(format!("unknown waveform {name}")))?;
        persist(self.0.update(|c| c.pen.ink_waveform = w.name().into()))?;
        invalidate(&em, Self::IFACE, Self::PROPS).await;
        Ok(())
    }

    /// Pen speed in mm/s below which the speed value sent is 0, 0..100; 0 = none. Windows
    /// has about 15.
    async fn set_speed_dead_zone(
        &self,
        mm_s: f64,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] em: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        polkit::check(conn, &hdr, "pen", self.0.no_polkit).await?;
        if !config::Pen::dead_zone_valid(mm_s) {
            return Err(invalid("dead zone must be 0..100 mm/s"));
        }
        persist(self.0.update(|c| c.pen.speed_dead_zone_mm_s = mm_s))?;
        invalidate(&em, Self::IFACE, Self::PROPS).await;
        Ok(())
    }

    /// Pen speed in mm/s that maps to the largest speed value, 50..2000; 0 = the default
    /// (300, Windows' scale). Not a strength setting.
    async fn set_speed_full_scale(
        &self,
        mm_s: f64,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] em: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        polkit::check(conn, &hdr, "pen", self.0.no_polkit).await?;
        let v = if mm_s == 0.0 {
            None
        } else if config::Pen::full_scale_valid(mm_s) {
            Some(mm_s)
        } else {
            return Err(invalid("full scale must be 50..2000 mm/s, or 0 for the default"));
        };
        persist(self.0.update(|c| c.pen.full_scale_mm_s = v))?;
        invalidate(&em, Self::IFACE, Self::PROPS).await;
        Ok(())
    }

    /// One click at the set intensity, to feel the setting.
    async fn test_pulse(&self, #[zbus(header)] hdr: Header<'_>, #[zbus(connection)] conn: &zbus::Connection) -> fdo::Result<()> {
        polkit::check(conn, &hdr, "pen", self.0.no_polkit).await?;
        let p = pen::find(&*self.0.b.hid).ok_or_else(|| fdo::Error::NotSupported("pen not connected".into()))?;
        p.trigger(&*self.0.b.hid, pen::ORDINAL_CLICK, self.0.cfg().pen.intensity).map_err(failed)
    }

    /// Couple pens with this host when they are put into the cover slot.
    async fn set_auto_couple(
        &self,
        on: bool,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] em: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        polkit::check(conn, &hdr, "pen-couple", self.0.no_polkit).await?;
        persist(self.0.update(|c| c.pen.auto_couple = on))?;
        self.0.couple.poke();
        invalidate(&em, Self::IFACE, Self::PROPS).await;
        Ok(())
    }

    fn start_couple(&self, force: bool) -> fdo::Result<()> {
        if !self.0.hardware {
            return Err(fdo::Error::NotSupported("coupling needs the system bus and the real device".into()));
        }
        // refused while a coupling runs: nothing is queued behind it
        if !self.0.couple.request(force) {
            return Err(failed("a coupling is in progress"));
        }
        Ok(())
    }

    /// Look at the pen that sits in the cover slot now: couple it when it has no bond with
    /// this host; leave it when it is connected; otherwise probe its bond and replace it only
    /// when the probe shows it dead. Returns at once; CoupleState and CoupleResult tell how
    /// it went.
    async fn couple(
        &self,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] em: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        polkit::check(conn, &hdr, "pen-couple", self.0.no_polkit).await?;
        self.start_couple(false)?;
        invalidate(&em, Self::IFACE, Self::PROPS).await;
        Ok(())
    }

    /// As Couple, but the bond of a pen that is not connected is replaced without probing
    /// it. If the pen then takes no connection it is left without a bond.
    async fn couple_force(
        &self,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] em: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        polkit::check(conn, &hdr, "pen-couple", self.0.no_polkit).await?;
        self.start_couple(true)?;
        invalidate(&em, Self::IFACE, Self::PROPS).await;
        Ok(())
    }
}

// ------------------------------------------------------------------ Diagnostics

pub struct Diagnostics(pub Arc<Shared>);

impl Snapshot for Diagnostics {
    const IFACE: &'static str = "io.github.joonhoekim.OpenDeviceHelper1.Diagnostics";
    const PROPS: &'static [&'static str] = &["LastReport"];
    fn snapshot(&self) -> String {
        self.0.last_report.lock().unwrap().clone()
    }
}

#[interface(name = "io.github.joonhoekim.OpenDeviceHelper1.Diagnostics")]
impl Diagnostics {
    /// path of the last report, "" none
    #[zbus(property)]
    fn last_report(&self) -> String {
        self.0.last_report.lock().unwrap().clone()
    }

    /// Run the post-install check (read only, masked) and pack it; returns the tarball path
    /// (owned by the caller).
    async fn run(
        &self,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] em: SignalEmitter<'_>,
    ) -> fdo::Result<String> {
        polkit::check(conn, &hdr, "diagnostics", self.0.no_polkit).await?;
        let uid = polkit::caller_uid(conn, &hdr).await;
        let (data, state) = (self.0.data_dir.clone(), self.0.state_dir.clone());
        let p = blocking::unblock(move || jobs::diagnostics(&data, &state, uid)).await.map_err(failed)?;
        let p = p.display().to_string();
        *self.0.last_report.lock().unwrap() = p.clone();
        invalidate(&em, Self::IFACE, Self::PROPS).await;
        Ok(p)
    }
}
