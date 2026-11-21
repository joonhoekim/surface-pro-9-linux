// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
//! sp9-helperd: system D-Bus service (`io.github.joonhoekim.OpenDeviceHelper1`) for the
//! device-specific controls of the Surface Pro 9 (Intel). See docs/helper.md.
//!
//!   sp9-helperd [--session] [--no-polkit] [--fake]
//!     --session    own the name on the session bus (tests)
//!     --no-polkit  skip authorization (tests on a private bus only)
//!     --fake       in-memory backends instead of the machine (tests, front-end work)
//!
//! Environment: SP9_SYSFS_ROOT (fake device tree), SP9_CONFIG (settings file),
//! SP9_STATE_DIR (default /var/lib/sp9), SP9_DATA_DIR (scripts; default
//! <dir of this binary>/sp9-helper), SP9_PYTHON (default python3).

#[cfg(target_os = "linux")]
mod clock;
mod couple;
mod ifaces;
mod jobs;
mod kbdwatch;
mod penwatch;
mod polkit;

use ifaces::*;
use sp9_helper_core::config::{config_path, Config};
use sp9_helper_core::engine::{self, Runtime};
use sp9_helper_core::ssam::{self, SmartCharging};
use sp9_helper_core::sys::Root;
use sp9_helper_core::Backends;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use zbus::object_server::SignalEmitter;

pub const BUS: &str = "io.github.joonhoekim.OpenDeviceHelper1";
pub const DEVICE: &str = "surface-pro-9";
pub const VERSION: &str = env!("SP9_VERSION");

pub struct Shared {
    cfg: Mutex<Config>,
    cfg_path: PathBuf,
    pub b: Backends,
    pub root: Root,
    pub rt: Mutex<Runtime>,
    pub no_polkit: bool,
    pub state_dir: PathBuf,
    pub data_dir: PathBuf,
    pub python: String,
    pub features: Vec<String>,
    sc_cache: Mutex<Option<(Instant, Result<SmartCharging, String>)>>,
    pub last_report: Mutex<String>,
    pub couple: couple::Couple,
    /// the machine's Bluetooth and cover are in reach (system bus, real backends): the
    /// coupling thread runs
    pub hardware: bool,
    /// the settings file is there but could not be read (the defaults are in force)
    cfg_unreadable: AtomicBool,
    /// the daemon is stopping: the device threads leave their devices in a quiet state
    pub stop: AtomicBool,
    /// the pen thread has done so
    pub pen_done: AtomicBool,
}

impl Shared {
    pub fn cfg(&self) -> Config {
        self.cfg.lock().unwrap().clone()
    }

    /// Change the configuration and persist it.
    pub fn update(&self, change: impl FnOnce(&mut Config)) -> std::io::Result<()> {
        let mut g = self.cfg.lock().unwrap();
        change(&mut g);
        // the file could not be read and the defaults stood in for it: keep it before they replace it
        if self.cfg_unreadable.swap(false, Ordering::SeqCst) {
            match Config::keep_unreadable(&self.cfg_path) {
                Ok(bad) => eprintln!("sp9-helperd: config: the unreadable settings file is kept as {}", bad.display()),
                Err(e) => eprintln!("sp9-helperd: config: cannot keep the unreadable settings file: {e}"),
            }
        }
        g.save(&self.cfg_path)
    }

    pub fn reload(&self) -> Vec<String> {
        let (c, w, readable) = Config::load_checked(&self.cfg_path);
        self.cfg_unreadable.store(!readable, Ordering::SeqCst);
        *self.cfg.lock().unwrap() = c;
        self.rt.lock().unwrap().reset();
        w
    }

    /// Smart charging state, read from SAM at most every 5 s.
    pub fn smart_charging(&self) -> Result<SmartCharging, String> {
        let mut c = self.sc_cache.lock().unwrap();
        if let Some((t, v)) = c.as_ref() {
            if t.elapsed() < Duration::from_secs(5) {
                return v.clone();
            }
        }
        let v = if self.b.ssam.available() {
            ssam::smart_charging(&*self.b.ssam).map_err(|e| e.to_string())
        } else {
            Err(format!("{} missing (surface_aggregator_cdev not loaded)", ssam::CDEV))
        };
        *c = Some((Instant::now(), v.clone()));
        v
    }

    pub fn forget_smart_charging(&self) {
        *self.sc_cache.lock().unwrap() = None;
    }

    pub fn has(&self, f: &str) -> bool {
        self.features.iter().any(|x| x == f)
    }

    /// Withdraw a fan rpm request a previous instance may have left in the SAM. The request
    /// lives in the SAM, not in the daemon: without this a restart leaves the fan at the old
    /// speed while FanRpmRequest reads 0. Only when requests are allowed, so a daemon that
    /// never used the feature sends nothing.
    pub fn withdraw_stale_fan_request(&self) -> std::io::Result<()> {
        if self.cfg().fan.allow_rpm_request && self.b.ssam.available() {
            ssam::fan_rpm_request(&*self.b.ssam, 0)?;
        }
        Ok(())
    }
}

fn data_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("SP9_DATA_DIR") {
        return PathBuf::from(d);
    }
    std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|p| p.join("sp9-helper")))
        .unwrap_or_else(|| PathBuf::from("/usr/libexec/sp9-helper"))
}

/// Emit PropertiesChanged for an object whose snapshot changed.
async fn watch<T: zbus::object_server::Interface + Snapshot>(
    conn: &zbus::Connection,
    path: &'static str,
    last: &mut HashMap<&'static str, String>,
) {
    let Ok(iface) = conn.object_server().interface::<_, T>(path).await else { return };
    let snap = iface.get().await.snapshot();
    if last.get(path) != Some(&snap) {
        if last.contains_key(path) {
            if let Ok(em) = SignalEmitter::new(conn, path) {
                invalidate(&em, T::IFACE, T::PROPS).await;
            }
        }
        last.insert(path, snap);
    }
}

impl Shared {
    pub fn new(b: Backends, root: Root, cfg_path: PathBuf, no_polkit: bool) -> Shared {
        let (cfg, warnings, readable) = Config::load_checked(&cfg_path);
        for w in warnings {
            eprintln!("sp9-helperd: config: {w}");
        }
        let has_perf = b.power.profile().is_some() || !b.power.rapl().is_empty();
        let has_battery = b.battery.battery().is_some() || b.ssam.available();
        let has_display = b.display.enable_psr_param().is_some() || b.display.panel_on().is_some() || b.display.calibration().is_some();
        let features = [
            (has_perf, "Performance"),
            (has_battery, "Battery"),
            (has_display, "Display"),
            (true, "Input"),
            (true, "Pen"),
            (true, "Diagnostics"),
            (true, "Install"),
        ]
        .into_iter()
        .filter(|(on, _)| *on)
        .map(|(_, n)| n.to_string())
        .collect();
        Shared {
            cfg: Mutex::new(cfg),
            cfg_path,
            b,
            root,
            rt: Mutex::new(Runtime::default()),
            no_polkit,
            state_dir: std::env::var_os("SP9_STATE_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/var/lib/sp9")),
            data_dir: data_dir(),
            python: std::env::var("SP9_PYTHON").unwrap_or_else(|_| "python3".into()),
            features,
            sc_cache: Mutex::new(None),
            last_report: Mutex::new(String::new()),
            couple: couple::Couple::default(),
            hardware: false,
            cfg_unreadable: AtomicBool::new(!readable),
            stop: AtomicBool::new(false),
            pen_done: AtomicBool::new(false),
        }
    }
}

/// Put the objects of `shared.features` on the connection being built.
pub fn serve<'a>(bld: zbus::connection::Builder<'a>, shared: &Arc<Shared>) -> zbus::Result<zbus::connection::Builder<'a>> {
    let mut bld = bld.serve_at(ROOT, Helper(shared.clone()))?;
    if shared.has("Performance") {
        bld = bld.serve_at(P_PERFORMANCE, Performance(shared.clone()))?;
    }
    if shared.has("Battery") {
        bld = bld.serve_at(P_BATTERY, Battery(shared.clone()))?;
    }
    if shared.has("Display") {
        bld = bld.serve_at(P_DISPLAY, Display(shared.clone()))?;
    }
    bld = bld.serve_at(P_INPUT, Input(shared.clone()))?;
    bld = bld.serve_at(P_PEN, Pen(shared.clone()))?;
    bld = bld.serve_at(P_DIAG, Diagnostics(shared.clone()))?;
    bld.serve_at(P_INSTALL, Install(shared.clone()))
}

async fn run(no_polkit: bool, session: bool, fake: bool) -> zbus::Result<()> {
    let root = Root::from_env();
    let b = if fake { Backends::fake() } else { Backends::system(root.clone()) };
    let mut shared = Shared::new(b, root, config_path(), no_polkit);
    shared.hardware = !fake && !session;
    let shared = Arc::new(shared);

    // the iptsd drop-in follows the settings (also after a manual edit of helper.toml)
    if let Err(e) = ifaces::apply_iptsd(&shared) {
        eprintln!("sp9-helperd: touch and pen settings: {e}");
    }
    if let Err(e) = shared.withdraw_stale_fan_request() {
        eprintln!("sp9-helperd: withdrawing the fan request: {e}");
    }

    let bld = if session { zbus::connection::Builder::session()? } else { zbus::connection::Builder::system()? };
    let bld = serve(bld, &shared)?;
    let conn = bld.name(BUS)?.build().await?;
    // only the owner of the bus name touches the devices: a second instance ends before this
    penwatch::spawn(shared.clone());
    kbdwatch::spawn(shared.clone(), conn.clone());
    if shared.hardware {
        couple::spawn(shared.clone(), conn.clone());
    }
    eprintln!(
        "sp9-helperd {VERSION} on the {} bus{}: {}",
        if session { "session" } else { "system" },
        if fake { " (fake backends)" } else { "" },
        shared.features.join(" ")
    );

    // Policy step every second (profile limits, skin guard, PSR, touch off); PropertiesChanged
    // every 2 s for objects whose values changed underneath.
    let mut last: HashMap<&'static str, String> = HashMap::new();
    let mut prev = Instant::now();
    let mut n: u64 = 0;
    loop {
        let dt = prev.elapsed().as_secs_f64().clamp(0.1, 10.0);
        prev = Instant::now();
        let cfg = shared.cfg();
        let errors = {
            let mut rt = shared.rt.lock().unwrap();
            // a long gap is a suspend: write everything again after resume
            if dt >= 10.0 {
                rt.reset();
            }
            engine::tick(&shared.b, &cfg, &mut rt, dt);
            rt.take_errors()
        };
        for e in errors {
            eprintln!("sp9-helperd: {e}");
        }
        if n % 2 == 0 {
            watch::<Helper>(&conn, ROOT, &mut last).await;
            if shared.has("Performance") {
                watch::<Performance>(&conn, P_PERFORMANCE, &mut last).await;
            }
            if shared.has("Battery") {
                watch::<Battery>(&conn, P_BATTERY, &mut last).await;
            }
            if shared.has("Display") {
                watch::<Display>(&conn, P_DISPLAY, &mut last).await;
            }
            watch::<Input>(&conn, P_INPUT, &mut last).await;
            watch::<Pen>(&conn, P_PEN, &mut last).await;
            watch::<Diagnostics>(&conn, P_DIAG, &mut last).await;
            watch::<Install>(&conn, P_INSTALL, &mut last).await;
        }
        n += 1;
        for _ in 0..4 {
            if TERMINATE.load(Ordering::SeqCst) {
                // let the pen thread stop a running waveform and take a contact back
                shared.stop.store(true, Ordering::SeqCst);
                let end = Instant::now() + Duration::from_secs(2);
                while !shared.pen_done.load(Ordering::SeqCst) && Instant::now() < end {
                    async_io::Timer::after(Duration::from_millis(20)).await;
                }
                eprintln!("sp9-helperd: terminated");
                return Ok(());
            }
            async_io::Timer::after(Duration::from_millis(250)).await;
        }
    }
}

/// SIGTERM or SIGINT arrived; the main loop winds down.
static TERMINATE: AtomicBool = AtomicBool::new(false);

#[cfg(unix)]
extern "C" fn on_terminate(_: libc::c_int) {
    TERMINATE.store(true, Ordering::SeqCst);
}

#[cfg(unix)]
fn catch_termination() {
    for sig in [libc::SIGTERM, libc::SIGINT] {
        // SAFETY: the handler only stores to an atomic.
        unsafe { libc::signal(sig, on_terminate as extern "C" fn(libc::c_int) as libc::sighandler_t) };
    }
}

#[cfg(not(unix))]
fn catch_termination() {}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("usage: sp9-helperd [--session] [--no-polkit] [--fake] | --version");
        return;
    }
    if args.iter().any(|a| a == "--version") {
        println!("sp9-helperd {VERSION}");
        return;
    }
    let no_polkit = args.iter().any(|a| a == "--no-polkit");
    let session = args.iter().any(|a| a == "--session");
    let fake = args.iter().any(|a| a == "--fake");
    if no_polkit {
        eprintln!("sp9-helperd: WARNING: --no-polkit, every caller is authorized (tests only)");
    }
    catch_termination();
    if let Err(e) = zbus::block_on(run(no_polkit, session, fake)) {
        eprintln!("sp9-helperd: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests;
