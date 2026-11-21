// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
//! sp9-digitizer-replay: replays a capture of raw reports through the processing and prints
//! everything it emits (format: `sp9_digitizerd::replay`).
//!
//!   sp9-digitizer-replay [--config FILE] [--iptsd-conf FILE]... [--rdesc FILE] CAPTURE
//!     --config FILE      settings (TOML) on top of the built-in ones
//!     --iptsd-conf FILE  an iptsd configuration file applied on top
//!     --rdesc FILE       the report descriptor of the device: replay only the reports the
//!                        daemon would process, cut to its read buffer. Without a capture:
//!                        print what the daemon would take from the descriptor
//!
//! Unlike the daemon it reads no configuration file unless one is named. The sensor geometry
//! is the Surface Pro 9's (the capture does not contain the metadata feature report).

use sp9_digitizer::hid::Descriptor;
use sp9_digitizer::{Config, Geometry};
use sp9_digitizerd::replay::{replay, ReportFilter};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::process::ExitCode;

/// What the daemon takes from a report descriptor.
fn describe(d: &Descriptor) {
    let id = |r: &sp9_digitizer::hid::Report| r.id.map_or("none".to_string(), |i| i.to_string());
    println!("touchscreen: {}, touchpad: {}", d.is_touchscreen(), d.is_touchpad());
    let touch = d.touch_data_reports();
    println!(
        "raw data reports (ID: bytes without the ID): {}",
        touch.iter().map(|r| format!("{}: {}", id(r), r.bytes())).collect::<Vec<_>>().join(", ")
    );
    println!("read buffer: {} bytes", ReportFilter::new(d).buffer_size());
    match d.mode_report() {
        Some(r) => println!("mode feature report: ID {}, {} byte", id(r), r.bytes()),
        None => println!("mode feature report: none"),
    }
    match d.metadata_report() {
        Some(r) => println!("metadata feature report: ID {}, {} bytes", id(r), r.bytes()),
        None => println!("metadata feature report: none"),
    }
}

const USAGE: &str = "\
usage: sp9-digitizer-replay [OPTIONS] CAPTURE
       sp9-digitizer-replay --rdesc FILE

Replays a capture of the digitizer's raw reports through the touch and pen processing and
prints everything it emits: one line per pen sample (P), per image's contacts (C) and per
input event packet (E), prefixed with the capture time of the report. Floating point values
are hexadecimal, so equal dumps mean equal results to the bit.

A capture is a sequence of records: f64 time, u8 source, u32 length, then the bytes (little
endian). Records of source 0 are reports as read from the hidraw node; others are skipped.

options:
  --config FILE      settings (TOML) on top of the built-in ones
  --iptsd-conf FILE  an iptsd configuration file applied on top (may be given more than once)
  --rdesc FILE       the report descriptor of the device (the file report_descriptor of the
                     hidraw node in sysfs): replay only the reports the daemon would process,
                     cut to its read buffer. Without a capture: print what the daemon takes
                     from the descriptor (raw data reports, mode and metadata reports)
  -h, --help         this text

No configuration file is read unless one is named. The sensor geometry is the Surface Pro 9's.";

fn run() -> Result<(), String> {
    let mut config = Config::default();
    let mut filter = None;
    let mut descriptor = None;
    let mut capture = None;

    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        let mut file = |what: &str| -> Result<(PathBuf, Vec<u8>), String> {
            let path = PathBuf::from(args.next().ok_or(format!("{what} needs a file"))?);
            let data = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            Ok((path, data))
        };
        match arg.to_str() {
            Some("--config") => {
                let (path, data) = file("--config")?;
                config = toml::from_str(&String::from_utf8_lossy(&data)).map_err(|e| format!("{}: {e}", path.display()))?;
            }
            Some("--iptsd-conf") => {
                let (path, data) = file("--iptsd-conf")?;
                let report = config.apply_iptsd_conf(&String::from_utf8_lossy(&data)).map_err(|e| format!("{}: {e}", path.display()))?;
                if report.other_device {
                    return Err(format!("{} is for another device", path.display()));
                }
                if !report.ignored.is_empty() {
                    eprintln!("sp9-digitizer-replay: {}: keys without a meaning here: {}", path.display(), report.ignored.join(", "));
                }
            }
            Some("--rdesc") => {
                let (path, data) = file("--rdesc")?;
                let parsed = Descriptor::parse(&data).map_err(|_| format!("{}: truncated report descriptor", path.display()))?;
                filter = Some(ReportFilter::new(&parsed));
                descriptor = Some(parsed);
            }
            Some("-h" | "--help") => {
                println!("{USAGE}");
                return Ok(());
            }
            Some(s) if s.starts_with('-') && s.len() > 1 => return Err(format!("unknown option {s} (--help lists the options)")),
            _ if capture.is_none() => capture = Some(PathBuf::from(arg)),
            _ => return Err("more than one capture".into()),
        }
    }

    if let (None, Some(d)) = (&capture, &descriptor) {
        describe(d);
        return Ok(());
    }
    let capture = capture.ok_or("no capture named (--help shows the usage)")?;
    config.validate()?;
    let data = std::fs::read(&capture).map_err(|e| format!("{}: {e}", capture.display()))?;

    let mut out = BufWriter::new(std::io::stdout().lock());
    let summary = replay(&data, &config, &Geometry::SP9, filter.as_ref(), &mut out).map_err(|e| e.to_string())?;
    out.flush().map_err(|e| e.to_string())?;
    eprintln!("sp9-digitizer-replay: {} reports, {} with a parse error", summary.reports, summary.errors);
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("sp9-digitizer-replay: {e}");
            ExitCode::FAILURE
        }
    }
}
