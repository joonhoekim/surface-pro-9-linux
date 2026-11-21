// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
//! Replays a capture of raw reports and writes everything the processing emits as text.
//!
//! A capture is a sequence of records: wall time (`f64`), source (`u8`), length (`u32`), then
//! the bytes; little endian. The wall time is the clock of the replay. Source 0 is a report as read from the hidraw node; other sources
//! (event streams recorded alongside) are skipped.
//!
//! One line per item, prefixed with the wall time of the report it came from:
//!
//! ```text
//! # device "<name>" bus:<n> version:<n> <setup steps>            once per device, first
//! <t> P <proximity> <contact> <button> <eraser> <x> <y> <pressure> <altitude> <azimuth>
//! <t> C <n> [<id> <valid> <stable> <x> <y> <size 0> <size 1> <orientation>] ...
//! <t> E <device> <type>:<code>:<value> ...                         one packet (SYN_REPORT)
//! <t> X                                                            the report ended in a parse error
//! ```
//!
//! Floating point values are hexadecimal (`%a` of C), so two dumps are equal only if every bit
//! of every value is. The format is that of the C++ reference replay.

use sp9_digitizer::hid::Descriptor;
use sp9_digitizer::output::{Device, DeviceSpec, Setup, Sink};
use sp9_digitizer::pen::PenSample;
use sp9_digitizer::touch::Contact;
use sp9_digitizer::{Config, Digitizer, Geometry};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::{self, Write};

/// A double as C's `%a` prints it.
pub fn hex_float(v: f64) -> String {
    if v.is_nan() {
        return if v.is_sign_negative() { "-nan".into() } else { "nan".into() };
    }
    let sign = if v.is_sign_negative() { "-" } else { "" };
    if v.is_infinite() {
        return format!("{sign}inf");
    }
    let bits = v.to_bits();
    let mantissa = bits & ((1u64 << 52) - 1);
    let exponent = ((bits >> 52) & 0x7ff) as i64;
    if exponent == 0 && mantissa == 0 {
        return format!("{sign}0x0p+0");
    }
    let (lead, exp) = if exponent == 0 { (0, -1022) } else { (1, exponent - 1023) };
    let mut digits = format!("{mantissa:013x}");
    while digits.ends_with('0') {
        digits.pop();
    }
    let point = if digits.is_empty() { "" } else { "." };
    format!("{sign}0x{lead}{point}{digits}p{exp:+}")
}

/// Which reports the daemon would process, from the report descriptor.
#[derive(Clone, Debug)]
pub struct ReportFilter {
    ids: Vec<u8>,
    /// The daemon reads into a buffer that holds the largest raw data report with its ID.
    buffer: usize,
}

impl ReportFilter {
    pub fn new(descriptor: &Descriptor) -> ReportFilter {
        let reports = descriptor.touch_data_reports();
        ReportFilter {
            ids: reports.iter().filter_map(|r| r.id).collect(),
            buffer: reports.iter().map(|r| r.bytes()).max().unwrap_or(0) + 1,
        }
    }

    /// The part of a report the daemon would see, or `None` if it would skip it.
    pub fn apply<'a>(&self, report: &'a [u8]) -> Option<&'a [u8]> {
        let id = report.first()?;
        self.ids.contains(id).then(|| &report[..report.len().min(self.buffer)])
    }

    pub fn buffer_size(&self) -> usize {
        self.buffer
    }
}

/// Writes the lines described in the module documentation.
struct TextSink<'a, W: Write> {
    out: &'a mut W,
    time: f64,
    pending: HashMap<Device, String>,
    result: io::Result<()>,
}

impl<W: Write> TextSink<'_, W> {
    fn line(&mut self, text: &str) {
        if self.result.is_ok() {
            self.result = writeln!(self.out, "{:.6} {text}", self.time);
        }
    }
}

impl<W: Write> Sink for TextSink<'_, W> {
    fn event(&mut self, device: Device, kind: u16, code: u16, value: i32) {
        let _ = write!(self.pending.entry(device).or_default(), " {kind}:{code}:{value}");
    }

    fn sync(&mut self, device: Device) {
        let events = self.pending.remove(&device).unwrap_or_default();
        self.line(&format!("E {}{events}", device.label()));
    }

    fn pen_sample(&mut self, s: &PenSample) {
        let line = format!(
            "P {} {} {} {} {} {} {} {} {}",
            u8::from(s.proximity),
            u8::from(s.contact),
            u8::from(s.button),
            u8::from(s.eraser),
            hex_float(s.x),
            hex_float(s.y),
            hex_float(s.pressure),
            hex_float(s.altitude),
            hex_float(s.azimuth)
        );
        self.line(&line);
    }

    fn contacts(&mut self, contacts: &[Contact]) {
        let mut line = format!("C {}", contacts.len());
        for c in contacts {
            let _ = write!(
                line,
                " [{} {} {} {} {} {} {} {}]",
                c.id,
                u8::from(c.valid),
                u8::from(c.stable),
                hex_float(c.x),
                hex_float(c.y),
                hex_float(c.size[0]),
                hex_float(c.size[1]),
                hex_float(c.orientation)
            );
        }
        self.line(&line);
    }
}

fn spec_line(spec: &DeviceSpec) -> String {
    let mut line = format!("# device \"{}\" bus:{} version:{}", spec.name, spec.bus, spec.version);
    for step in &spec.setup {
        let _ = match step {
            Setup::Event(v) => write!(line, " ev:{v}"),
            Setup::Key(v) => write!(line, " key:{v}"),
            Setup::Property(v) => write!(line, " prop:{v}"),
            Setup::Abs { code, min, max, resolution } => write!(line, " abs:{code}:{min}:{max}:{resolution}"),
        };
    }
    line
}

/// What a replay went through.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    /// Reports that were processed.
    pub reports: usize,
    /// Of those, the ones that ended in a parse error.
    pub errors: usize,
}

/// Replays `capture` and writes the dump to `out`. With a filter, only the reports the
/// daemon would process are replayed, cut to the size of its read buffer.
pub fn replay<W: Write>(
    capture: &[u8],
    config: &Config,
    geometry: &Geometry,
    filter: Option<&ReportFilter>,
    out: &mut W,
) -> io::Result<Summary> {
    replay_with(capture, config, geometry, filter, out, &mut |_, _| {})
}

/// Like [`replay`], with `before_report` called before every report that is processed, with
/// its number (from 0) and the processing: for changing settings in the middle of a capture.
pub fn replay_with<W: Write>(
    capture: &[u8],
    config: &Config,
    geometry: &Geometry,
    filter: Option<&ReportFilter>,
    out: &mut W,
    before_report: &mut dyn FnMut(usize, &mut Digitizer),
) -> io::Result<Summary> {
    for spec in Digitizer::devices(config, geometry) {
        writeln!(out, "{}", spec_line(&spec))?;
    }

    let mut digitizer = Digitizer::new(config, geometry);
    let mut sink = TextSink { out, time: 0.0, pending: HashMap::new(), result: Ok(()) };
    let mut summary = Summary::default();

    let mut rest = capture;
    while rest.len() >= 13 {
        let time = f64::from_le_bytes(rest[0..8].try_into().expect("8 bytes"));
        let source = rest[8];
        let len = u32::from_le_bytes(rest[9..13].try_into().expect("4 bytes")) as usize;
        let Some(data) = rest.get(13..13 + len) else { break };
        rest = &rest[13 + len..];

        if source != 0 || data.is_empty() {
            continue;
        }
        let data = match filter {
            Some(f) => match f.apply(data) {
                Some(d) => d,
                None => continue,
            },
            None => data,
        };

        before_report(summary.reports, &mut digitizer);
        sink.time = time;
        summary.reports += 1;
        if digitizer.process(data, time, &mut sink).is_err() {
            summary.errors += 1;
            sink.line("X");
        }
    }

    sink.result?;
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_floats_like_printf() {
        assert_eq!(hex_float(0.0), "0x0p+0");
        assert_eq!(hex_float(-0.0), "-0x0p+0");
        assert_eq!(hex_float(1.0), "0x1p+0");
        assert_eq!(hex_float(-3.0), "-0x1.8p+1");
        assert_eq!(hex_float(f64::from_bits(0x3FDA_A533_3BDC_244B)), "0x1.aa5333bdc244bp-2");
        assert_eq!(hex_float(0.1), "0x1.999999999999ap-4");
        assert_eq!(hex_float(f64::MIN_POSITIVE / 2.0), "0x0.8p-1022");
        assert_eq!(hex_float(f64::INFINITY), "inf");
        assert_eq!(hex_float(f64::NAN), "nan");
    }

    #[test]
    fn replays_records_of_source_zero() {
        let mut capture = Vec::new();
        let mut record = |source: u8, data: &[u8]| {
            capture.extend(12.5f64.to_le_bytes());
            capture.push(source);
            capture.extend((data.len() as u32).to_le_bytes());
            capture.extend(data);
        };
        record(1, &[1, 2, 3]);
        // A report that is too short to parse.
        record(0, &[0x0B, 0]);
        record(0, &[]);
        // A truncated record at the end of the file is dropped.
        capture.extend([0; 20]);

        let mut out = Vec::new();
        let summary = replay(&capture, &Config::default(), &Geometry::SP9, None, &mut out).unwrap();
        assert_eq!(summary, Summary { reports: 1, errors: 1 });
        let text = String::from_utf8(out).unwrap();
        let lines: Vec<_> = text.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(
            lines[0].starts_with("# device \"IPTSD Virtual Touchscreen 045E:0C52\" bus:6 version:0 ev:3 ev:1 key:330 prop:1 abs:47:0:16:0")
        );
        assert!(lines[1].starts_with("# device \"IPTSD Virtual Stylus 045E:0C52\" bus:6 version:0 ev:1 ev:3 prop:1 prop:0 key:330 key:331 key:320 key:321 abs:0:0:27389:100"));
        assert_eq!(lines[2], "12.500000 X");
    }
}
