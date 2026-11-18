// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
// Derived from iptsd (src/core/generic/application.hpp, src/apps/daemon/daemon.hpp),
// GPL-2.0-or-later, Copyright (C) the iptsd authors.
//! Reports in, events out: the parser, the two pipelines and the two output devices, and the
//! rule between them: no touch while the pen is in range and for a moment after it left, and
//! what touched the screen in that time (the hand that holds the pen) stays out until it is
//! lifted.

use crate::config::{Config, Geometry};
use crate::frame::{self, Event, Parser};
use crate::output::{stylus_spec, touchscreen_spec, DeviceSpec, Sink, StylusOutput, TouchOutput};
use crate::pen::{Pen, PenSample};
use crate::touch::TouchPipeline;

pub struct Digitizer {
    parser: Parser,
    core: Core,
    config: Config,
    geometry: Geometry,
}

struct Core {
    touch: TouchPipeline,
    pen: Pen,
    touchscreen: Option<TouchOutput>,
    stylus: Option<StylusOutput>,
    disable_on_stylus: bool,
    pen_holdoff: f64,
    /// Whether the last pen sample was in range, and when the pen was last in range.
    pen_in_range: bool,
    pen_seen: Option<f64>,
}

impl Digitizer {
    pub fn new(config: &Config, geometry: &Geometry) -> Digitizer {
        Digitizer {
            config: config.clone(),
            geometry: *geometry,
            parser: Parser::new(),
            core: Core {
                touch: TouchPipeline::new(&config.touch.contacts, geometry),
                pen: Pen::new(&config.pen, geometry),
                touchscreen: (!config.touch.disable).then(|| TouchOutput::new(config, geometry)),
                stylus: (!config.pen.disable).then(|| StylusOutput::new(config, geometry)),
                disable_on_stylus: config.touch.disable_on_stylus,
                pen_holdoff: config.touch.pen_holdoff_s,
                pen_in_range: false,
                pen_seen: None,
            },
        }
    }

    /// The input devices this configuration reports to, the touchscreen first.
    pub fn devices(config: &Config, geometry: &Geometry) -> Vec<DeviceSpec> {
        let mut specs = Vec::new();
        if !config.touch.disable {
            specs.push(touchscreen_spec(geometry));
        }
        if !config.pen.disable {
            specs.push(stylus_spec(geometry, config.pen.axis_resolution));
        }
        specs
    }

    /// Applies a changed configuration while running: contacts on the screen and a stroke in
    /// progress go on. What decides which input devices exist and what they look like cannot
    /// change this way.
    pub fn set_config(&mut self, config: &Config) -> Result<(), String> {
        config.validate()?;
        let now = &self.config;
        if config.touch.disable != now.touch.disable
            || config.pen.disable != now.pen.disable
            || config.pen.axis_resolution != now.pen.axis_resolution
        {
            return Err("touch.disable, pen.disable and pen.axis_resolution need a restart".into());
        }
        let core = &mut self.core;
        core.touch.set_config(&config.touch.contacts, &self.geometry);
        core.pen.set_config(&config.pen);
        if let Some(touchscreen) = &mut core.touchscreen {
            touchscreen.set_config(config, &self.geometry);
        }
        core.disable_on_stylus = config.touch.disable_on_stylus;
        core.pen_holdoff = config.touch.pen_holdoff_s;
        self.config = config.clone();
        Ok(())
    }

    /// The device went away (or the daemon stops): takes the pen out of range and lifts every
    /// contact, so that nothing stays pressed in the input devices, and forgets everything
    /// about the stream so far. The input devices themselves stay as they are.
    pub fn release(&mut self, sink: &mut dyn Sink) {
        let core = &mut self.core;
        if let Some(stylus) = &mut core.stylus {
            if stylus.active() {
                stylus.update(&PenSample::default(), sink);
            }
        }
        if let Some(touchscreen) = &mut core.touchscreen {
            touchscreen.block(sink);
        }
        let config = self.config.clone();
        *self = Digitizer::new(&config, &self.geometry);
    }

    /// The configuration in force.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Processes one report as read from the device. `now`: a monotonic time in seconds. On
    /// an error the events of the part of the report before it have been sent.
    pub fn process(&mut self, report: &[u8], now: f64, sink: &mut dyn Sink) -> Result<(), frame::Error> {
        let core = &mut self.core;
        self.parser.parse(report, &mut |event| match event {
            Event::Heatmap(heatmap) => {
                let blocked = core.touch_blocked(now);
                if let Some(contacts) = core.touch.process(&heatmap) {
                    sink.contacts(contacts);
                    if let Some(touchscreen) = &mut core.touchscreen {
                        touchscreen.update(contacts, blocked, sink);
                    }
                }
            }
            Event::Dft(window) => {
                if let Some(sample) = core.pen.input(&window) {
                    sink.pen_sample(&sample);
                    core.report_pen(&sample, now, sink);
                }
            }
            Event::Metadata(_) => {}
        })
    }

    pub fn pen(&self) -> &Pen {
        &self.core.pen
    }

    /// For changing the pen configuration while running.
    pub fn pen_mut(&mut self) -> &mut Pen {
        &mut self.core.pen
    }
}

impl Core {
    /// Whether touch is off: the pen is in range, or was a moment ago.
    fn touch_blocked(&self, now: f64) -> bool {
        self.disable_on_stylus && self.stylus.is_some() && (self.pen_in_range || self.pen_seen.is_some_and(|t| now - t < self.pen_holdoff))
    }

    fn report_pen(&mut self, sample: &PenSample, now: f64, sink: &mut dyn Sink) {
        if self.stylus.is_none() {
            return;
        }

        if sample.proximity {
            // The pen arrives: whatever touches the screen is lifted.
            if self.disable_on_stylus && !self.touch_blocked(now) {
                if let Some(touchscreen) = &mut self.touchscreen {
                    touchscreen.block(sink);
                }
            }
            self.pen_seen = Some(now);
        }
        self.pen_in_range = sample.proximity;

        if let Some(stylus) = &mut self.stylus {
            stylus.update(sample, sink);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::testdata::*;
    use crate::frame::{window, Row};
    use crate::output::testsink::Packets;
    use crate::pen::pressure::testwindow;
    use crate::pen::signal::testrows::{bump, row};

    /// The windows of one pen frame with the tip at antenna (x, y).
    fn pen_frame(group: u32, x: f64, y: f64, force: bool, value: u32, button: bool) -> Vec<u8> {
        let tone = |on: bool| row(0, [0; 9], if on { 200_000 } else { 10 });
        let tip = |pos: f64, amplitude: f64| bump((pos.round() as i8) - 4, pos, amplitude, 1.0);
        let ring = |pos: f64| bump((pos.round() as i8) - 4, pos, 2000.0, 1.6);

        // Position window: weak tip tone, force tone in row 1 (closed) or 2 (open).
        let position = |pos: f64| vec![tip(pos, 300.0), tone(force), tone(!force)];
        // MPP v2 position windows: tip, ring, force tone in row 3 (closed) or 2 (open).
        let fine = |pos: f64| vec![tip(pos, 3000.0), ring(pos), tone(!force), tone(force)];
        // Binary window: button bit in row 5 (set) or 4 (clear).
        let binary: Vec<Row> = (0..6).map(|i| tone(i == if button { 5 } else { 4 })).collect();
        // Button window: the button tone in antiphase to the tip tone.
        let mut button_rows = vec![row(0, [0, 0, 0, 0, -500, 0, 0, 0, 0], 250_000), tone(false), tone(false), tone(false)];
        if !button {
            button_rows[0] = tone(false);
        }
        let pressure = testwindow::rows(value);

        let mut f = Vec::new();
        f.extend(dft(group, 1, window::POSITION, 0, &position(x), &position(y)));
        f.extend(dft(group, 2, window::POSITION_MPP2, 2900, &fine(x), &fine(y)));
        f.extend(dft(group, 3, window::BINARY_MPP2, 5000, &binary, &binary));
        f.extend(dft(group, 4, window::BUTTON, 6000, &button_rows, &button_rows));
        f.extend(dft(group, 5, window::PRESSURE, 8000, &pressure, &pressure));
        f.extend(dft(group, 6, window::POSITION_MPP2_SECOND, 10100, &fine(x), &fine(y)));
        f.extend(dft(group, 7, window::BINARY_MPP2, 12000, &binary, &binary));
        dft_report(&f)
    }

    fn no_pen_frame(group: u32) -> Vec<u8> {
        let silent = vec![row(0, [0; 9], 10); 4];
        dft_report(&dft(group, 1, window::POSITION, 0, &silent, &silent))
    }

    fn finger_report(cx: f64, cy: f64) -> Vec<u8> {
        let mut img = vec![200u8; 68 * 46];
        for y in 0..46usize {
            for x in 0..68usize {
                let d2 = (x as f64 - cx).powi(2) + (y as f64 - cy).powi(2);
                img[y * 68 + x] = (200.0 - 120.0 * (-d2 / 3.0).exp()).round() as u8;
            }
        }
        heatmap_report(46, 68, 0, 255, &img)
    }

    #[test]
    fn a_stroke_with_force() {
        let mut d = Digitizer::new(&Config::default(), &Geometry::SP9);
        let mut sink = Packets::default();
        let mut t = 0.0;
        let mut now = || {
            t += 0.015;
            t
        };
        for i in 0..6 {
            d.process(&pen_frame(i, 30.0, 20.0, false, 0, false), now(), &mut sink).unwrap();
        }
        // Twice per frame, from the first MPP v2 position window of the first frame on.
        assert_eq!(sink.samples.len(), 6 * 2);
        assert!(sink.samples.iter().all(|s| s.proximity && !s.contact && s.pressure == 0.0 && !s.button && !s.eraser));
        let hover = *sink.samples.last().unwrap();
        // Antenna 30 is at 0.44 + 30 * 4.07478 mm of 273.89 mm.
        assert!((hover.x * 273.89 - (0.44 + 30.0 * 4.07478)).abs() < 0.3, "{}", hover.x * 273.89);
        assert!((hover.y * 182.59 - (0.94 + 20.0 * 4.01578)).abs() < 0.3);

        sink.samples.clear();
        for i in 6..10 {
            d.process(&pen_frame(i, 30.0, 20.0, true, 520, false), now(), &mut sink).unwrap();
        }
        // The force tone of the position window and of the first MPP v2 window: contact from
        // the first sample of the frame on. The pressure window comes later in the frame: the
        // first sample has no measurement yet and reports the zero force pressure.
        let s = &sink.samples;
        assert!(s.iter().all(|s| s.contact));
        assert!(s[0].pressure > 0.0 && s[0].pressure < 0.2);
        assert_eq!(s.last().unwrap().pressure, 520.0 / 1024.0);

        sink.samples.clear();
        d.process(&pen_frame(10, 30.0, 20.0, false, 0, true), now(), &mut sink).unwrap();
        d.process(&pen_frame(11, 30.0, 20.0, false, 0, true), now(), &mut sink).unwrap();
        let last = sink.samples.last().unwrap();
        assert!(last.button && !last.eraser && !last.contact && last.pressure == 0.0);

        sink.samples.clear();
        sink.packets.clear();
        d.process(&no_pen_frame(12), now(), &mut sink).unwrap();
        assert!(!sink.samples[0].proximity);
        assert_eq!(sink.packets.last().unwrap(), "S 1:330:0 1:320:0 1:321:0 1:331:0");
    }

    /// A frame of the eraser end: its tone in the position window and in the button window,
    /// nothing in the MPP v2 position windows.
    fn eraser_frame(group: u32, pos: f64) -> Vec<u8> {
        let silent = |n: usize| vec![row(0, [0; 9], 10); n];
        let tone = |amplitude: f64| bump((pos.round() as i8) - 4, pos, amplitude, 1.0);
        let position = vec![tone(300.0), row(0, [0; 9], 10), row(0, [0; 9], 10)];
        let button = vec![tone(320.0), row(0, [0; 9], 10), row(0, [0; 9], 10), row(0, [0; 9], 10)];
        let mut f = Vec::new();
        f.extend(dft(group, 1, window::POSITION, 0, &position, &position));
        f.extend(dft(group, 2, window::POSITION_MPP2, 2900, &silent(4), &silent(4)));
        f.extend(dft(group, 3, window::BINARY_MPP2, 5000, &silent(6), &silent(6)));
        f.extend(dft(group, 4, window::BUTTON, 6000, &button, &button));
        f.extend(dft(group, 5, window::PRESSURE, 8000, &silent(16), &silent(16)));
        f.extend(dft(group, 6, window::POSITION_MPP2_SECOND, 10100, &silent(4), &silent(4)));
        dft_report(&f)
    }

    #[test]
    fn a_pen_comes_into_range_with_its_tool() {
        // The eraser end: nothing in its first frame, then the eraser, never the pen.
        let mut d = Digitizer::new(&Config::default(), &Geometry::SP9);
        let mut sink = Packets::default();
        d.process(&eraser_frame(1, 30.0), 0.000, &mut sink).unwrap();
        assert!(sink.packets.is_empty());
        d.process(&eraser_frame(2, 30.0), 0.015, &mut sink).unwrap();
        d.process(&eraser_frame(3, 30.0), 0.030, &mut sink).unwrap();
        // (the first packet is the touchscreen being lifted for the pen)
        let stylus: Vec<_> = sink.packets.iter().filter(|p| p.starts_with('S')).collect();
        assert_eq!(stylus.len(), 2);
        assert!(stylus.iter().all(|p| p.starts_with("S 1:330:0 1:320:0 1:321:1")), "{stylus:?}");

        // The tip: the pen, from the first MPP v2 position window of its first frame.
        let mut d = Digitizer::new(&Config::default(), &Geometry::SP9);
        let mut sink = Packets::default();
        d.process(&pen_frame(1, 30.0, 20.0, false, 0, false), 0.0, &mut sink).unwrap();
        let stylus: Vec<_> = sink.packets.iter().filter(|p| p.starts_with('S')).collect();
        assert_eq!(stylus.len(), 2);
        assert!(stylus.iter().all(|p| p.starts_with("S 1:330:0 1:320:1 1:321:0")), "{stylus:?}");
    }

    #[test]
    fn touch_is_off_while_the_pen_is_in_range() {
        let mut d = Digitizer::new(&Config::default(), &Geometry::SP9);
        let mut sink = Packets::default();
        // The first image has no size yet.
        d.process(&finger_report(30.0, 20.0), 0.00, &mut sink).unwrap();
        assert!(sink.packets.is_empty());
        d.process(&finger_report(30.0, 20.0), 0.01, &mut sink).unwrap();
        assert_eq!(sink.contact_counts, vec![1]);
        assert!(sink.packets[0].starts_with("T 3:47:0 3:57:0 3:53:"));

        // The pen arrives: everything is lifted, and images do not report.
        d.process(&pen_frame(1, 40.0, 20.0, false, 0, false), 0.02, &mut sink).unwrap();
        assert_eq!(sink.packets[1], "T 3:47:0 3:47:0 3:57:-1 1:330:0");
        let before = sink.packets.len();
        d.process(&finger_report(30.0, 20.0), 0.03, &mut sink).unwrap();
        assert_eq!(sink.packets.len(), before);

        // The pen leaves. Touch stays off for a moment ...
        d.process(&no_pen_frame(2), 0.04, &mut sink).unwrap();
        let before = sink.packets.len();
        d.process(&finger_report(30.0, 20.0), 0.05, &mut sink).unwrap();
        assert_eq!(sink.packets.len(), before);
        // ... and the contact that was there with the pen stays out for as long as it is down.
        d.process(&finger_report(30.0, 20.0), 1.0, &mut sink).unwrap();
        assert_eq!(sink.packets.last().unwrap(), "T 1:330:0");
        // Lifted and down again: a touch.
        d.process(&heatmap_report(46, 68, 0, 255, &[200; 68 * 46]), 1.01, &mut sink).unwrap();
        d.process(&finger_report(30.0, 20.0), 1.02, &mut sink).unwrap();
        assert!(sink.packets.last().unwrap().starts_with("T 3:47:0 3:57:0"));
    }

    #[test]
    fn a_touch_after_the_holdoff_is_a_touch() {
        let mut d = Digitizer::new(&Config::default(), &Geometry::SP9);
        let mut sink = Packets::default();
        d.process(&finger_report(30.0, 20.0), 0.0, &mut sink).unwrap();
        d.process(&pen_frame(1, 40.0, 20.0, false, 0, false), 0.01, &mut sink).unwrap();
        d.process(&no_pen_frame(2), 0.02, &mut sink).unwrap();
        // 0.2 s after the pen was last in range.
        let before = sink.packets.len();
        d.process(&finger_report(30.0, 20.0), 0.20, &mut sink).unwrap();
        assert_eq!(sink.packets.len(), before);
        d.process(&heatmap_report(46, 68, 0, 255, &[200; 68 * 46]), 0.21, &mut sink).unwrap();
        d.process(&finger_report(30.0, 20.0), 0.22, &mut sink).unwrap();
        assert!(sink.packets.last().unwrap().starts_with("T 3:47:0 3:57:0"));
    }

    #[test]
    fn settings_change_while_the_pen_is_in_range() {
        use crate::control::{set_params, Param, PenState, Value};

        let mut config = Config::default();
        let mut d = Digitizer::new(&config, &Geometry::SP9);
        let mut sink = Packets::default();
        let mut group = 0;
        let mut frames = |d: &mut Digitizer, sink: &mut Packets, n: u32| {
            for _ in 0..n {
                group += 1;
                d.process(&pen_frame(group, 30.0, 20.0, false, 0, false), f64::from(group) * 0.015, sink).unwrap();
            }
        };

        // The tip rests on the glass without force: no contact while zero force contact is off.
        frames(&mut d, &mut sink, 20);
        assert!(sink.samples.iter().all(|s| s.proximity && !s.contact));
        let energy = d.pen().tip_signal().energy.unwrap();

        // Switched on with the reference at the level of this signal: contact after the run
        // of samples a contact out of hover needs, and the pen never left.
        set_params(
            &mut config,
            &[
                (Param::ZeroForce, Value::Bool(true)),
                (Param::ZeroForceReference, Value::Number(energy)),
                (Param::PressureFloor, Value::Number(0.03)),
            ],
        )
        .unwrap();
        d.set_config(&config).unwrap();
        sink.samples.clear();
        frames(&mut d, &mut sink, 10);
        assert!(sink.samples.iter().all(|s| s.proximity));
        assert!(!sink.samples[6].contact && sink.samples[8].contact, "{:?}", sink.samples.iter().map(|s| s.contact).collect::<Vec<_>>());
        assert!(sink.samples.last().unwrap().pressure >= 0.03);
        assert!(PenState::of(d.pen()).contact);
        assert!((PenState::of(d.pen()).tip_level - 1.0).abs() < 1e-9);

        // A reference far above the signal ends the contact; back to learning forgets it.
        set_params(&mut config, &[(Param::ZeroForceReference, Value::Number(energy * 4.0))]).unwrap();
        d.set_config(&config).unwrap();
        sink.samples.clear();
        frames(&mut d, &mut sink, 3);
        assert!(!sink.samples.last().unwrap().contact);
        set_params(&mut config, &[(Param::ZeroForceReference, Value::Number(0.0))]).unwrap();
        d.set_config(&config).unwrap();
        assert_eq!(d.pen().zero_force_reference(), 0.0);

        // What decides about the devices does not change this way.
        config.pen.axis_resolution = 50;
        assert!(d.set_config(&config).is_err());
        assert_eq!(d.config().pen.axis_resolution, 100);
    }

    #[test]
    fn release_lifts_everything() {
        let mut d = Digitizer::new(&Config::default(), &Geometry::SP9);
        let mut sink = Packets::default();
        d.process(&finger_report(30.0, 20.0), 0.00, &mut sink).unwrap();
        d.process(&finger_report(30.0, 20.0), 0.01, &mut sink).unwrap();
        sink.packets.clear();
        d.release(&mut sink);
        assert_eq!(sink.packets, vec!["T 3:47:0 3:47:0 3:57:-1 1:330:0"]);

        d.process(&pen_frame(1, 40.0, 20.0, true, 520, false), 1.0, &mut sink).unwrap();
        d.process(&pen_frame(2, 40.0, 20.0, true, 520, false), 1.015, &mut sink).unwrap();
        assert!(sink.packets.last().unwrap().starts_with("S 1:330:1 1:320:1"));
        sink.packets.clear();
        d.release(&mut sink);
        assert_eq!(sink.packets, vec!["S 1:330:0 1:320:0 1:321:0 1:331:0", "T 1:330:0"]);
        // Nothing is left of the stream: the pen comes into range anew.
        assert!(!d.pen().sample().proximity);
        sink.packets.clear();
        d.release(&mut sink);
        assert_eq!(sink.packets, vec!["T 1:330:0"]);
    }

    #[test]
    fn an_error_in_a_report_keeps_what_came_before() {
        let mut d = Digitizer::new(&Config::default(), &Geometry::SP9);
        let mut sink = Packets::default();
        let mut report = pen_frame(1, 30.0, 20.0, false, 0, false);
        // Cut into the last window.
        report.truncate(report.len() - 10);
        let len = (report.len() - 3) as u32;
        report[3..7].copy_from_slice(&len.to_le_bytes());
        assert!(d.process(&report, 0.0, &mut sink).is_err());
        assert!(!sink.samples.is_empty());
    }
}
