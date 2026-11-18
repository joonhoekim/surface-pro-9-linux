// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
//! Finding the hidraw node of the digitizer. Its number is not stable: the kernel hands out
//! a new one whenever the device is set up again (it was seen to change within one boot), so
//! the node is found by the ID of the HID device behind it.

use sp9_digitizer::config::{PRODUCT, VENDOR};
use std::path::{Path, PathBuf};

/// The hidraw nodes (`/dev/hidrawN`) of the digitizer below a sysfs root (`/sys`), lowest
/// number first.
pub fn digitizer_nodes(sys: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(sys.join("class/hidraw")) else { return Vec::new() };
    let mut found: Vec<(u32, PathBuf)> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let number = name.strip_prefix("hidraw")?.parse::<u32>().ok()?;
            let uevent = std::fs::read_to_string(e.path().join("device/uevent")).ok()?;
            is_digitizer(&uevent).then(|| (number, PathBuf::from("/dev").join(name)))
        })
        .collect();
    found.sort();
    found.into_iter().map(|(_, path)| path).collect()
}

/// Whether the uevent of a HID device names the digitizer: `HID_ID=<bus>:<vendor>:<product>`
/// with eight hexadecimal digits each.
fn is_digitizer(uevent: &str) -> bool {
    uevent.lines().filter_map(|l| l.strip_prefix("HID_ID=")).any(|id| {
        let mut parts = id.split(':').skip(1).map(|p| u32::from_str_radix(p, 16).ok());
        parts.next() == Some(Some(u32::from(VENDOR))) && parts.next() == Some(Some(u32::from(PRODUCT)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn by_id_not_by_number() {
        let sys = std::env::temp_dir().join(format!("sp9-digitizerd-find-{}", std::process::id()));
        let node = |name: &str, id: &str| {
            let dir = sys.join("class/hidraw").join(name).join("device");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("uevent"), format!("DRIVER=hid-generic\nHID_ID={id}\nHID_NAME=x\n")).unwrap();
        };
        node("hidraw0", "0019:0000045E:0000005D");
        node("hidraw7", "0001:0000045E:00000C52");
        node("hidraw12", "0005:0000045E:00000C0F");
        std::fs::create_dir_all(sys.join("class/hidraw/hidraw3")).unwrap();
        assert_eq!(digitizer_nodes(&sys), vec![PathBuf::from("/dev/hidraw7")]);
        assert!(digitizer_nodes(&sys.join("nothing")).is_empty());
        std::fs::remove_dir_all(&sys).unwrap();
    }
}
