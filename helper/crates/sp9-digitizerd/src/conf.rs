// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
//! Loading the configuration.
//!
//! The built-in configuration is the device's. On top of it: one TOML file with the structure
//! of [`Config`], then any number of iptsd configuration files, of which only the keys that
//! have a meaning here are taken (the helper writes its settings as such a file).

use sp9_digitizer::Config;
use std::path::{Path, PathBuf};

/// The daemon's own settings.
pub const CONFIG: &str = "/etc/sp9/digitizer.toml";
/// The drop-in sp9-helperd writes for iptsd (touch thresholds, zero force contact).
pub const HELPER_DROPIN: &str = "/etc/iptsd.d/zz-sp9-helper.conf";

/// Which files to read. `None` / empty: the default files, if they exist.
#[derive(Clone, Debug, Default)]
pub struct Sources {
    pub toml: Option<PathBuf>,
    pub iptsd: Vec<PathBuf>,
}

/// The configuration and what was read for it (for the log).
pub fn load(sources: &Sources) -> Result<(Config, Vec<String>), String> {
    let mut notes = Vec::new();
    let mut config = Config::default();

    let (toml_path, required) = match &sources.toml {
        Some(p) => (p.clone(), true),
        None => (PathBuf::from(CONFIG), false),
    };
    if let Some(text) = read(&toml_path, required)? {
        config = toml::from_str(&text).map_err(|e| format!("{}: {e}", toml_path.display()))?;
        notes.push(format!("read {}", toml_path.display()));
    }

    let (iptsd_paths, required) =
        if sources.iptsd.is_empty() { (vec![PathBuf::from(HELPER_DROPIN)], false) } else { (sources.iptsd.clone(), true) };
    for path in &iptsd_paths {
        let Some(text) = read(path, required)? else { continue };
        let report = config.apply_iptsd_conf(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        if report.other_device {
            notes.push(format!("{} is for another device", path.display()));
            continue;
        }
        notes.push(format!("read {} ({} keys)", path.display(), report.applied.len()));
        if !report.ignored.is_empty() {
            notes.push(format!("{}: keys without a meaning here: {}", path.display(), report.ignored.join(", ")));
        }
    }

    config.validate()?;
    Ok((config, notes))
}

fn read(path: &Path, required: bool) -> Result<Option<String>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && !required => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toml_then_iptsd_files() {
        let dir = std::env::temp_dir().join(format!("sp9-digitizerd-conf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let toml = dir.join("digitizer.toml");
        let ini = dir.join("helper.conf");
        let other = dir.join("other.conf");
        std::fs::write(&toml, "[pen.pressure]\nfloor = 0.05\n[touch]\ndisable_on_palm = true\n").unwrap();
        std::fs::write(&ini, "[Device]\nVendor = 0x045E\nProduct = 0x0C52\n[DFT]\nContactMinPressure = 0.03\nZeroForceContact = true\n")
            .unwrap();
        std::fs::write(&other, "[Device]\nVendor = 0x045E\nProduct = 0x099F\n[DFT]\nContactMinPressure = 0.5\n").unwrap();

        let (config, notes) = load(&Sources { toml: Some(toml.clone()), iptsd: vec![ini, other] }).unwrap();
        assert!(config.touch.disable_on_palm);
        assert_eq!(config.pen.pressure.floor, 0.03);
        assert!(config.pen.contact.zero_force.enabled);
        assert_eq!(notes.len(), 3);

        assert!(load(&Sources { toml: Some(dir.join("missing.toml")), iptsd: vec![] }).is_err());
        std::fs::write(&toml, "[pen]\naxis_resolution = 0\n").unwrap();
        assert!(load(&Sources { toml: Some(toml), iptsd: vec![dir.join("helper.conf")] }).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
