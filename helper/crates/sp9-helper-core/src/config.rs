// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
//! Persistent settings: /etc/sp9/helper.toml (`SP9_CONFIG`). Every field has a default, so a
//! missing or partial file works; unknown or invalid values are reported and replaced by the
//! default. Experimental features are off by default.

use crate::display::PsrMode;
use crate::input::Iptsd;
use crate::perf::{self, Limits};
use crate::sys;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const DEFAULT_PATH: &str = "/etc/sp9/helper.toml";

pub fn config_path() -> PathBuf {
    std::env::var_os("SP9_CONFIG").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(DEFAULT_PATH))
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Performance {
    /// apply the profile's RAPL limits (and EPP) whenever platform_profile or the power source changes
    pub follow_profile: bool,
    /// write the EPP of the profile too (and again within `engine::EPP_HOLD_S` when replaced)
    pub epp: bool,
    /// overrides of the table, key "<profile>.ac" / "<profile>.dc"
    pub limits: BTreeMap<String, Limits>,
    /// experimental: lower PL1/PL2 by the VTS1 skin temperature
    pub skin_guard: bool,
    /// skin sensor: a surface_temp hwmon channel ("temp9"), or a label where labels are unique
    pub skin_sensor: String,
}

impl Default for Performance {
    fn default() -> Self {
        Performance {
            follow_profile: true,
            epp: true,
            limits: BTreeMap::new(),
            skin_guard: false,
            skin_sensor: crate::thermal::SKIN_SENSOR.into(),
        }
    }
}

impl Performance {
    /// The limits in force for a profile: the override, else the table.
    pub fn limits_for(&self, profile: &str, ac: bool) -> Option<Limits> {
        self.limits.get(&perf::limits_key(profile, ac)).copied().or_else(|| perf::default_limits(profile, ac))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Fan {
    /// experimental: allow the FAN 0x0b rpm request (sp9-ctl fan allow on)
    pub allow_rpm_request: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Display {
    pub psr_ac: PsrMode,
    pub psr_dc: PsrMode,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Input {
    pub touch_off_when_display_off: bool,
    pub iptsd: Iptsd,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Pen {
    /// Windows-style inking haptics (start on tip down, stop on tip up)
    pub ink_feedback: bool,
    /// 1..255
    pub intensity: u8,
}

impl Default for Pen {
    fn default() -> Self {
        Pen { ink_feedback: false, intensity: crate::pen::INK_INTENSITY }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub performance: Performance,
    pub fan: Fan,
    pub display: Display,
    pub input: Input,
    pub pen: Pen,
}

impl Config {
    /// Load and validate; returns the config and the problems found (already corrected).
    pub fn load(path: &Path) -> (Config, Vec<String>) {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(_) => return (Config::default(), vec![]),
        };
        match toml::from_str::<Config>(&text) {
            Ok(mut c) => {
                let w = c.sanitize();
                (c, w)
            }
            Err(e) => (Config::default(), vec![format!("{}: {e}; using defaults", path.display())]),
        }
    }

    /// Drop values outside their bounds.
    pub fn sanitize(&mut self) -> Vec<String> {
        let mut w = Vec::new();
        self.performance.limits.retain(|k, l| {
            let known = k.rsplit_once('.').is_some_and(|(p, s)| perf::PROFILES.contains(&p) && (s == "ac" || s == "dc"));
            match (known, l.validate()) {
                (true, Ok(())) => true,
                (false, _) => {
                    w.push(format!("performance.limits.{k}: unknown profile key, ignored"));
                    false
                }
                (_, Err(e)) => {
                    w.push(format!("performance.limits.{k}: {e}, ignored"));
                    false
                }
            }
        });
        if let Err(e) = self.input.iptsd.validate() {
            w.push(format!("input.iptsd: {e}, ignored"));
            self.input.iptsd = Iptsd::default();
        }
        if self.pen.intensity == 0 {
            self.pen.intensity = 1;
        }
        w
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let text = toml::to_string_pretty(self).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        sys::write_atomic(path, format!("# sp9-helperd settings (written by the daemon; see docs/helper.md)\n{text}").as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sys::FakeTree;

    #[test]
    fn defaults_and_roundtrip() {
        let t = FakeTree::new("cfg");
        let p = t.root.p("/etc/sp9/helper.toml");
        let (c, w) = Config::load(&p);
        assert!(w.is_empty());
        assert!(c.performance.follow_profile && !c.performance.skin_guard && !c.fan.allow_rpm_request);
        assert!(!c.pen.ink_feedback && !c.input.touch_off_when_display_off);
        let mut c2 = c.clone();
        c2.performance.limits.insert("performance.ac".into(), Limits::new(30.0, 50.0, 28.0, 64));
        c2.display.psr_ac = PsrMode::Off;
        c2.input.iptsd.activation_threshold = Some(22.0);
        c2.save(&p).unwrap();
        let (c3, w) = Config::load(&p);
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(c2, c3);
        assert_eq!(c3.performance.limits_for("performance", true).unwrap().pl1_w, 30.0);
        assert_eq!(c3.performance.limits_for("performance", false).unwrap().pl1_w, 30.0);
        assert_eq!(c3.performance.limits_for("balanced", false).unwrap().pl1_w, 13.0);
    }

    #[test]
    fn shipped_example_is_the_default() {
        let c: Config = toml::from_str(include_str!("../../../data/helper.toml.example")).unwrap();
        assert_eq!(c, Config::default());
    }

    #[test]
    fn invalid_values_dropped() {
        let t = FakeTree::new("cfg2");
        t.file(
            "/etc/sp9/helper.toml",
            "[performance.limits.\"performance.ac\"]\npl1_w = 50.0\npl2_w = 60.0\ntau_s = 28.0\nepp = 0\n\
             [performance.limits.\"turbo.ac\"]\npl1_w = 20.0\npl2_w = 30.0\ntau_s = 28.0\nepp = 0\n\
             [display]\npsr_dc = \"psr1\"\n",
        );
        let (c, w) = Config::load(&t.root.p("/etc/sp9/helper.toml"));
        assert_eq!(w.len(), 2, "{w:?}");
        assert!(c.performance.limits.is_empty());
        assert_eq!(c.display.psr_dc, PsrMode::Psr1);
        t.file("/etc/sp9/helper.toml", "[display]\npsr_dc = \"psr2\"\n");
        let (c, w) = Config::load(&t.root.p("/etc/sp9/helper.toml"));
        assert_eq!(w.len(), 1);
        assert_eq!(c, Config::default());
    }

    /// A table this version has no field for (a file written by another version) is neither an
    /// error nor a warning: an error would replace every setting in the file by its default.
    /// The next save writes only the known tables.
    #[test]
    fn unknown_table_is_ignored_and_not_written_back() {
        let t = FakeTree::new("cfg3");
        let p = t.root.p("/etc/sp9/helper.toml");
        t.file(
            "/etc/sp9/helper.toml",
            "[display]\npsr_dc = \"psr1\"\n\
             [input]\ntouch_off_when_display_off = true\n\
             [input.edge_scroll]\nenabled = false\ndevice = \"045E:09AF\"\n\
             [input.edge_scroll.touchpads.\"045E:09AF\"]\nmax_speed = 9000.0\nshape = \"ease\"\n\
             [pen]\nintensity = 90\n",
        );
        let (c, w) = Config::load(&p);
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(c.display.psr_dc, PsrMode::Psr1);
        assert!(c.input.touch_off_when_display_off);
        assert_eq!(c.pen.intensity, 90);
        c.save(&p).unwrap();
        assert!(!t.read("/etc/sp9/helper.toml").contains("edge_scroll"));
        assert_eq!(Config::load(&p), (c, vec![]));
    }
}
