// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
// Derived from iptsd (src/core/generic/dft.hpp) with the pen patches of this repository,
// GPL-2.0-or-later, Copyright (C) the iptsd authors.
//! Stage "contact": whether the tip is on the glass.
//!
//! The pen says so itself with a tone that switches rows while its tip switch is closed (the
//! force tone, in every position window). Below the activation force of the pen that tone
//! stays off although the tip rests on the glass; then the strength of the tip signal
//! decides, which saturates at contact (zero force contact).

use super::signal;
use crate::config::{ContactConfig, ZeroForceConfig};
use crate::frame::DftWindow;
use crate::num;

/// Rows of the force tone in the position window (0x06): tip switch open / closed.
const POSITION_ROW_OPEN: usize = 2;
const POSITION_ROW_CLOSED: usize = 1;
/// The same in the MPP v2 position windows (0x07, 0x08).
const MPP2_ROW_OPEN: usize = 2;
const MPP2_ROW_CLOSED: usize = 3;

/// Windows in a row that have to agree before the force state changes.
const FORCE_DEBOUNCE: u32 = 2;
/// How fast the signal level of a stroke follows the measurements while there is force.
const STROKE_GAIN: f64 = 0.25;
/// The learned reference moves by this part of the difference per pressure window.
const REFERENCE_GAIN: f64 = 1.0 / 64.0;

/// What an MPP v2 position window says about the tip.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TipSignal {
    /// The force tone: `None` if neither row carries it.
    pub force_tone: Option<bool>,
    /// Energy of the tip signal from the interpolated peaks of both axes.
    pub energy: Option<f64>,
    /// Energy of the tip signal from the summed amplitudes of three antennas of both axes.
    pub sum: Option<f64>,
}

#[derive(Clone, Debug)]
pub struct ContactStage {
    config: ContactConfig,

    tip: TipSignal,

    /// The debounced force tone and for how many windows it was seen / not seen.
    force: bool,
    force_on: u32,
    force_off: u32,

    /// The tip energy at contact: configured, or the running average of the windows in which
    /// the pen reported force, and how many of those there were.
    reference: f64,
    reference_samples: u32,

    /// Contact without force, and for how many samples in a row the signal was strong enough
    /// for one to begin.
    zero_force: bool,
    above: u32,

    /// Averages of the tip energy and of the summed tip signal over the part of the stroke
    /// that had force.
    stroke_energy: Option<f64>,
    stroke_sum: Option<f64>,

    /// Samples left of the pause after a firm stroke, and the pressure of the last three
    /// pressure windows that decides about it.
    lockout: u32,
    recent_pressure: [f64; 3],

    /// The summed magnitude of the tone in the first row of the last position window, and
    /// whether it says that the eraser end rests on the glass (see `eraser_touch`).
    position_tone: u32,
    eraser_touch: bool,
}

impl ContactStage {
    pub fn new(config: &ContactConfig) -> ContactStage {
        let mut stage = ContactStage {
            config: config.clone(),
            tip: TipSignal::default(),
            force: false,
            force_on: 0,
            force_off: 0,
            reference: 0.0,
            reference_samples: 0,
            zero_force: false,
            above: 0,
            stroke_energy: None,
            stroke_sum: None,
            lockout: 0,
            recent_pressure: [0.0; 3],
            position_tone: 0,
            eraser_touch: false,
        };
        stage.take_configured_reference();
        stage
    }

    fn take_configured_reference(&mut self) {
        let zf = &self.config.zero_force;
        if zf.reference > 0.0 {
            self.reference = zf.reference;
            self.reference_samples = zf.min_samples.max(1);
        }
    }

    /// Replaces the configuration. A configured reference replaces a learned one, and going
    /// from a configured reference to none starts the learning anew.
    pub fn set_config(&mut self, config: &ContactConfig) {
        let had_configured = self.config.zero_force.reference > 0.0;
        self.config = config.clone();
        if had_configured && config.zero_force.reference <= 0.0 {
            self.reference = 0.0;
            self.reference_samples = 0;
        }
        self.take_configured_reference();
    }

    pub fn force(&self) -> bool {
        self.force
    }

    /// The pen reports force and its last position window carried the force tone: the tip
    /// switch is closed right now, not about to be debounced open.
    pub fn force_steady(&self) -> bool {
        self.force && self.force_off == 0
    }

    /// The summed magnitude (both axes) of the first row of the last position window.
    pub fn position_tone(&self) -> u32 {
        self.position_tone
    }

    pub fn tip(&self) -> &TipSignal {
        &self.tip
    }

    /// The tip energy that counts as contact, 0 while it is neither configured nor learned.
    pub fn reference(&self) -> f64 {
        self.reference
    }

    /// The force tone of the position window (0x06). Only called while the pen is in range.
    pub fn position_window(&mut self, w: &DftWindow<'_>) {
        let tone = signal::tone_bit(w, POSITION_ROW_OPEN, POSITION_ROW_CLOSED, self.config.force_tone_min_magnitude);
        self.debounce(tone);
        self.position_tone = if w.x.is_empty() || w.y.is_empty() { 0 } else { signal::pair_magnitude(w, 0) };
    }

    /// Whether the eraser end rests on the glass without being pressed.
    ///
    /// The eraser has a switch instead of a force sensor, and no force tone. Its tone in the
    /// position window is about 50 times stronger (in magnitude, with both axes summed: 0.5
    /// to 7 million) on the glass than hovering just above it (up to 0.14 million), so with
    /// zero force contact enabled that strength counts as contact, with two thresholds.
    pub fn eraser_touch(&mut self) -> bool {
        let (enter, exit) = (self.config.eraser_touch_enter, self.config.eraser_touch_exit);
        self.eraser_touch =
            self.config.zero_force.enabled && enter > 0 && self.position_tone >= if self.eraser_touch { exit.min(enter) } else { enter };
        self.eraser_touch
    }

    /// Reads the tip signal of an MPP v2 position window (0x07, 0x08).
    pub fn measure(&mut self, w: &DftWindow<'_>) {
        self.tip.force_tone = None;
        self.tip.energy = None;

        // The summed signal of a window that is too short stays what it was.
        if w.x.len() <= MPP2_ROW_CLOSED {
            return;
        }

        let exponent = self.config.zero_force.peak_exponent;
        let peak_x = signal::peak_amplitude(&w.x[0], exponent);
        let peak_y = signal::peak_amplitude(&w.y[0], exponent);
        if !peak_x.is_nan() && !peak_y.is_nan() {
            self.tip.energy = Some((peak_x * peak_x) + (peak_y * peak_y));
        }

        let sum_x = signal::core_sum(&w.x[0]);
        let sum_y = signal::core_sum(&w.y[0]);
        self.tip.sum = (!sum_x.is_nan() && !sum_y.is_nan()).then_some((sum_x * sum_x) + (sum_y * sum_y));

        self.tip.force_tone = signal::tone_bit(w, MPP2_ROW_OPEN, MPP2_ROW_CLOSED, self.config.mpp2_force_tone_min_magnitude);
    }

    /// Decides about contact after [`measure`](Self::measure). Only called while the pen is
    /// in range. `eraser`: the eraser end is in use, which never has zero force contact.
    pub fn decide(&mut self, eraser: bool) -> bool {
        self.debounce(self.tip.force_tone);

        if self.lockout > 0 {
            self.lockout -= 1;
        }

        let zf = &self.config.zero_force;
        let start = zf.start_samples.max(1);
        let usable = zf.enabled && !eraser && self.reference_samples > 0 && self.reference_samples >= zf.min_samples;

        match self.tip.energy.filter(|_| usable) {
            None => {
                self.zero_force = false;
                self.above = 0;
            }
            Some(energy) if self.force => {
                // The tip is on the glass: zero force contact may follow at once.
                self.zero_force = false;
                self.above = start;
                self.stroke_energy = Some(follow(self.stroke_energy, energy));
                if let Some(sum) = self.tip.sum {
                    self.stroke_sum = Some(follow(self.stroke_sum, sum));
                }
            }
            Some(_) if self.lockout > 0 => {
                self.zero_force = false;
                self.above = 0;
                self.end_stroke();
            }
            Some(energy) if self.zero_force || self.above >= start => {
                self.zero_force = continues(zf, energy, self.reference, self.stroke_energy, self.tip.sum, self.stroke_sum);
                if !self.zero_force {
                    self.above = 0;
                    self.end_stroke();
                }
            }
            Some(energy) => {
                // Out of hover the signal has to stay strong for a number of samples.
                if energy >= zf.enter * self.reference {
                    self.above += 1;
                } else {
                    self.above = 0;
                }
                self.zero_force = self.above >= start;
            }
        }

        self.force || self.zero_force
    }

    /// Takes the pressure of a pressure window (0 without force): it decides about the pause
    /// after a firm stroke, and a window with force teaches the reference.
    pub fn pressure_window(&mut self, pressure: f64) {
        self.recent_pressure = [self.recent_pressure[1], self.recent_pressure[2], pressure];

        if pressure > 0.0 && self.force {
            if let Some(energy) = self.tip.energy {
                if self.reference_samples == 0 {
                    self.reference = energy;
                } else {
                    self.reference += (energy - self.reference) * REFERENCE_GAIN;
                }
                self.reference_samples += 1;
            }
        }
    }

    /// The pen left. The reference stays.
    pub fn reset(&mut self) {
        self.tip = TipSignal::default();
        self.force = false;
        self.force_on = 0;
        self.force_off = 0;
        self.zero_force = false;
        self.above = 0;
        self.end_stroke();
        self.lockout = 0;
        self.recent_pressure = [0.0; 3];
        self.position_tone = 0;
        self.eraser_touch = false;
    }

    fn end_stroke(&mut self) {
        self.stroke_energy = None;
        self.stroke_sum = None;
    }

    /// The pen counts as pressed after two windows in a row with the tone in the "closed" row
    /// and as released after two windows in a row without it. A window without any tone does
    /// not interrupt a beginning press.
    fn debounce(&mut self, tone: Option<bool>) {
        if tone == Some(true) {
            self.force_on += 1;
            self.force_off = 0;
        } else {
            self.force_off += 1;
            if tone.is_some() {
                self.force_on = 0;
            }
        }

        if !self.force && self.force_on >= FORCE_DEBOUNCE {
            self.force = true;
        } else if self.force && self.force_off >= FORCE_DEBOUNCE {
            self.force = false;

            // A pen lifted from a firm stroke is still close to the glass, with a tip signal
            // nearly as strong as on it. It was lifted on purpose.
            let zf = &self.config.zero_force;
            let recent = num::max3(self.recent_pressure[0], self.recent_pressure[1], self.recent_pressure[2]);
            if zf.lockout_pressure > 0.0 && recent >= zf.lockout_pressure {
                self.lockout = zf.lockout_samples;
            }
        }
    }
}

/// A running average that starts at the first value.
fn follow(average: Option<f64>, value: f64) -> f64 {
    let a = average.unwrap_or(value);
    a + ((value - a) * STROKE_GAIN)
}

/// Whether a zero force contact goes on.
///
/// A stroke that had force (and whose summed tip signal is known) is judged by that sum
/// alone, relative to what it was during the force: the interpolated peak of a tilted pen
/// changes too much within a stroke. Any other contact needs the tip energy above the exit
/// level, and above the stroke level of its energy during the force if it had any.
fn continues(
    zf: &ZeroForceConfig,
    energy: f64,
    reference: f64,
    stroke_energy: Option<f64>,
    sum: Option<f64>,
    stroke_sum: Option<f64>,
) -> bool {
    let level = zf.stroke_level;
    if level > 0.0 {
        if let (Some(sum), Some(stroke_sum)) = (sum, stroke_sum) {
            return sum >= level * stroke_sum;
        }
    }
    let faded = level > 0.0 && stroke_energy.is_some_and(|s| energy < level * s);
    energy >= zf.exit * reference && !faded
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(enabled: bool, reference: f64) -> ContactConfig {
        let mut c = ContactConfig::default();
        c.zero_force.enabled = enabled;
        c.zero_force.reference = reference;
        c
    }

    fn step(stage: &mut ContactStage, tone: Option<bool>, energy: f64) -> bool {
        stage.tip = TipSignal { force_tone: tone, energy: Some(energy), sum: Some(energy) };
        stage.decide(false)
    }

    #[test]
    fn force_tone_is_debounced() {
        let mut s = ContactStage::new(&config(false, 0.0));
        assert!(!step(&mut s, Some(true), 1.0));
        assert!(step(&mut s, Some(true), 1.0));
        // A window without the tone neither starts nor, alone, ends the contact.
        assert!(step(&mut s, None, 1.0));
        assert!(step(&mut s, Some(true), 1.0));
        assert!(step(&mut s, Some(false), 1.0));
        assert!(!step(&mut s, Some(false), 1.0));
        // The first press again, interrupted by a window without the tone.
        assert!(!step(&mut s, Some(true), 1.0));
        assert!(!step(&mut s, None, 1.0));
        assert!(step(&mut s, Some(true), 1.0));
    }

    #[test]
    fn zero_force_contact_needs_a_run_of_strong_samples() {
        let mut s = ContactStage::new(&config(true, 100.0));
        for i in 0..7 {
            assert!(!step(&mut s, Some(false), 90.0), "sample {i}");
        }
        assert!(step(&mut s, Some(false), 90.0));
        // Between exit and enter it stays, below exit it ends.
        assert!(step(&mut s, Some(false), 80.0));
        assert!(!step(&mut s, Some(false), 74.0));
        // A weak sample restarts the run.
        for _ in 0..7 {
            assert!(!step(&mut s, Some(false), 90.0));
        }
        assert!(!step(&mut s, Some(false), 80.0));
        for _ in 0..7 {
            assert!(!step(&mut s, Some(false), 90.0));
        }
        assert!(step(&mut s, Some(false), 90.0));
    }

    #[test]
    fn a_stroke_with_force_continues_by_its_own_level() {
        let mut s = ContactStage::new(&config(true, 100.0));
        step(&mut s, Some(true), 300.0);
        assert!(step(&mut s, Some(true), 300.0));
        // Force gone: contact goes on at once, judged against the stroke (0.8 * 300).
        assert!(step(&mut s, Some(false), 300.0));
        assert!(step(&mut s, Some(false), 250.0));
        assert!(step(&mut s, Some(false), 241.0));
        // Still far above the reference, but faded relative to the stroke.
        assert!(!step(&mut s, Some(false), 230.0));
    }

    #[test]
    fn a_firm_stroke_is_followed_by_a_pause() {
        let mut s = ContactStage::new(&config(true, 100.0));
        step(&mut s, Some(true), 150.0);
        step(&mut s, Some(true), 150.0);
        s.pressure_window(0.5);
        step(&mut s, Some(false), 150.0);
        assert!(!step(&mut s, Some(false), 150.0));
        // 50 samples of pause, counted from the sample that ended the force; the first sample
        // after it is the first of the 8 that a contact out of hover needs.
        for i in 0..48 + 7 {
            assert!(!step(&mut s, Some(false), 150.0), "sample {i}");
        }
        assert!(step(&mut s, Some(false), 150.0));
    }

    #[test]
    fn the_reference_is_learned_from_windows_with_force() {
        let mut s = ContactStage::new(&config(true, 0.0));
        for _ in 0..20 {
            assert!(!step(&mut s, Some(false), 100.0));
        }
        step(&mut s, Some(true), 100.0);
        step(&mut s, Some(true), 100.0);
        for _ in 0..20 {
            s.pressure_window(0.1);
        }
        assert_eq!(s.reference(), 100.0);
        step(&mut s, Some(false), 100.0);
        step(&mut s, Some(false), 100.0);
        assert!(step(&mut s, Some(false), 100.0));
        // The eraser end has no zero force contact.
        s.tip = TipSignal { force_tone: Some(false), energy: Some(100.0), sum: Some(100.0) };
        assert!(!s.decide(true));
    }

    #[test]
    fn the_eraser_touches_by_the_strength_of_its_tone() {
        use super::super::signal::testrows::row;
        let mut s = ContactStage::new(&config(true, 0.0));
        let tone = |s: &mut ContactStage, magnitude: u32| {
            let rows = [row(0, [0; 9], magnitude / 2), row(0, [0; 9], 0), row(0, [0; 9], 0)];
            s.position_window(&DftWindow { group: None, kind: 6, timestamp: 0, columns: 0, rows: 0, x: &rows, y: &rows });
            s.eraser_touch()
        };
        assert!(!tone(&mut s, 140_000));
        assert!(tone(&mut s, 4_000_000));
        assert!(tone(&mut s, 300_000));
        assert!(!tone(&mut s, 200_000));
        assert!(!tone(&mut s, 300_000));
        // Not without zero force contact.
        let mut s = ContactStage::new(&config(false, 0.0));
        assert!(!tone(&mut s, 4_000_000));
    }

    #[test]
    fn disabled() {
        let mut s = ContactStage::new(&config(false, 100.0));
        for _ in 0..20 {
            assert!(!step(&mut s, Some(false), 500.0));
        }
    }
}
