// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
// Derived from iptsd (src/core/generic/dft.hpp) with the pen patches of this repository,
// GPL-2.0-or-later, Copyright (C) the iptsd authors.
//! Stage "pressure": the force the pen measures, and the pressure that is reported with
//! every position sample.
//!
//! The pen sends its force once per frame (the pressure window), twice over: as a tone whose
//! frequency steps through 256 levels, and as six bits next to it (the lower four bits of that
//! level and two more below them, ten bits in total). Two samples are reported per frame, and
//! a contact can begin and go on without any force, so the reported pressure is shaped from
//! the measurements: see [`PressureStage::shape`].

use super::signal;
use crate::config::PressureConfig;
use crate::frame::{DftWindow, NUM_COMPONENTS, PRESSURE_ROWS};
use crate::num;
use std::f64::consts::FRAC_PI_2;

/// Rows a pressure window needs for the digital bits.
const DECODE_ROWS: usize = 16;
/// The pen derives the pressure tone from a clock that it divides by this plus the level.
const TONE_CLOCK: f64 = 22054884.0;
const TONE_DIVIDER: f64 = 945.0;

/// How many stroke starts are remembered for the pressure of a contact without force, and
/// for how many pressure windows.
const STROKE_STARTS: usize = 5;
const STROKE_START_AGE: u64 = 330;
/// The zero force pressure relative to the pressure the last strokes began with.
const ZERO_FORCE_START_PART: f64 = 0.85;
/// The zero force pressure where a stroke loses its force, relative to the last force.
const ZERO_FORCE_FADE_PART: f64 = 0.8;
/// Limits of the zero force pressure.
const ZERO_FORCE_MIN: f64 = 37.0 / 1024.0;
const ZERO_FORCE_MAX: f64 = 450.0 / 1024.0;
/// By how much the scale of the zero force pressure changes per contact at most, and how
/// many such steps it may be away from 1.
const ADAPT_STEP: f64 = 1.25;
const ADAPT_STEPS: f64 = 5.0;
/// Samples over which a step between measured and zero force pressure is spread.
const RAMP_UP: u32 = 2;
const RAMP_DOWN: u32 = 25;
/// The smallest pressure during a ramp: one unit of the output axis.
const RAMP_MIN: f64 = 1.0 / 4096.0;

/// Where the pressure of the last reported sample came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Source {
    NoContact,
    Force,
    ZeroForce,
}

#[derive(Clone, Debug)]
pub struct PressureStage {
    config: PressureConfig,

    /// The pressure of the last pressure window and of the one before (0 without force), and
    /// whether a sample was reported since.
    measured: f64,
    previous: f64,
    reported: bool,

    source: Source,
    output: f64,
    /// The last pressure with force during this contact.
    last_force: f64,

    /// The pressure of a contact without force, and whether it was taken from the force
    /// that preceded it.
    zero_force: f64,
    zero_force_fixed: bool,

    /// The ramp between the two kinds of pressure: the step, the samples left, the length.
    gap: f64,
    ramp: u32,
    ramp_length: u32,

    /// With what pressure the last strokes began and in which pressure window, and the scale
    /// of the zero force pressure derived from it.
    stroke_starts: Vec<(f64, u64)>,
    scale: f64,

    /// A counter of pressure windows and the pressures of the two windows before the last
    /// one (negative: unknown).
    window: u64,
    history: [f64; 2],
}

impl PressureStage {
    pub fn new(config: &PressureConfig) -> PressureStage {
        PressureStage {
            config: config.clone(),
            measured: 0.0,
            previous: 0.0,
            reported: true,
            source: Source::NoContact,
            output: 0.0,
            last_force: 0.0,
            zero_force: 0.0,
            zero_force_fixed: false,
            gap: 0.0,
            ramp: 0,
            ramp_length: 0,
            stroke_starts: Vec::with_capacity(STROKE_STARTS),
            scale: 1.0,
            window: 0,
            history: [-1.0, -1.0],
        }
    }

    pub fn set_config(&mut self, config: &PressureConfig) {
        self.config = config.clone();
    }

    /// The pressure of the last pressure window, 0 without force.
    pub fn measured(&self) -> f64 {
        self.measured
    }

    pub fn floor(&self) -> f64 {
        self.config.floor
    }

    /// Takes a pressure window. Returns the measured pressure (0 without force), or `None`
    /// for a window that is too short to be one. `force_tone`: the force tone of the position
    /// windows says that the tip is pressed right now.
    pub fn pressure_window(&mut self, w: &DftWindow<'_>, eraser: bool, force_tone: bool) -> Option<f64> {
        if w.x.len() < PRESSURE_ROWS {
            return None;
        }
        let p = measure(&self.config, w, eraser, force_tone);
        let pressure = if p > 0.0 { num::clamp(p, 0.0, 1.0) } else { 0.0 };

        // Two windows with force after one without: with this pressure a stroke begins.
        if self.history[0] == 0.0 && self.history[1] > 0.0 && pressure > 0.0 {
            if self.stroke_starts.len() == STROKE_STARTS {
                self.stroke_starts.remove(0);
            }
            self.stroke_starts.push(((self.history[1] + pressure) / 2.0, self.window));
        }
        self.history = [self.history[1], pressure];

        self.previous = self.measured;
        self.measured = pressure;
        self.reported = false;
        self.window += 1;
        Some(pressure)
    }

    /// The pressure of the sample that is about to be reported.
    ///
    /// With force it is the measurement; the first of the two samples after a pressure window
    /// (`first_after_window`) is half way from the measurement before, or 0.9 of the
    /// measurement at the start of a contact.
    ///
    /// Without force the contact is either the beginning of a stroke (the force tone is up to
    /// a frame ahead of the pressure window) or light writing below the activation force. Its
    /// pressure is a default that falls with the tilt (`tilt`: radians from the vertical, if
    /// known), scaled towards 0.85 of the pressure the last strokes began with; where a
    /// stroke loses its force it goes on at 0.8 of the last force.
    ///
    /// A change between the two kinds is spread over 2 samples upwards and 25 downwards. The
    /// configured floor is the lower bound of everything.
    pub fn shape(&mut self, contact: bool, first_after_window: bool, tilt: Option<f64>) -> f64 {
        let fresh = !self.reported;
        self.reported = true;

        if !contact {
            self.source = Source::NoContact;
            self.output = 0.0;
            self.last_force = 0.0;
            self.ramp = 0;
            self.zero_force = 0.0;
            self.zero_force_fixed = false;
            return 0.0;
        }

        let floor = self.config.floor;
        let before = self.source;
        let mut p;

        if self.measured > 0.0 {
            self.source = Source::Force;
            p = self.measured;
            if fresh && first_after_window {
                if before == Source::Force && self.previous > 0.0 {
                    p = (self.previous + self.measured) / 2.0;
                } else if before == Source::NoContact {
                    p = 0.9 * self.measured;
                }
            }
            self.last_force = self.measured;
        } else {
            self.source = Source::ZeroForce;
            if before == Source::Force {
                self.zero_force_fixed = true;
                self.zero_force = ZERO_FORCE_FADE_PART * self.last_force;
            } else if !self.zero_force_fixed {
                let base = default_zero_force(tilt);
                // Once per contact.
                if self.zero_force <= 0.0 {
                    self.adapt(base);
                }
                self.zero_force = base * self.scale;
            }
            self.zero_force = num::clamp(self.zero_force, ZERO_FORCE_MIN, ZERO_FORCE_MAX);
            p = self.zero_force;
        }

        if before == Source::NoContact {
            self.ramp = 0;
        } else if before != self.source {
            self.gap = p - self.output;
            self.ramp_length = if self.gap >= 0.0 { RAMP_UP } else { RAMP_DOWN };
            self.ramp = self.ramp_length;
        }

        if self.ramp > 0 {
            self.ramp -= 1;
            p -= self.gap * f64::from(self.ramp) / f64::from(self.ramp_length);
            p = num::max(p, RAMP_MIN);
        }

        self.output = p;
        num::clamp(num::max(p, floor), 0.0, 1.0)
    }

    /// Moves the scale of the zero force pressure towards 0.85 of the pressure the recent
    /// strokes began with, by at most one step.
    fn adapt(&mut self, base: f64) {
        let mut sum = 0.0;
        let mut count = 0u32;
        for &(pressure, window) in &self.stroke_starts {
            if window + STROKE_START_AGE <= self.window {
                continue;
            }
            sum += pressure;
            count += 1;
        }

        let target = if count >= 2 { ZERO_FORCE_START_PART * sum / f64::from(count) } else { base };
        let current = base * self.scale;

        if target > ADAPT_STEP * current {
            self.scale *= ADAPT_STEP;
        } else if target < current / ADAPT_STEP {
            self.scale /= ADAPT_STEP;
        } else {
            self.scale = target / base;
        }

        let limit = ADAPT_STEP.powf(ADAPT_STEPS);
        self.scale = num::clamp(self.scale, 1.0 / limit, limit);
    }

    /// The pen left. The stroke starts and the scale stay.
    pub fn reset(&mut self) {
        self.measured = 0.0;
        self.previous = 0.0;
        self.source = Source::NoContact;
        self.output = 0.0;
        self.last_force = 0.0;
        self.ramp = 0;
        self.zero_force = 0.0;
        self.zero_force_fixed = false;
    }
}

/// The zero force pressure before it is adapted: about a tenth of the range for an upright
/// pen, falling linearly to a fifth of that for a pen that lies flat; half way if the tilt
/// is not known.
fn default_zero_force(tilt: Option<f64>) -> f64 {
    let tilt = num::clamp(tilt.unwrap_or(std::f64::consts::FRAC_PI_4), 0.0, FRAC_PI_2);
    (400.0 - (324.0 * tilt / FRAC_PI_2)) / 4096.0
}

/// The position of the pressure tone among the first rows of the window, 0 .. 1 from the
/// first to the last of them, by Jacobsen's quadratic estimator on the summed antennas.
///
/// The sums and products are 32 bit integers and wrap around for very strong tones.
fn tone_position(config: &PressureConfig, w: &DftWindow<'_>) -> f64 {
    let rows = PRESSURE_ROWS;

    let mut strongest = 0usize;
    let mut magnitude = 0u64;
    for i in 0..rows {
        let m = u64::from(w.x[i].magnitude.wrapping_add(w.y[i].magnitude));
        if m > magnitude {
            magnitude = m;
            strongest = i;
        }
    }
    if magnitude < 2 * u64::from(config.tone_min_magnitude) {
        return f64::NAN;
    }

    let mut lowest = -0.5;
    let mut highest = 0.5;
    if strongest < 1 {
        strongest = 1;
        lowest = -1.0;
    } else if strongest > rows - 2 {
        strongest = rows - 2;
        highest = 1.0;
    }

    // All antennas of a row, and the same row on the other axis, have the same phase.
    let mut real = [0i32; 3];
    let mut imag = [0i32; 3];
    for i in 0..3 {
        let x = &w.x[strongest + i - 1];
        let y = &w.y[strongest + i - 1];
        for j in 0..NUM_COMPONENTS {
            real[i] += i32::from(x.real[j]) + i32::from(y.real[j]);
            imag[i] += i32::from(x.imag[j]) + i32::from(y.imag[j]);
        }
    }

    let ra = real[0].wrapping_sub(real[2]);
    let rb = real[1].wrapping_mul(2).wrapping_sub(real[0]).wrapping_sub(real[2]);
    let ia = imag[0].wrapping_sub(imag[2]);
    let ib = imag[1].wrapping_mul(2).wrapping_sub(imag[0]).wrapping_sub(imag[2]);

    let numerator = ra.wrapping_mul(rb).wrapping_add(ia.wrapping_mul(ib));
    let denominator = rb.wrapping_mul(rb).wrapping_add(ib.wrapping_mul(ib));
    let d = f64::from(numerator) / f64::from(denominator);

    (strongest as f64 + num::clamp(d, lowest, highest)) / (rows - 1) as f64
}

/// The level (0 .. 255, not rounded) the frequency of the pressure tone stands for, or `None`
/// if the window carries no usable frequencies.
fn analog_level(w: &DftWindow<'_>, position: f64) -> Option<f64> {
    // Every row carries its frequency; the tone is between those of the first and the last row.
    let first = f64::from(f32::from_bits(w.x[0].frequency));
    let last = f64::from(f32::from_bits(w.x[PRESSURE_ROWS - 1].frequency));
    let frequency = first + (position * (last - first));
    (frequency > 0.0).then(|| (TONE_CLOCK / frequency) - TONE_DIVIDER)
}

/// The pressure of a pressure window: 0 .. 1, 0 or less (or NaN) without force.
///
/// The tone alone reads about twice the true value at light force (the lowest level is not
/// at the row of the lowest frequency) but is within 7 levels of it, which is what the four
/// digital bits need to correct it. All bits zero is either no force (the tone outlasts the
/// force by a frame) or a force whose bits happen to be zero (one level in 16, placed by the
/// tone like any other); `force_tone` tells the two apart. The eraser end reports the
/// position of its tone among the tone rows, which only says that its switch is closed: the
/// tone does not move with the force.
fn measure(config: &PressureConfig, w: &DftWindow<'_>, eraser: bool, force_tone: bool) -> f64 {
    let position = tone_position(config, w);

    if eraser {
        // The eraser end has a switch: its tone is in the row after the tone rows while the
        // switch is open and jumps into them when it closes. What the open tone leaks into
        // the last tone row is not a force.
        if w.x.len() > PRESSURE_ROWS && w.y.len() > PRESSURE_ROWS {
            let open = signal::pair_magnitude(w, PRESSURE_ROWS);
            if (0..PRESSURE_ROWS).all(|row| signal::pair_magnitude(w, row) < open) {
                return 0.0;
            }
        }
        return 1.0 - position;
    }
    if w.x.len() < DECODE_ROWS || w.y.len() < DECODE_ROWS {
        return 1.0 - position;
    }

    let magnitude = |row: usize| f64::from(w.x[row].magnitude) + f64::from(w.y[row].magnitude);

    // Three bits as a tone in one of two rows each, three as a tone that is on or off.
    let mut bits = [false; 6];
    let mut strength = 0.0;
    for (i, bit) in bits.iter_mut().enumerate().take(3) {
        let first = magnitude(7 + 2 * i);
        let second = magnitude(8 + 2 * i);
        let high = num::max(first, second);
        let low = num::min(first, second);

        // Without a clean tone in each pair the pen is too far or the window is disturbed.
        if high <= f64::from(config.bit_min_magnitude) || high <= 2.0 * low {
            return 0.0;
        }
        *bit = second > first;
        strength += high / 3.0;
    }
    for i in 0..3 {
        bits[3 + i] = magnitude(13 + i) > strength / 4.0;
    }

    let nibble = (u32::from(bits[0]) << 3) | (u32::from(bits[1]) << 2) | (u32::from(bits[2]) << 1) | u32::from(bits[3]);
    if position.is_nan() {
        return 0.0;
    }
    let Some(analog) = analog_level(w, position) else { return 0.0 };

    let zero_code = nibble == 0 && !bits[4] && !bits[5];
    if zero_code && !force_tone {
        // No force. A force whose lower six bits are zero (one level in 16) looks the same
        // in this window, and the tone cannot tell either: it outlasts the force by a
        // frame. The force tone of the position windows can.
        return 0.0;
    }

    let mut level = num::clamp((analog + 0.5).floor(), 1.0, 256.0) as i32;
    if level == 256 {
        if nibble == 15 && bits[4] && bits[5] {
            return 1.0;
        }
        level = 255;
    }

    // The level closest to the analog one whose lower four bits are the digital ones.
    let mut combined = (level & !15) | nibble as i32;
    if combined - level >= 8 {
        combined -= 16;
    } else if combined - level < -7 {
        combined += 16;
    }
    if combined < 0 {
        combined += 16;
    }
    if combined > 255 {
        combined -= 16;
    }

    let value = (combined * 4) + (i32::from(bits[4]) * 2) + i32::from(bits[5]);
    f64::from(value) / 1024.0
}

#[cfg(test)]
pub(crate) mod testwindow {
    use crate::frame::Row;

    /// The rows of a pressure window (same on both axes) for a 10 bit force value. The tone
    /// sits exactly on the third of the six tone rows, whose frequencies are chosen to match.
    pub fn rows(value: u32) -> Vec<Row> {
        let level = value >> 2;
        let tone = 22054884.0 / (945.0 + f64::from(level));
        let step = 200.0;

        let mut rows = vec![Row::default(); 16];
        for (i, row) in rows.iter_mut().enumerate().take(6) {
            row.frequency = ((tone + (i as f64 - 2.0) * step) as f32).to_bits();
        }
        rows[2].real = [1000; 9];
        rows[2].magnitude = 9_000_000;
        if value == 0 {
            // No force: the tone may still be there, all bits are zero.
            for pair in 0..3 {
                rows[7 + 2 * pair].magnitude = 100_000;
            }
            return rows;
        }
        for pair in 0..3 {
            let set = (level >> (3 - pair)) & 1 == 1;
            rows[7 + 2 * pair + usize::from(set)].magnitude = 100_000;
        }
        for (i, bit) in [(13, (level & 1) == 1), (14, (value & 2) != 0), (15, (value & 1) != 0)] {
            rows[i].magnitude = if bit { 100_000 } else { 10 };
        }
        rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(rows: &[crate::frame::Row]) -> DftWindow<'_> {
        DftWindow { group: Some(1), kind: crate::frame::window::PRESSURE, timestamp: 0, columns: 68, rows: 46, x: rows, y: rows }
    }

    #[test]
    fn decodes_every_level() {
        let config = PressureConfig::default();
        for value in [1u32, 2, 3, 4, 37, 100, 205, 511, 513, 800, 1019] {
            let rows = testwindow::rows(value);
            assert_eq!(measure(&config, &window(&rows), false, false), f64::from(value) / 1024.0, "value {value}");
        }
        assert_eq!(measure(&config, &window(&testwindow::rows(0)), false, false), 0.0);
        assert_eq!(measure(&config, &window(&testwindow::rows(0)), false, true), 0.0);
    }

    #[test]
    fn a_force_with_all_bits_zero_needs_the_force_tone() {
        let config = PressureConfig::default();
        let rows = testwindow::rows(512);
        // The tone alone may be the one that outlasts a force.
        assert_eq!(measure(&config, &window(&rows), false, false), 0.0);
        assert_eq!(measure(&config, &window(&rows), false, true), 0.5);
    }

    #[test]
    fn the_eraser_keeps_the_interpolated_tone() {
        let config = PressureConfig::default();
        let rows = testwindow::rows(400);
        // The tone is on the third of six rows.
        assert!((measure(&config, &window(&rows), true, true) - 0.6).abs() < 1e-12);
        // The tone in the row after the tone rows: the switch is open.
        let mut open = vec![crate::frame::Row::default(); 16];
        open[6].magnitude = 10_000_000;
        open[5].magnitude = 30_000;
        open[5].real = [50; 9];
        assert_eq!(measure(&config, &window(&open), true, false), 0.0);
        // A window without a tone has no pressure, also for the eraser.
        let silent = vec![crate::frame::Row::default(); 16];
        assert!(measure(&config, &window(&silent), true, false).is_nan());
        assert_eq!(measure(&config, &window(&silent), false, true), 0.0);
    }

    fn feed(stage: &mut PressureStage, value: u32) {
        stage.pressure_window(&window(&testwindow::rows(value)), false, true).unwrap();
    }

    #[test]
    fn half_steps_between_the_two_samples_of_a_frame() {
        let mut s = PressureStage::new(&PressureConfig::default());
        feed(&mut s, 400);
        // Start of the contact: 0.9 of the measurement, then the measurement.
        assert_eq!(s.shape(true, true, None), 0.9 * 400.0 / 1024.0);
        assert_eq!(s.shape(true, false, None), 400.0 / 1024.0);
        feed(&mut s, 600);
        assert_eq!(s.shape(true, true, None), 500.0 / 1024.0);
        assert_eq!(s.shape(true, false, None), 600.0 / 1024.0);
        // A second sample without a new window in between is not a half step.
        assert_eq!(s.shape(true, true, None), 600.0 / 1024.0);
        assert_eq!(s.shape(false, false, None), 0.0);
    }

    #[test]
    fn zero_force_pressure_and_ramps() {
        let mut s = PressureStage::new(&PressureConfig { floor: 0.03, ..PressureConfig::default() });
        // Contact without any force: the default for an upright pen, scaled by nothing yet.
        feed(&mut s, 0);
        assert_eq!(s.shape(true, false, Some(0.0)), 400.0 / 4096.0);
        // Force arrives: two samples up to the measurement.
        feed(&mut s, 520);
        let a = s.shape(true, true, Some(0.0));
        let b = s.shape(true, false, Some(0.0));
        assert!(a > 400.0 / 4096.0 && a < b);
        assert_eq!(b, 520.0 / 1024.0);
        // Force is lost: 25 samples down to 0.8 of the last force.
        feed(&mut s, 0);
        let mut last = b;
        for i in 0..25 {
            let p = s.shape(true, false, Some(0.0));
            assert!(p < last, "sample {i}");
            last = p;
        }
        assert!((last - 0.8 * 520.0 / 1024.0).abs() < 1e-12);
        assert_eq!(s.shape(true, false, Some(0.0)), 0.8 * 520.0 / 1024.0);
    }

    #[test]
    fn zero_force_pressure_follows_the_stroke_starts() {
        let mut s = PressureStage::new(&PressureConfig::default());
        // Two strokes that begin at 0.3.
        for _ in 0..2 {
            feed(&mut s, 0);
            s.shape(false, false, None);
            for _ in 0..3 {
                feed(&mut s, 307);
                s.shape(true, false, None);
            }
        }
        feed(&mut s, 0);
        s.shape(false, false, None);
        let base = 400.0 / 4096.0;
        // The target is 0.85 * 0.3 = 2.6 times the default: one step of 1.25 per contact.
        assert_eq!(s.shape(true, false, Some(0.0)), base * 1.25);
        s.shape(false, false, None);
        assert_eq!(s.shape(true, false, Some(0.0)), base * 1.25 * 1.25);
        // A flat pen has a fifth of the default.
        s.shape(false, false, None);
        let flat = s.shape(true, false, Some(FRAC_PI_2));
        assert!((flat - (76.0 / 4096.0) * 1.25f64.powi(3)).abs() < 1e-12);
    }
}
