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
    /// pen haptics as a whole: Windows-style inking haptics (start on tip down, stop on tip
    /// up) and, with `speed_feedback`, the pen speed to the digitizer
    pub ink_feedback: bool,
    /// 1..255
    pub intensity: u8,
    /// feed the pen speed back to the digitizer (the continuous waveforms need it); only
    /// while `ink_feedback` is on
    pub speed_feedback: bool,
    /// one click when the eraser end touches down (it plays no continuous waveform); only
    /// while `ink_feedback` is on
    pub eraser_click: bool,
    /// pen speed in mm/s that maps to the largest speed value; absent = 353 (Windows' scale)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_scale_mm_s: Option<f64>,
    /// the key earlier versions kept the scale in; `sp9-ctl pen strength` wrote it too, so
    /// its value is not carried over
    #[serde(rename = "speed_full_scale", skip_serializing)]
    legacy_full_scale: Option<f64>,
    /// couple a pen with this host when it is put into the cover slot
    pub auto_couple: bool,
    /// restart bluetooth.service when BlueZ has not attached the pen's HID 10 s after a
    /// coupling
    pub couple_restart_bluetooth: bool,
}

impl Default for Pen {
    fn default() -> Self {
        Pen {
            ink_feedback: false,
            intensity: crate::pen::INK_INTENSITY,
            speed_feedback: true,
            eraser_click: true,
            full_scale_mm_s: None,
            legacy_full_scale: None,
            auto_couple: false,
            couple_restart_bluetooth: false,
        }
    }
}

impl Pen {
    pub fn full_scale_valid(v: f64) -> bool {
        let (lo, hi) = crate::pen_feedback::FULL_SCALE_RANGE;
        v.is_finite() && (lo..=hi).contains(&v)
    }

    /// The speed that maps to the largest speed value.
    pub fn full_scale(&self) -> f64 {
        self.full_scale_mm_s.unwrap_or(crate::pen_feedback::DEFAULT_FULL_SCALE)
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
        if let Some(v) = self.pen.full_scale_mm_s.filter(|v| !Pen::full_scale_valid(*v)) {
            w.push(format!("pen.full_scale_mm_s: {v} outside 50..2000 mm/s, ignored"));
            self.pen.full_scale_mm_s = None;
        }
        if let Some(v) = self.pen.legacy_full_scale.take() {
            w.push(format!(
                "pen.speed_full_scale = {v} is no longer used (strength is the intensity now): the speed scale is {} mm/s",
                self.pen.full_scale()
            ));
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
        assert!(c.pen.speed_feedback && !c.pen.auto_couple && !c.pen.couple_restart_bluetooth);
        assert!(c.pen.eraser_click && c.pen.full_scale_mm_s.is_none());
        assert_eq!(c.pen.full_scale(), 353.0);
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
        t.file("/etc/sp9/helper.toml", "[pen]\nfull_scale_mm_s = 5.0\nauto_couple = true\n");
        let (c, w) = Config::load(&t.root.p("/etc/sp9/helper.toml"));
        assert_eq!(w.len(), 1, "{w:?}");
        assert_eq!(c.pen.full_scale(), 353.0);
        assert!(c.pen.auto_couple);
        t.file("/etc/sp9/helper.toml", "[display]\npsr_dc = \"psr2\"\n");
        let (c, w) = Config::load(&t.root.p("/etc/sp9/helper.toml"));
        assert_eq!(w.len(), 1);
        assert_eq!(c, Config::default());
    }

    /// A file written before strength became the intensity: the old scale (which `pen strength`
    /// wrote) is dropped with a note, the intensity stays, and the next save has neither key.
    #[test]
    fn old_strength_scale_is_not_carried_over() {
        let t = FakeTree::new("cfg4");
        let p = t.root.p("/etc/sp9/helper.toml");
        t.file("/etc/sp9/helper.toml", "[pen]\nink_feedback = true\nintensity = 127\nspeed_full_scale = 145.0\n");
        let (c, w) = Config::load(&p);
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(w[0].contains("speed_full_scale = 145") && w[0].contains("353"), "{w:?}");
        assert_eq!((c.pen.full_scale(), c.pen.intensity, c.pen.ink_feedback), (353.0, 127, true));
        c.save(&p).unwrap();
        assert!(!t.read("/etc/sp9/helper.toml").contains("full_scale"));
        assert_eq!(Config::load(&p), (c.clone(), vec![]));
        // set through `pen full-scale`: kept
        let mut c2 = c;
        c2.pen.full_scale_mm_s = Some(200.0);
        c2.save(&p).unwrap();
        assert!(t.read("/etc/sp9/helper.toml").contains("full_scale_mm_s = 200.0"));
        assert_eq!(Config::load(&p), (c2, vec![]));
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
