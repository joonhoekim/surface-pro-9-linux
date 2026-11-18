// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
//! sp9-digitizerd: touch and pen daemon for the digitizer of the Surface Pro 9 (Intel).
//! Reads raw reports from the hidraw node and feeds two uinput devices.
//!
//! The digitizer's HID device is torn down and set up again at every suspend, and its hidraw
//! node comes back under another number. The daemon outlives that: the uinput devices are
//! created once and stay (clients keep stale state when their devices are replaced), the
//! pen and all contacts are lifted when the node goes away, and the node is found again by
//! its ID.
//!
//!   sp9-digitizerd [--config FILE] [--iptsd-conf FILE]... [DEVICE]
//!     DEVICE             the hidraw node of the digitizer (045E:0C52); found by its ID
//!                        if not given
//!     --config FILE      settings (TOML); default /etc/sp9/digitizer.toml if it exists
//!     --iptsd-conf FILE  an iptsd configuration file whose known keys are applied on top;
//!                        default /etc/iptsd.d/zz-sp9-helper.conf if it exists

use sp9_digitizer::config::{PRODUCT, VENDOR};
use sp9_digitizer::control::PenState;
use sp9_digitizer::frame::{Event, Metadata, Parser};
use sp9_digitizer::hid::Descriptor;
use sp9_digitizer::output::{Device, Sink};
use sp9_digitizer::pen::PenSample;
use sp9_digitizer::touch::Contact;
use sp9_digitizer::Config;
use sp9_digitizer::{Digitizer, Geometry};
use sp9_digitizerd::conf::{self, Sources};
use sp9_digitizerd::find;
use sp9_digitizerd::hidraw::Hidraw;
use sp9_digitizerd::service::{self, Shared};
use sp9_digitizerd::uinput::Uinput;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Mode of the device: it sends single-touch HID reports by itself, or raw data.
const MODE_SINGLETOUCH: u8 = 0;
const MODE_MULTITOUCH: u8 = 1;

/// After this many failed reports in a row the node is closed and opened again.
const MAX_ERRORS: u32 = 50;
/// How often the node is looked for while it is not there.
const SEARCH_INTERVAL: Duration = Duration::from_millis(250);
/// The device is told to send raw data again at most this often when it sends something else.
const MODE_INTERVAL: Duration = Duration::from_secs(1);

static STOP: AtomicBool = AtomicBool::new(false);

extern "C" fn on_terminate(_: libc::c_int) {
    STOP.store(true, Ordering::Relaxed);
}

/// SIGTERM and SIGINT set the stop flag and interrupt the blocking read (no SA_RESTART).
fn install_signal_handlers() {
    for sig in [libc::SIGTERM, libc::SIGINT] {
        // SAFETY: the handler only stores to an atomic.
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = on_terminate as extern "C" fn(libc::c_int) as libc::sighandler_t;
            libc::sigemptyset(&mut action.sa_mask);
            libc::sigaction(sig, &action, std::ptr::null_mut());
        }
    }
}

const USAGE: &str = "\
usage: sp9-digitizerd [OPTIONS] [DEVICE]

Touch and pen daemon for the digitizer of the Surface Pro 9 (045E:0C52). Reads raw reports
from its hidraw node and feeds a uinput touchscreen and a uinput stylus. Switches the device
to raw data while it runs and back when it is stopped (SIGTERM, SIGINT).

Without DEVICE the node is found by the ID of the device, waited for if it is not there, and
found again when it went away and came back (it does at every suspend, under a new number).
With DEVICE only that path is used.

options:
  --config FILE      settings (TOML); default /etc/sp9/digitizer.toml if it exists
  --iptsd-conf FILE  an iptsd configuration file whose known keys are applied on top (may be
                     given more than once); default /etc/iptsd.d/zz-sp9-helper.conf if it exists
  -h, --help         this text";

struct Args {
    device: Option<PathBuf>,
    sources: Sources,
}

fn parse_args() -> Result<Args, String> {
    let mut device = None;
    let mut sources = Sources::default();
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--config") => sources.toml = Some(args.next().ok_or("--config needs a file")?.into()),
            Some("--iptsd-conf") => sources.iptsd.push(args.next().ok_or("--iptsd-conf needs a file")?.into()),
            Some("-h" | "--help") => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            Some(s) if s.starts_with("--") => return Err(format!("unknown option {s} (--help lists the options)")),
            _ if device.is_none() => device = Some(PathBuf::from(arg)),
            _ => return Err("more than one device".into()),
        }
    }
    Ok(Args { device, sources })
}

/// The sensor geometry from the metadata feature report.
fn read_geometry(hidraw: &Hidraw, descriptor: &Descriptor) -> Result<Geometry, String> {
    let report = descriptor.metadata_report().ok_or("the device has no metadata report")?;
    let id = report.id.ok_or("the metadata report has no ID")?;
    let mut buf = vec![0u8; report.bytes() + 1];
    buf[0] = id;
    hidraw.get_feature(&mut buf).map_err(|e| format!("reading the metadata: {e}"))?;

    let mut metadata: Option<Metadata> = None;
    Parser::new()
        .parse_feature(&buf, &mut |event| {
            if let Event::Metadata(m) = event {
                metadata = Some(m);
            }
        })
        .map_err(|e| format!("the metadata does not parse: {e}"))?;
    let m = metadata.ok_or("the metadata report is empty")?;

    if m.invert_x || m.invert_y {
        return Err("the device reports mirrored axes, which this daemon does not handle".into());
    }
    if m.rows < 2 || m.columns < 2 || m.width <= 0.0 || m.height <= 0.0 {
        return Err(format!("implausible metadata: {m:?}"));
    }
    Ok(Geometry { rows: m.rows, columns: m.columns, width_cm: m.width, height_cm: m.height })
}

/// The open node and what its report descriptor says.
struct Node {
    path: PathBuf,
    hidraw: Hidraw,
    geometry: Geometry,
    /// IDs of the raw data reports.
    raw_reports: Vec<u8>,
    mode_report: u8,
    /// Size of the largest raw data report with its ID byte.
    buffer: usize,
}

impl Node {
    fn open(path: &Path) -> Result<Node, String> {
        let hidraw = Hidraw::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let (vendor, product) = hidraw.ids().map_err(|e| format!("{}: {e}", path.display()))?;
        if (vendor, product) != (VENDOR, PRODUCT) {
            return Err(format!("{} is {vendor:04X}:{product:04X}, not the digitizer {VENDOR:04X}:{PRODUCT:04X}", path.display()));
        }

        let raw = hidraw.descriptor().map_err(|e| format!("reading the report descriptor: {e}"))?;
        let descriptor = Descriptor::parse(&raw).map_err(|_| "the report descriptor is truncated")?;
        let reports: Vec<(u8, usize)> = descriptor.touch_data_reports().iter().filter_map(|r| r.id.map(|id| (id, r.bytes()))).collect();
        let mode_report = descriptor.mode_report().and_then(|r| r.id);
        let (Some(mode_report), false, true) = (mode_report, reports.is_empty(), descriptor.is_touchscreen() && !descriptor.is_touchpad())
        else {
            return Err(format!("{} does not look like the raw-data touchscreen", path.display()));
        };
        let geometry = read_geometry(&hidraw, &descriptor)?;

        Ok(Node {
            path: path.to_path_buf(),
            hidraw,
            geometry,
            raw_reports: reports.iter().map(|&(id, _)| id).collect(),
            mode_report,
            buffer: reports.iter().map(|&(_, bytes)| bytes).max().unwrap_or(0) + 1,
        })
    }

    fn set_mode(&self, mode: u8) -> std::io::Result<()> {
        self.hidraw.set_feature(&[self.mode_report, mode])
    }
}

/// Looks for the node until it can be opened or the daemon is told to stop.
fn wait_for_node(fixed: Option<&Path>) -> Option<Node> {
    let mut complaint = String::new();
    while !STOP.load(Ordering::Relaxed) {
        let candidates = match fixed {
            Some(path) => vec![path.to_path_buf()],
            None => find::digitizer_nodes(Path::new("/sys")),
        };
        let mut problem = if candidates.is_empty() { "no digitizer (045E:0C52) among the hidraw nodes".to_string() } else { String::new() };
        for path in &candidates {
            match Node::open(path) {
                Ok(node) => match node.set_mode(MODE_MULTITOUCH) {
                    Ok(()) => return Some(node),
                    Err(e) => problem = format!("{}: switching to raw data: {e}", path.display()),
                },
                Err(e) => problem = e,
            }
        }
        // Say it once per kind of problem, not four times a second.
        if problem != complaint {
            eprintln!("sp9-digitizerd: waiting for the digitizer: {problem}");
            complaint = problem;
        }
        std::thread::sleep(SEARCH_INTERVAL);
    }
    None
}

/// The uinput devices behind a sink that counts what goes through.
struct Output<'a> {
    uinput: &'a mut Uinput,
    shared: &'a Shared,
}

impl Sink for Output<'_> {
    fn event(&mut self, device: Device, kind: u16, code: u16, value: i32) {
        self.uinput.event(device, kind, code, value);
    }
    fn sync(&mut self, device: Device) {
        self.uinput.sync(device);
    }
    fn pen_sample(&mut self, _sample: &PenSample) {
        self.shared.counters.pen_samples.fetch_add(1, Ordering::Relaxed);
    }
    fn contacts(&mut self, _contacts: &[Contact]) {
        self.shared.counters.touch_frames.fetch_add(1, Ordering::Relaxed);
    }
}

/// Everything that is created once, when the digitizer is seen for the first time, and lives
/// until the daemon ends.
struct Running {
    geometry: Geometry,
    uinput: Uinput,
    digitizer: Digitizer,
    shared: Arc<Shared>,
    changes: service::Changes,
}

impl Running {
    fn start(config: &Config, node: &Node) -> Result<Running, String> {
        let specs = Digitizer::devices(config, &node.geometry);
        if config.touch.disable {
            eprintln!("sp9-digitizerd: the touchscreen is disabled");
        }
        if config.pen.disable {
            eprintln!("sp9-digitizerd: the stylus is disabled");
        }
        let uinput = Uinput::create(&specs).map_err(|e| format!("/dev/uinput: {e}"))?;
        let shared = Arc::new(Shared::new(config.clone(), &node.path.display().to_string(), node.geometry, false));
        let (changes, receiver) = std::sync::mpsc::channel();
        service::spawn(shared.clone(), receiver);
        Ok(Running { geometry: node.geometry, uinput, digitizer: Digitizer::new(config, &node.geometry), shared, changes })
    }

    /// Publishes the pen state after a report (or after everything was lifted).
    fn publish(&self) {
        let state = PenState::of(self.digitizer.pen());
        if self.shared.store_pen(state) {
            let _ = self.changes.send(state);
        }
    }

    /// The node is gone: nothing stays pressed, the stream starts anew.
    fn release(&mut self) {
        self.digitizer.release(&mut self.uinput);
        let _ = self.uinput.take_error();
        self.publish();
    }
}

fn run() -> Result<bool, String> {
    let args = parse_args()?;
    let (config, notes) = conf::load(&args.sources)?;
    for note in notes {
        eprintln!("sp9-digitizerd: config: {note}");
    }
    install_signal_handlers();

    let started = std::time::Instant::now();
    let mut running: Option<Running> = None;

    while let Some(node) = wait_for_node(args.device.as_deref()) {
        let run = match &mut running {
            Some(run) => run,
            None => running.insert(Running::start(&config, &node)?),
        };
        // The input devices were made for this sensor; another one needs a new daemon.
        run.shared.set_device(&node.path.display().to_string());
        if node.geometry != run.geometry {
            return Err(format!("{} reports another sensor ({:?}, was {:?})", node.path.display(), node.geometry, run.geometry));
        }
        eprintln!(
            "sp9-digitizerd: {} ({}x{} antennas, {} x {} cm), {} raw report types",
            node.path.display(),
            node.geometry.columns,
            node.geometry.rows,
            node.geometry.width_cm,
            node.geometry.height_cm,
            node.raw_reports.len()
        );

        let why = read_reports(&node, run, started);
        run.release();
        match why {
            Left::Stopped => {
                if let Err(e) = node.set_mode(MODE_SINGLETOUCH) {
                    eprintln!("sp9-digitizerd: switching back to single-touch: {e}");
                }
            }
            Left::Gone(e) => eprintln!("sp9-digitizerd: {} went away ({e}); looking for the digitizer again", node.path.display()),
        }
    }
    Ok(true)
}

/// Why the reading of a node ended.
enum Left {
    Stopped,
    Gone(String),
}

/// Reads and processes reports until the daemon is told to stop or the node is of no use any more.
fn read_reports(node: &Node, run: &mut Running, started: std::time::Instant) -> Left {
    let mut buf = vec![0u8; node.buffer];
    let mut errors = 0;
    let mut mode_set = std::time::Instant::now();

    while !STOP.load(Ordering::Relaxed) {
        if errors >= MAX_ERRORS {
            return Left::Gone(format!("{MAX_ERRORS} failed reports in a row"));
        }
        // A setting changed over D-Bus is in force from here on.
        if let Some(config) = run.shared.take_changed_config() {
            if let Err(e) = run.digitizer.set_config(&config) {
                eprintln!("sp9-digitizerd: the changed settings were not applied: {e}");
            }
        }

        let n = match node.hidraw.read(&mut buf) {
            Ok(0) => return Left::Gone("end of file".into()),
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            // What a read gives once the HID device behind the node was removed.
            Err(e) if matches!(e.raw_os_error(), Some(libc::ENODEV | libc::EIO | libc::ENXIO)) => return Left::Gone(e.to_string()),
            Err(e) => {
                eprintln!("sp9-digitizerd: read: {e}");
                errors += 1;
                continue;
            }
        };

        if !node.raw_reports.contains(&buf[0]) {
            // Reports of the device's own touch processing: it is not in raw data mode
            // (any more). Ask again, but not for every report.
            if mode_set.elapsed() >= MODE_INTERVAL {
                mode_set = std::time::Instant::now();
                if let Err(e) = node.set_mode(MODE_MULTITOUCH) {
                    eprintln!("sp9-digitizerd: switching to raw data again: {e}");
                }
            }
            continue;
        }

        let counters = &run.shared.counters;
        counters.reports.fetch_add(1, Ordering::Relaxed);
        let mut output = Output { uinput: &mut run.uinput, shared: &run.shared };
        let parsed = run.digitizer.process(&buf[..n], started.elapsed().as_secs_f64(), &mut output);
        let written = run.uinput.take_error();
        run.publish();

        if let Err(e) = &parsed {
            counters.parse_errors.fetch_add(1, Ordering::Relaxed);
            eprintln!("sp9-digitizerd: report {:#04x}: {e}", buf[0]);
        }
        if let Some(e) = &written {
            counters.write_errors.fetch_add(1, Ordering::Relaxed);
            eprintln!("sp9-digitizerd: uinput: {e}");
        }
        errors = if parsed.is_ok() && written.is_none() { 0 } else { errors + 1 };
    }
    Left::Stopped
}

fn main() -> ExitCode {
    match run() {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("sp9-digitizerd: {e}");
            ExitCode::FAILURE
        }
    }
}
