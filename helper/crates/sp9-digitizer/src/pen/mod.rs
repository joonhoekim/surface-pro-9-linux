// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
// Derived from iptsd (src/core/generic/dft.hpp) with the pen patches of this repository,
// GPL-2.0-or-later, Copyright (C) the iptsd authors.
//! The pen pipeline: DFT windows in, pen samples out.
//!
//! A frame of the pen (15 ms) is a group of windows, each sampled at its own time:
//!
//! | window | carries | handled by |
//! |---|---|---|
//! | position (0x06) | weak tip tone, force tone | proximity, [`contact`] |
//! | MPP v2 position (0x07, 0x08) | strong tip tone, ring tone, force tone | [`position`], [`tilt`], [`contact`], one sample each |
//! | button (0x09) | button / eraser tone | [`button`] |
//! | binary (0x0A, twice) | button bit | [`button`] |
//! | pressure (0x0B) | pressure tone and bits | [`pressure`], [`contact`] |
//!
//! The position of a sample goes through [`lock`] and, if a horizon is configured, through
//! [`predict`] last.
//!
//! Every stage owns its state and its part of the configuration; [`signal`] holds the
//! stateless measurements on window rows. [`Pen::input`] is the only place that knows the order
//! in which the stages run and what they hand to each other, and [`Pen::lift`] the only place
//! that knows what a pen leaving means. A sample is reported for each of the two MPP v2
//! position windows (133 per second); the position window decides about proximity and reports
//! only when the frame before had no such sample.

pub mod button;
pub mod contact;
pub mod lock;
pub mod position;
pub mod predict;
pub mod pressure;
pub mod signal;
pub mod tilt;

use crate::config::{Geometry, PenConfig};
use crate::frame::{window, DftWindow};
use crate::num;

/// The tone of the position window may fall to this part of the magnitude that brings a pen
/// into range before a pen that is in range leaves.
const EXIT_MAGNITUDE_DIVISOR: u32 = 2;

/// The state of the pen as it is reported.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PenSample {
    /// The pen is in range. All other fields are meaningless (stale) when it is not.
    pub proximity: bool,
    /// The tip (or the eraser) is on the glass.
    pub contact: bool,
    /// The barrel button is pressed.
    pub button: bool,
    /// The eraser end is in use.
    pub eraser: bool,
    /// Position as a fraction of the screen, 0 .. 1.
    pub x: f64,
    pub y: f64,
    /// 0 .. 1.
    pub pressure: f64,
    /// Tilt from the vertical in radians, 0 while it is not known.
    pub altitude: f64,
    /// Direction of the tilt in radians.
    pub azimuth: f64,
}

#[derive(Clone, Debug)]
pub struct Pen {
    config: PenConfig,
    geometry: Geometry,
    position: position::PositionStage,

    tilt: tilt::TiltStage,
    contact: contact::ContactStage,
    pressure: pressure::PressureStage,
    lock: lock::LockStage,
    predict: predict::PredictStage,
    button: button::ButtonStage,

    sample: PenSample,
    /// The frame in which the position window found the pen, and the phase of its tip tone.
    group: Option<u32>,
    tip_phase: (i32, i32),
    /// Samples the MPP v2 position windows reported since the last position window.
    fine_samples: u32,
    /// The pen came into range with the last position window and nothing was reported yet:
    /// which end it is shows later in the frame.
    tool_pending: bool,
}

impl Pen {
    pub fn new(config: &PenConfig, geometry: &Geometry) -> Pen {
        Pen {
            config: config.clone(),
            geometry: *geometry,
            position: position::PositionStage::new(&config.position, geometry),
            tilt: tilt::TiltStage::new(&config.tilt, geometry),
            contact: contact::ContactStage::new(&config.contact),
            pressure: pressure::PressureStage::new(&config.pressure),
            lock: lock::LockStage::new(&config.lock),
            predict: predict::PredictStage::new(config.prediction_ms),
            button: button::ButtonStage::new(&config.button),
            sample: PenSample::default(),
            group: None,
            tip_phase: (0, 0),
            fine_samples: 0,
            tool_pending: false,
        }
    }

    /// Replaces the configuration without disturbing a stroke: the state of every stage stays.
    pub fn set_config(&mut self, config: &PenConfig) {
        self.config = config.clone();
        self.position = position::PositionStage::new(&config.position, &self.geometry);
        self.tilt.set_config(&config.tilt);
        self.contact.set_config(&config.contact);
        self.pressure.set_config(&config.pressure);
        self.lock.set_config(&config.lock);
        self.predict.set_horizon(config.prediction_ms);
        self.button.set_config(&config.button);
    }

    pub fn config(&self) -> &PenConfig {
        &self.config
    }

    /// The current state, as of the last window.
    pub fn sample(&self) -> &PenSample {
        &self.sample
    }

    /// The tip signal of the last MPP v2 position window.
    pub fn tip_signal(&self) -> &contact::TipSignal {
        self.contact.tip()
    }

    /// The tip energy that counts as contact without force (configured or learned), 0 if unknown.
    pub fn zero_force_reference(&self) -> f64 {
        self.contact.reference()
    }

    /// Takes a window. Returns the sample to report, if this window completes one.
    pub fn input(&mut self, w: &DftWindow<'_>) -> Option<PenSample> {
        match w.kind {
            window::POSITION => self.position_window(w).then_some(self.sample),
            window::POSITION_MPP2 | window::POSITION_MPP2_SECOND => {
                self.contact.measure(w);
                self.update_contact();
                self.fine_position(w).then_some(self.sample)
            }
            window::BUTTON => {
                // The phase of the button tone means something only relative to the tip tone
                // of the same frame.
                if !w.x.is_empty() && self.group.is_some() && self.group == w.group {
                    // In the frame in which the pen came into range, a tone here that is as
                    // strong as the one of the position window is the eraser end: it sends
                    // the same tone in both windows, the tip sends none in this one.
                    let eraser_tone =
                        self.tool_pending && 2 * u64::from(signal::pair_magnitude(w, 0)) >= u64::from(self.contact.position_tone());
                    let buttons = self.button.button_window(w, self.tip_phase, eraser_tone);
                    self.sample.button = buttons.barrel;
                    self.sample.eraser = buttons.eraser;
                }
                None
            }
            window::BINARY_MPP2 => {
                self.button.binary_window(w);
                None
            }
            window::PRESSURE => {
                if let Some(pressure) = self.pressure.pressure_window(w, self.sample.eraser, self.contact.force_steady()) {
                    self.contact.pressure_window(pressure);
                }
                None
            }
            _ => None,
        }
    }

    /// The position window: is there a pen, and where roughly. Returns whether to report.
    fn position_window(&mut self, w: &DftWindow<'_>) -> bool {
        let entering = !self.sample.proximity;
        let before = (self.sample.x, self.sample.y);
        let had_fine_samples = self.fine_samples > 0;
        self.fine_samples = 0;

        let min = self.config.position.min_magnitude;
        let found = (w.x.len() > 1 && w.x[0].magnitude > min && w.y[0].magnitude > min)
            .then(|| {
                self.group = w.group;
                self.tip_phase = signal::center_phase(w);
                self.position.coarse(&w.x[0], &w.y[0])
            })
            .flatten();

        let Some((x, y)) = found else {
            // A pen that is in range does not leave because of one weak position window:
            // while the stronger MPP v2 windows of the frame before located it and the tone
            // of this window is still there, the frame goes on without its position.
            let exit = min / EXIT_MAGNITUDE_DIVISOR;
            if self.sample.proximity && had_fine_samples && w.x.len() > 1 && w.x[0].magnitude > exit && w.y[0].magnitude > exit {
                self.group = w.group;
                self.tip_phase = signal::center_phase(w);
                self.contact.position_window(w);
                return false;
            }
            self.lift();
            return true;
        };

        self.sample.proximity = true;
        self.sample.x = num::clamp(self.position.mapping().x(x), 0.0, 1.0);
        self.sample.y = num::clamp(self.position.mapping().y(y), 0.0, 1.0);

        self.contact.position_window(w);

        // While the MPP v2 position windows deliver the position, the one of this window is
        // left out: its tone is much weaker and, with a tilted pen, off by more than the pen
        // moved. Mixing the two makes the path zigzag.
        if had_fine_samples {
            (self.sample.x, self.sample.y) = before;
            return false;
        }

        (self.sample.x, self.sample.y) = self.finish_position(self.sample.x, self.sample.y, 0.0, 0.0);
        self.sample.pressure = self.pressure.shape(self.sample.contact, false, self.tilt_for_pressure());
        // A position of this window is not part of the stream the prediction follows.
        self.predict.reset();

        // A pen that comes into range is not reported before it is known which end it is:
        // a client may take the tool it sees first. The tip shows in the MPP v2 position
        // window 3 ms later (and reports there), the eraser in the button window; whatever
        // it is, the next position window reports.
        self.tool_pending = entering;
        !entering
    }

    /// Contact from the tip signal that [`contact::ContactStage::measure`] just read.
    fn update_contact(&mut self) {
        if !self.sample.proximity {
            return;
        }

        let mut contact = self.contact.decide(self.sample.eraser);

        // The eraser end has a switch and no force tone: it is in contact while the
        // pressure window shows the switch closed, or while it rests on the glass.
        if self.sample.eraser {
            contact = self.pressure.measured() > 0.0 || self.contact.eraser_touch();
        }
        self.sample.contact = contact;

        // The pressure of this sample is set when it is reported; a sample that is not (no
        // position in this window) keeps the measurement, and the end of a contact reaches
        // the pressure stage at once.
        self.sample.pressure =
            if contact { num::max(self.pressure.measured(), self.pressure.floor()) } else { self.pressure.shape(false, false, None) };
    }

    /// The position from an MPP v2 position window. Returns whether to report.
    fn fine_position(&mut self, w: &DftWindow<'_>) -> bool {
        // Proximity is not decided here: without a position window that found the pen in the
        // same frame nothing is reported.
        if !self.sample.proximity || self.group.is_none() || self.group != w.group {
            return false;
        }
        if w.x.is_empty() || w.y.is_empty() {
            return false;
        }
        let min = self.config.position.min_magnitude;
        if w.x[0].magnitude <= min || w.y[0].magnitude <= min {
            return false;
        }
        // The tip tone is about 15 times stronger here than in the position window. What
        // is weaker than there is not the tip: the eraser end sends nothing in these
        // windows, and what is left of other tones must not pass for a position.
        if signal::pair_magnitude(w, 0) <= self.contact.position_tone() {
            return false;
        }
        // The size of the sensor: from the heatmap dimensions once an image came by, from
        // the device metadata before.
        let (columns, rows) = if w.columns == 0 || w.rows == 0 { (self.geometry.columns, self.geometry.rows) } else { (w.columns, w.rows) };
        if columns < 2 || rows < 2 {
            return false;
        }

        let Some(tip) = self.position.fine(&w.x[0], &w.y[0]) else {
            return false;
        };

        self.tilt.update(w, &tip, self.position.mapping());
        (self.sample.altitude, self.sample.azimuth) = self.tilt.angles().unwrap_or((0.0, 0.0));

        // The offset of the tip is applied in screen coordinates.
        let (ox, oy) = match self.tilt.offset() {
            Some((ox, oy)) => (
                self.position.mapping().x(tip.x + ox) - self.position.mapping().x(tip.x),
                self.position.mapping().y(tip.y + oy) - self.position.mapping().y(tip.y),
            ),
            None => (0.0, 0.0),
        };

        let (x, y) = self.finish_position(self.position.mapping().x(tip.x), self.position.mapping().y(tip.y), ox, oy);
        self.sample.x = num::clamp(x, 0.0, 1.0);
        self.sample.y = num::clamp(y, 0.0, 1.0);

        // Of the two samples of a frame, the second window follows the pressure window.
        let first_after_window = w.kind == window::POSITION_MPP2_SECOND;
        self.sample.pressure = self.shape_pressure(first_after_window);

        // Last: where the tip will be a moment from now, if that is asked for.
        let (w_mm, h_mm) = (self.position.mapping().width_mm(), self.position.mapping().height_mm());
        let context = predict::Context {
            contact: self.sample.contact,
            pressure: self.sample.pressure,
            holding: self.lock.holding(),
            force_steady: self.contact.force_steady(),
        };
        let (px, py) = self.predict.apply(w.timestamp, self.sample.x * w_mm, self.sample.y * h_mm, &context);
        if (px, py) != (self.sample.x * w_mm, self.sample.y * h_mm) {
            self.sample.x = num::clamp(px / w_mm, 0.0, 1.0);
            self.sample.y = num::clamp(py / h_mm, 0.0, 1.0);
        }

        self.fine_samples += 1;
        self.tool_pending = false;
        true
    }

    fn shape_pressure(&mut self, first_after_window: bool) -> f64 {
        self.pressure.shape(self.sample.contact, first_after_window, self.tilt_for_pressure())
    }

    fn tilt_for_pressure(&self) -> Option<f64> {
        self.tilt.angles().map(|(altitude, _)| altitude)
    }

    /// Adds the tilt offset to a position (fractions of the screen) and passes it through
    /// the lock, which works in millimeters.
    fn finish_position(&mut self, x: f64, y: f64, ox: f64, oy: f64) -> (f64, f64) {
        let w = self.position.mapping().width_mm();
        let h = self.position.mapping().height_mm();
        if !self.lock.enabled() || w <= 0.0 || h <= 0.0 {
            return (x + ox, y + oy);
        }
        let mut fx = x * w;
        let mut fy = y * h;
        fx += ox * w;
        fy += oy * h;
        let (fx, fy) = self.lock.apply(fx, fy, self.sample.contact);
        (fx / w, fy / h)
    }

    /// The pen left the range of the sensor.
    fn lift(&mut self) {
        self.sample.proximity = false;
        self.sample.contact = false;
        self.sample.button = false;
        self.sample.eraser = false;
        self.sample.altitude = 0.0;
        self.sample.azimuth = 0.0;
        self.tool_pending = false;

        self.button.reset();
        self.contact.reset();
        self.pressure.reset();
        self.tilt.reset();
        self.lock.reset();
        self.predict.reset();
    }
}
