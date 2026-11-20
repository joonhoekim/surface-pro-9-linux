// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
//! Work that runs other programs: the post-install check (diagnostics), the panel ICC profile
//! (tools/sdc-blob.py) and restarting iptsd after its drop-in changed. Called off the D-Bus
//! executor (blocking::unblock).

use sp9_helper_core::display;
use std::path::{Path, PathBuf};
use std::process::Command;

fn run(cmd: &mut Command) -> Result<String, String> {
    let out = cmd.output().map_err(|e| format!("{:?}: {e}", cmd.get_program()))?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    if out.status.success() {
        Ok(text)
    } else {
        Err(format!("{:?} failed ({}): {}", cmd.get_program(), out.status, text.lines().last().unwrap_or("")))
    }
}

/// Restart every running iptsd instance (iptsd@<hidraw>.service) so it reads /etc/iptsd.d again.
///
/// Restarts are kept apart: a slider in the settings app sends a change per step, and systemd gives
/// up on a unit that is started too often in a row (start-limit-hit left the machine without touch
/// and pen). A change that comes within `IPTSD_RESTART_GAP` of the last restart is applied by one
/// deferred restart.
#[cfg(not(test))]
pub fn restart_iptsd_blocking() -> Result<(), String> {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    const IPTSD_RESTART_GAP: Duration = Duration::from_secs(4);
    static LAST: Mutex<Option<Instant>> = Mutex::new(None);
    static PENDING: AtomicBool = AtomicBool::new(false);

    fn restart_now() -> Result<(), String> {
        *LAST.lock().unwrap() = Some(Instant::now());
        // a unit that hit the start limit earlier stays failed otherwise
        let _ = run(Command::new("systemctl").args(["reset-failed", "iptsd@*.service"]));
        run(Command::new("systemctl").args(["restart", "iptsd@*.service"])).map(|_| ())
    }

    let since = LAST.lock().unwrap().map(|t| t.elapsed());
    match since {
        Some(d) if d < IPTSD_RESTART_GAP => {
            if !PENDING.swap(true, Ordering::SeqCst) {
                let wait = IPTSD_RESTART_GAP - d;
                std::thread::spawn(move || {
                    std::thread::sleep(wait);
                    PENDING.store(false, Ordering::SeqCst);
                    if let Err(e) = restart_now() {
                        eprintln!("sp9-helperd: iptsd restart: {e}");
                    }
                });
            }
            Ok(())
        }
        _ => restart_now(),
    }
}

/// The tests drive SetIptsd and Reload as an ordinary user against fake files, but systemctl is
/// the machine's own: there it raises a polkit dialog on the developer's desktop and, once that is
/// answered, restarts the real touch daemon (touch and pen input stop for a moment).
#[cfg(test)]
pub fn restart_iptsd_blocking() -> Result<(), String> {
    Ok(())
}

/// Hands the touch and pen settings to sp9-digitizerd if it is on the system bus: it takes
/// them while it runs, without a restart (a restart replaces its input devices, and
/// applications keep stale state for the old ones). `Ok(false)`: it is not there, and the
/// settings reach whoever processes touch through the drop-in.
#[cfg(not(test))]
pub fn push_digitizer_blocking(
    iptsd: &sp9_helper_core::input::Iptsd,
    zero_force: &sp9_helper_core::input::ZeroForce,
    prediction_ms: f64,
) -> Result<bool, String> {
    use sp9_digitizer::control::{self, Param, Value};
    use std::collections::HashMap;
    use zbus::zvariant::Value as V;

    let dict = |params: &[(Param, Value)]| -> HashMap<&'static str, V<'static>> {
        params
            .iter()
            .map(|&(p, v)| {
                let value = match v {
                    Value::Bool(b) => V::from(b),
                    Value::Number(n) => V::from(n),
                };
                (p.name(), value)
            })
            .collect()
    };
    let sp9_helper_core::input::DigitizerParams { pen, touch } = sp9_helper_core::input::digitizer_params(iptsd, zero_force, prediction_ms);

    zbus::block_on(async {
        let conn = zbus::Connection::system().await.map_err(|e| format!("system bus: {e}"))?;
        let owned = conn
            .call_method(
                Some("org.freedesktop.DBus"),
                "/org/freedesktop/DBus",
                Some("org.freedesktop.DBus"),
                "NameHasOwner",
                &(control::BUS_NAME,),
            )
            .await
            .and_then(|r| r.body().deserialize::<bool>())
            .map_err(|e| format!("system bus: {e}"))?;
        if !owned {
            return Ok(false);
        }
        for (interface, params) in [(control::PEN_INTERFACE, dict(&pen)), (control::TOUCH_INTERFACE, dict(&touch))] {
            conn.call_method(Some(control::BUS_NAME), control::OBJECT_PATH, Some(interface), "SetParams", &(params,))
                .await
                .map_err(|e| format!("sp9-digitizerd: {e}"))?;
        }
        Ok(true)
    })
}

/// The tests have no system bus to talk to.
#[cfg(test)]
pub fn push_digitizer_blocking(
    _iptsd: &sp9_helper_core::input::Iptsd,
    _zero_force: &sp9_helper_core::input::ZeroForce,
    _prediction_ms: f64,
) -> Result<bool, String> {
    Ok(false)
}

fn stamp() -> String {
    let s = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    s.to_string()
}

/// Reports kept in STATE/diagnostics; older ones are removed when a new one is packed.
const DIAGNOSTICS_KEEP: usize = 5;

/// tools/linux/post-install-check.sh into STATE/diagnostics/check-<time>, packed as
/// check-<time>.tar.gz, readable only by root and by `uid` (its owner when given). The script
/// masks MACs, UUIDs and serials; the report still describes the machine, so it is not left
/// readable by every user.
pub fn diagnostics(data_dir: &Path, state_dir: &Path, uid: Option<u32>) -> Result<PathBuf, String> {
    let script = data_dir.join("post-install-check.sh");
    if !script.exists() {
        return Err(format!("{} not installed", script.display()));
    }
    let base = state_dir.join("diagnostics");
    std::fs::create_dir_all(&base).map_err(|e| format!("{}: {e}", base.display()))?;
    let name = format!("check-{}", stamp());
    let dir = base.join(&name);
    private_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    // SUDO_USER would make the script write as, and run session tools as, that user.
    let ran = run(Command::new("bash")
        .arg(&script)
        .arg(&dir)
        .env("LC_ALL", "C")
        .env_remove("SUDO_USER")
        .env_remove("SUDO_UID")
        .env_remove("SUDO_GID"));
    let tar = base.join(format!("{name}.tar.gz"));
    // Created 0600 before tar writes into it, so it is never readable by others.
    let packed = ran
        .and_then(|_| private_file(&tar).map_err(|e| format!("{}: {e}", tar.display())))
        .and_then(|_| run(Command::new("tar").arg("-czf").arg(&tar).arg("-C").arg(&base).arg(&name)));
    let _ = std::fs::remove_dir_all(&dir);
    packed?;
    chown(&tar, uid);
    prune(&base, DIAGNOSTICS_KEEP);
    Ok(tar)
}

#[cfg(unix)]
fn private_dir(p: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new().mode(0o700).create(p)
}

#[cfg(not(unix))]
fn private_dir(p: &Path) -> std::io::Result<()> {
    std::fs::create_dir(p)
}

#[cfg(unix)]
fn private_file(p: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(p).map(|_| ())
}

#[cfg(not(unix))]
fn private_file(p: &Path) -> std::io::Result<()> {
    std::fs::File::create(p).map(|_| ())
}

/// Remove all but the `keep` newest check-<time>.tar.gz in `dir` (names sort by time).
fn prune(dir: &Path, keep: usize) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut names: Vec<String> = rd
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("check-") && n.ends_with(".tar.gz"))
        .collect();
    names.sort_by_key(|n| n["check-".len()..n.len() - ".tar.gz".len()].parse::<u64>().unwrap_or(0));
    let n = names.len().saturating_sub(keep);
    for old in &names[..n] {
        let _ = std::fs::remove_file(dir.join(old));
    }
}

/// The panel's native ICC profile from the UEFI calibration, at STATE/display/panel-native.icc.
pub fn icc(python: &str, data_dir: &Path, state_dir: &Path, cal: &(PathBuf, Option<PathBuf>)) -> Result<PathBuf, String> {
    let script = data_dir.join("sdc-blob.py");
    if !script.exists() {
        return Err(format!("{} not installed", script.display()));
    }
    let dir = state_dir.join("display");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let out = dir.join("panel-native.icc");
    let argv = display::icc_command(python, &script.display().to_string(), cal, &out.display().to_string());
    run(Command::new(&argv[0]).args(&argv[1..]))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o644));
    }
    Ok(out)
}

#[cfg(unix)]
fn chown(p: &Path, uid: Option<u32>) {
    if let Some(u) = uid {
        let _ = std::os::unix::fs::chown(p, Some(u), None);
    }
}

#[cfg(not(unix))]
fn chown(_p: &Path, _uid: Option<u32>) {}

#[cfg(test)]
mod diagnostics_tests {
    use super::prune;

    #[test]
    fn prune_keeps_the_newest_reports() {
        let dir = std::env::temp_dir().join(format!("sp9-prune-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for t in [5u64, 100, 20, 3, 40] {
            std::fs::write(dir.join(format!("check-{t}.tar.gz")), b"").unwrap();
        }
        std::fs::write(dir.join("other.txt"), b"").unwrap();
        prune(&dir, 2);
        let mut left: Vec<String> =
            std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        left.sort();
        assert_eq!(left, ["check-100.tar.gz", "check-40.tar.gz", "other.txt"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
