// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
//! Replays captures of the device and compares the dump with golden dumps of the C++ reference,
//! byte for byte (every packet, and every bit of every pen sample and contact).
//!
//! The captures and the golden dumps are not in the repository. The test runs when both
//! directories are named:
//!
//!   SP9_CAPTURE_DIR   <capture>.bin files
//!   SP9_GOLDEN_DIR    rdesc.bin (the report descriptor), conf/<name>.conf (iptsd configuration
//!                     files the reference was run with), <name>/<capture>.txt (its dumps)
//!
//! With SP9_GOLDEN_UPDATE=1 the Rust dumps are written as <name>/<capture>.rs.txt next to the
//! golden ones instead of failing, for a diff to review after an intended change of behaviour.

use sp9_digitizer::hid::Descriptor;
use sp9_digitizer::{Config, Geometry};
use sp9_digitizerd::replay::{replay, replay_with, ReportFilter};
use std::path::PathBuf;

/// The first line that differs, with its number.
fn first_difference(golden: &str, ours: &str) -> String {
    let mut a = golden.lines();
    let mut b = ours.lines();
    let mut n = 1;
    loop {
        match (a.next(), b.next()) {
            (Some(x), Some(y)) if x == y => n += 1,
            (x, y) => return format!("line {n}:\n  golden: {}\n  ours:   {}", x.unwrap_or("<end>"), y.unwrap_or("<end>")),
        }
    }
}

#[test]
fn captures_replay_to_the_golden_dumps() {
    let (Some(captures), Some(golden)) = (std::env::var_os("SP9_CAPTURE_DIR"), std::env::var_os("SP9_GOLDEN_DIR")) else {
        eprintln!("skipped: SP9_CAPTURE_DIR and SP9_GOLDEN_DIR are not set");
        return;
    };
    let (captures, golden) = (PathBuf::from(captures), PathBuf::from(golden));
    let update = std::env::var_os("SP9_GOLDEN_UPDATE").is_some();

    let rdesc = std::fs::read(golden.join("rdesc.bin")).expect("rdesc.bin in SP9_GOLDEN_DIR");
    let filter = ReportFilter::new(&Descriptor::parse(&rdesc).expect("a complete report descriptor"));

    let mut confs: Vec<PathBuf> =
        std::fs::read_dir(golden.join("conf")).expect("conf/ in SP9_GOLDEN_DIR").map(|e| e.unwrap().path()).collect();
    confs.sort();
    assert!(!confs.is_empty(), "no configuration in {}", golden.join("conf").display());

    let mut compared = 0;
    let mut failures = Vec::new();

    for conf in &confs {
        let name = conf.file_stem().unwrap().to_string_lossy().into_owned();
        let mut config = Config::default();
        let report = config.apply_iptsd_conf(&std::fs::read_to_string(conf).unwrap()).unwrap_or_else(|e| panic!("{}: {e}", conf.display()));
        assert!(!report.other_device, "{} is for another device", conf.display());
        config.validate().unwrap();

        let mut dumps: Vec<PathBuf> = std::fs::read_dir(golden.join(&name))
            .unwrap_or_else(|e| panic!("{name}/ in SP9_GOLDEN_DIR: {e}"))
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|x| x == "txt") && !p.to_string_lossy().ends_with(".rs.txt"))
            .collect();
        dumps.sort();

        for dump in &dumps {
            let capture_name = dump.file_stem().unwrap().to_string_lossy().into_owned();
            let capture = std::fs::read(captures.join(format!("{capture_name}.bin"))).unwrap_or_else(|e| panic!("{capture_name}.bin: {e}"));
            let expected = std::fs::read_to_string(dump).unwrap();

            let mut ours = Vec::new();
            replay(&capture, &config, &Geometry::SP9, Some(&filter), &mut ours).unwrap();
            let ours = String::from_utf8(ours).unwrap();
            compared += 1;

            if ours != expected {
                if update {
                    std::fs::write(dump.with_extension("rs.txt"), &ours).unwrap();
                }
                failures.push(format!("{name}/{capture_name}: {}", first_difference(&expected, &ours)));
            }
        }
    }

    eprintln!("compared {compared} dumps");
    assert!(compared > 0, "no golden dump found");
    assert!(update || failures.is_empty(), "{} of {compared} dumps differ:\n{}", failures.len(), failures.join("\n"));
}

/// Settings changed in the middle of a capture, as over D-Bus: a pen resting on the glass
/// without force (sel1) is not in contact while zero force contact is off, and is from the
/// moment it is switched on, without leaving range.
#[test]
fn settings_change_in_the_middle_of_a_capture() {
    use sp9_digitizer::control::{set_params, Param, Value};

    let (Some(captures), Some(golden)) = (std::env::var_os("SP9_CAPTURE_DIR"), std::env::var_os("SP9_GOLDEN_DIR")) else {
        eprintln!("skipped: SP9_CAPTURE_DIR and SP9_GOLDEN_DIR are not set");
        return;
    };
    let (captures, golden) = (PathBuf::from(captures), PathBuf::from(golden));
    let rdesc = std::fs::read(golden.join("rdesc.bin")).expect("rdesc.bin in SP9_GOLDEN_DIR");
    let filter = ReportFilter::new(&Descriptor::parse(&rdesc).unwrap());
    let capture = std::fs::read(captures.join("sel1.bin")).expect("sel1.bin");

    let conf = |name: &str| {
        let mut c = Config::default();
        c.apply_iptsd_conf(&std::fs::read_to_string(golden.join("conf").join(name)).unwrap()).unwrap();
        c
    };
    let (off, on) = (conf("zf-off.conf"), conf("effective.conf"));
    let dump = |hook: &mut dyn FnMut(usize, &mut sp9_digitizer::Digitizer)| {
        let mut out = Vec::new();
        let summary = replay_with(&capture, &off, &Geometry::SP9, Some(&filter), &mut out, hook).unwrap();
        (String::from_utf8(out).unwrap(), summary.reports)
    };

    let (all_off, reports) = dump(&mut |_, _| {});
    let half = reports / 2;
    let (switched, _) = dump(&mut |n, digitizer| {
        if n == half {
            let mut config = digitizer.config().clone();
            let zf = &on.pen.contact.zero_force;
            let params = [
                (Param::ZeroForce, Value::Bool(true)),
                (Param::ZeroForceEnter, Value::Number(zf.enter)),
                (Param::ZeroForceExit, Value::Number(zf.exit)),
                (Param::ZeroForceReference, Value::Number(zf.reference)),
                (Param::PressureFloor, Value::Number(on.pen.pressure.floor)),
            ];
            set_params(&mut config, &params).unwrap();
            digitizer.set_config(&config).unwrap();
        }
    });

    // Pen samples: "<time> P <proximity> <contact> ...".
    let samples = |text: &str| -> Vec<(bool, bool)> {
        text.lines()
            .filter_map(|l| {
                let f: Vec<&str> = l.split(' ').collect();
                (f.get(1) == Some(&"P")).then(|| (f[2] == "1", f[3] == "1"))
            })
            .collect()
    };
    let (a, b) = (samples(&all_off), samples(&switched));
    assert_eq!(a.len(), b.len(), "the change must not add or drop samples");
    assert!(a.iter().all(|s| !s.1), "no contact without zero force contact in this capture");
    assert_eq!(a.iter().filter(|s| s.0).count(), b.iter().filter(|s| s.0).count(), "proximity is untouched");

    // Up to the change the two runs are the same, line for line.
    let common = all_off.lines().zip(switched.lines()).take_while(|(x, y)| x == y).count();
    assert!(common * 3 > all_off.lines().count(), "the runs differ from line {common} on");

    // After it: contact, kept to the end of the capture.
    let first = b.iter().position(|s| s.1).expect("contact after the change");
    let tail = &b[first..];
    let in_contact = tail.iter().filter(|s| s.1).count();
    assert!(
        first * 3 > b.len() && in_contact * 10 >= tail.len() * 9,
        "contact from sample {first} of {}, in {in_contact} of {}",
        b.len(),
        tail.len()
    );
}
