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
    let sp9_helper_core::input::DigitizerParams { pen, touch } = sp9_helper_core::input::digitizer_params(iptsd, zero_force);

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
) -> Result<bool, String> {
    Ok(false)
}

fn stamp() -> String {
    let s = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    s.to_string()
}

/// tools/linux/post-install-check.sh into STATE/diagnostics/check-<time>, packed as
/// check-<time>.tar.gz (owned by `uid` when given). The script masks MACs, UUIDs and serials.
pub fn diagnostics(data_dir: &Path, state_dir: &Path, uid: Option<u32>) -> Result<PathBuf, String> {
    let script = data_dir.join("post-install-check.sh");
    if !script.exists() {
        return Err(format!("{} not installed", script.display()));
    }
    let base = state_dir.join("diagnostics");
    std::fs::create_dir_all(&base).map_err(|e| format!("{}: {e}", base.display()))?;
    let name = format!("check-{}", stamp());
    let dir = base.join(&name);
    run(Command::new("bash").arg(&script).arg(&dir).env("LC_ALL", "C"))?;
    let tar = base.join(format!("{name}.tar.gz"));
    run(Command::new("tar").arg("-czf").arg(&tar).arg("-C").arg(&base).arg(&name))?;
    let _ = std::fs::remove_dir_all(&dir);
    chown(&tar, uid);
    Ok(tar)
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
