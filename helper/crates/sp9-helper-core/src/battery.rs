// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
//! Battery and adapter from the power supply class (surface_battery `BAT1`, surface_charger
//! `ADP1` with the adapter rating: `input_power_limit` from kernel/patches/0010, or
//! `dptf/adapter_rating_mw` of linux-surface's kernels from 7.3.5 on), and the UEFI "Battery Limit"
//! switch read from efivarfs (docs/research/control/battery-charging.md 2). Nothing here writes;
//! smart charging is in [`crate::ssam`].

use crate::perf::POWER_SUPPLY;
use crate::sys::Root;
use std::sync::{Arc, Mutex};

pub const BATTERY_LIMIT_VAR: &str = "/sys/firmware/efi/efivars/BatteryLimitEnable-41d9f172-a9e9-4f9f-81a9-133e2d5902d4";

#[derive(Clone, Debug, PartialEq, Default)]
pub struct BatteryInfo {
    pub name: String,
    pub status: String,
    pub capacity: i32,
    pub cycle_count: i32,
    pub energy_full_uwh: Option<u64>,
    pub energy_full_design_uwh: Option<u64>,
    pub energy_now_uwh: Option<u64>,
    pub power_uw: Option<u64>,
    pub voltage_uv: Option<u64>,
}

impl BatteryInfo {
    /// Full charge capacity in % of the design capacity (-1 unknown).
    pub fn health_pct(&self) -> f64 {
        match (self.energy_full_uwh, self.energy_full_design_uwh) {
            (Some(f), Some(d)) if d > 0 => (f as f64 * 1000.0 / d as f64).round() / 10.0,
            _ => -1.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct AdapterInfo {
    pub online: bool,
    /// adapter rating from SAM BAT 0x0f (patch 0010 or linux-surface's `dptf/adapter_rating_mw`), µW
    pub input_power_limit_uw: Option<u64>,
}

pub trait BatteryBackend: Send + Sync {
    fn battery(&self) -> Option<BatteryInfo>;
    fn adapter(&self) -> Option<AdapterInfo>;
    /// UEFI BatteryLimitEnable: Some(true) on, Some(false) off, None unreadable.
    fn uefi_battery_limit(&self) -> Option<bool>;
}

/// efivarfs content: 4 bytes of attributes, then the value (little endian).
pub fn parse_efivar_u64(b: &[u8]) -> Option<u64> {
    let v = b.get(4..)?;
    if v.is_empty() {
        return None;
    }
    Some(v.iter().take(8).rev().fold(0u64, |a, x| (a << 8) | *x as u64))
}

pub struct SysfsBattery {
    root: Root,
}

impl SysfsBattery {
    pub fn new(root: Root) -> Self {
        SysfsBattery { root }
    }

    fn supply_of(&self, ty: &str) -> Option<String> {
        let all = self.root.list(POWER_SUPPLY);
        all.into_iter().find(|s| self.root.read_opt(&format!("{POWER_SUPPLY}/{s}/type")).as_deref() == Some(ty))
    }
}

impl BatteryBackend for SysfsBattery {
    fn battery(&self) -> Option<BatteryInfo> {
        let n = self.supply_of("Battery")?;
        let d = format!("{POWER_SUPPLY}/{n}");
        let u = |a: &str| self.root.read_i64(&format!("{d}/{a}")).map(|v| v.unsigned_abs());
        Some(BatteryInfo {
            status: self.root.read_opt(&format!("{d}/status")).unwrap_or_else(|| "Unknown".into()),
            capacity: self.root.read_i64(&format!("{d}/capacity")).map(|v| v as i32).unwrap_or(-1),
            cycle_count: self.root.read_i64(&format!("{d}/cycle_count")).map(|v| v as i32).unwrap_or(-1),
            energy_full_uwh: u("energy_full"),
            energy_full_design_uwh: u("energy_full_design"),
            energy_now_uwh: u("energy_now"),
            power_uw: u("power_now"),
            voltage_uv: u("voltage_now"),
            name: n,
        })
    }

    fn adapter(&self) -> Option<AdapterInfo> {
        let n = self.supply_of("Mains")?;
        let d = format!("{POWER_SUPPLY}/{n}");
        Some(AdapterInfo {
            online: self.root.read_i64(&format!("{d}/online")) == Some(1),
            input_power_limit_uw: self
                .root
                .read_i64(&format!("{d}/input_power_limit"))
                .or_else(|| self.root.read_i64(&format!("{d}/dptf/adapter_rating_mw")).map(|mw| mw.saturating_mul(1000)))
                .map(|v| v.max(0) as u64),
        })
    }

    fn uefi_battery_limit(&self) -> Option<bool> {
        let b = std::fs::read(self.root.p(BATTERY_LIMIT_VAR)).ok()?;
        parse_efivar_u64(&b).map(|v| v != 0)
    }
}

#[derive(Clone, Debug, Default)]
pub struct FakeBatteryState {
    pub battery: Option<BatteryInfo>,
    pub adapter: Option<AdapterInfo>,
    pub uefi_limit: Option<bool>,
}

#[derive(Clone, Default)]
pub struct FakeBattery(pub Arc<Mutex<FakeBatteryState>>);

impl BatteryBackend for FakeBattery {
    fn battery(&self) -> Option<BatteryInfo> {
        self.0.lock().unwrap().battery.clone()
    }
    fn adapter(&self) -> Option<AdapterInfo> {
        self.0.lock().unwrap().adapter.clone()
    }
    fn uefi_battery_limit(&self) -> Option<bool> {
        self.0.lock().unwrap().uefi_limit
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sys::FakeTree;

    #[test]
    fn efivar() {
        assert_eq!(parse_efivar_u64(&[7, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]), Some(0));
        assert_eq!(parse_efivar_u64(&[7, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0]), Some(1));
        assert_eq!(parse_efivar_u64(&[7, 0, 0, 0]), None);
    }

    #[test]
    fn sysfs_supplies() {
        let t = FakeTree::new("bat");
        let b = format!("{POWER_SUPPLY}/BAT1");
        t.file(&format!("{b}/type"), "Battery\n")
            .file(&format!("{b}/status"), "Charging\n")
            .file(&format!("{b}/capacity"), "97\n")
            .file(&format!("{b}/cycle_count"), "40\n")
            .file(&format!("{b}/energy_full"), "43190000\n")
            .file(&format!("{b}/energy_full_design"), "47400000\n")
            .file(&format!("{POWER_SUPPLY}/ADP1/type"), "Mains\n")
            .file(&format!("{POWER_SUPPLY}/ADP1/online"), "1\n")
            .file(&format!("{POWER_SUPPLY}/ADP1/input_power_limit"), "65000000\n")
            .file(BATTERY_LIMIT_VAR, [7u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        let s = SysfsBattery::new(t.root.clone());
        let i = s.battery().unwrap();
        assert_eq!((i.name.as_str(), i.capacity, i.cycle_count), ("BAT1", 97, 40));
        assert_eq!(i.health_pct(), 91.1);
        assert_eq!(s.adapter().unwrap(), AdapterInfo { online: true, input_power_limit_uw: Some(65_000_000) });
        assert_eq!(s.uefi_battery_limit(), Some(false));
    }

    #[test]
    fn adapter_rating_of_linux_surface() {
        let t = FakeTree::new("bat-dptf");
        t.file(&format!("{POWER_SUPPLY}/ADP1/type"), "Mains\n")
            .file(&format!("{POWER_SUPPLY}/ADP1/online"), "1\n")
            .file(&format!("{POWER_SUPPLY}/ADP1/dptf/adapter_rating_mw"), "45000\n");
        let s = SysfsBattery::new(t.root.clone());
        assert_eq!(s.adapter().unwrap(), AdapterInfo { online: true, input_power_limit_uw: Some(45_000_000) });
    }
}
