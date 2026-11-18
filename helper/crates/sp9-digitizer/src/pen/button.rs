// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
// Derived from iptsd (src/core/generic/dft.hpp) with the pen patches of this repository,
// GPL-2.0-or-later, Copyright (C) the iptsd authors.
//! Stage "button": the barrel button and the eraser end.
//!
//! Two windows take part. The binary window (twice per frame) carries a bit as a tone in one
//! of two rows: whether the barrel button or the eraser is in use at all. The button window
//! carries a tone whose phase relative to the tip tone of the position window tells the two
//! apart: opposite phase is the barrel button, same phase the eraser.

use super::signal;
use crate::config::ButtonConfig;
use crate::frame::DftWindow;

/// Rows of the bit in the binary window: clear / set.
const BINARY_ROW_CLEAR: usize = 4;
const BINARY_ROW_SET: usize = 5;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Buttons {
    pub barrel: bool,
    pub eraser: bool,
}

#[derive(Clone, Debug)]
pub struct ButtonStage {
    config: ButtonConfig,

    /// The frame whose first binary window was seen, and whether its second one was.
    binary_group: Option<u32>,
    binary_second_seen: bool,
    /// What the binary windows say (`None`: nothing), and what they said a frame before.
    binary: Option<bool>,
    binary_before: Option<bool>,

    /// What the phase alone said in the previous frame; without the binary window a state
    /// counts once two frames in a row agree on it.
    candidate: Buttons,
}

impl ButtonStage {
    pub fn new(config: &ButtonConfig) -> ButtonStage {
        ButtonStage {
            config: config.clone(),
            binary_group: None,
            binary_second_seen: false,
            binary: None,
            binary_before: None,
            candidate: Buttons::default(),
        }
    }

    pub fn set_config(&mut self, config: &ButtonConfig) {
        self.config = config.clone();
    }

    /// Takes a binary window (0x0A).
    ///
    /// Normally the stronger of the two rows has more than 1000 times the magnitude of the
    /// other. For two to three frames while the pen comes into range both rows carry a signal
    /// (the stronger 3 to 17 times the weaker) and the first window reads as a pressed button
    /// although none is: a window whose rows are closer than the configured purity decides
    /// nothing, and such a second window takes back what the first one decided.
    pub fn binary_window(&mut self, w: &DftWindow<'_>) {
        if w.x.len() <= BINARY_ROW_SET || w.group.is_none() {
            return;
        }

        let clear = signal::pair_magnitude(w, BINARY_ROW_CLEAR);
        let set = signal::pair_magnitude(w, BINARY_ROW_SET);
        let threshold = 2 * u64::from(self.config.binary_min_magnitude);
        let weak = u64::from(clear) < threshold && u64::from(set) < threshold;

        let purity = self.config.binary_purity;
        let mixed = purity > 0.0 && !weak && f64::from(clear.max(set)) < purity * f64::from(clear.min(set));

        if self.binary_group == w.group {
            if mixed && !self.binary_second_seen {
                self.binary = self.binary_before;
            }
            self.binary_second_seen = true;
            return;
        }

        self.binary_group = w.group;
        self.binary_before = self.binary;
        self.binary_second_seen = false;

        if mixed {
            return;
        }
        self.binary = if weak { None } else { Some(clear < set) };
    }

    /// Takes a button window (0x09) of the frame whose position window had the phase
    /// reference `tip_phase` (summed center components, real and imaginary).
    /// `eraser_tone`: the caller knows that this tone is the eraser's own (see `Pen`), so
    /// the eraser does not have to show in two frames first.
    pub fn button_window(&mut self, w: &DftWindow<'_>, tip_phase: (i32, i32), eraser_tone: bool) -> Buttons {
        let active = self.binary.unwrap_or_else(|| {
            w.x[0].magnitude > self.config.min_magnitude && w.y[0].magnitude > self.config.min_magnitude && self.is_button_tone(w)
        });

        let mut now = Buttons::default();
        if active {
            let (real, imag) = signal::center_phase(w);
            // 32 bit arithmetic of the reference: wraps around for very strong signals.
            let dot = tip_phase.0.wrapping_mul(real).wrapping_add(tip_phase.1.wrapping_mul(imag));
            now.barrel = dot < 0;
            now.eraser = dot > 0;
        }

        if self.binary.is_some() {
            return now;
        }
        if eraser_tone && now.eraser {
            self.candidate = now;
            return now;
        }

        // The phase of a weak signal is noise in the first frame after the pen came into range.
        let agreed = Buttons { barrel: now.barrel && self.candidate.barrel, eraser: now.eraser && self.candidate.eraser };
        self.candidate = now;
        agreed
    }

    /// Whether the first row of a button window can be the tone of a button. The other rows
    /// carry other tones of the pen, and what they leak into the first row passes the
    /// magnitude threshold; then they are much stronger than the first row, on both axes.
    fn is_button_tone(&self, w: &DftWindow<'_>) -> bool {
        let ratio = self.config.tone_ratio;
        if ratio <= 0.0 || w.x.len() < 4 || w.y.len() < 4 {
            return true;
        }
        let others = |rows: &[crate::frame::Row]| rows[1].magnitude.max(rows[2].magnitude).max(rows[3].magnitude);
        f64::from(others(w.x)) <= ratio * f64::from(w.x[0].magnitude) || f64::from(others(w.y)) <= ratio * f64::from(w.y[0].magnitude)
    }

    /// The pen left. Which frame's binary windows were seen stays.
    pub fn reset(&mut self) {
        self.binary = None;
        self.binary_before = None;
        self.candidate = Buttons::default();
    }
}

#[cfg(test)]
mod tests {
    use super::super::signal::testrows::*;
    use super::*;
    use crate::frame::Row;

    fn binary(group: u32, clear: u32, set: u32) -> (Vec<Row>, u32) {
        let mut rows = vec![row(0, [0; 9], 0); 6];
        rows[4].magnitude = clear / 2;
        rows[5].magnitude = set / 2;
        (rows, group)
    }

    fn feed_binary(stage: &mut ButtonStage, (rows, group): &(Vec<Row>, u32)) {
        stage.binary_window(&DftWindow { group: Some(*group), kind: 0x0A, timestamp: 0, columns: 0, rows: 0, x: rows, y: rows });
    }

    fn button(stage: &mut ButtonStage, amplitude: i16, magnitude: u32) -> Buttons {
        let rows = [row(0, [0, 0, 0, 0, amplitude, 0, 0, 0, 0], magnitude), row(0, [0; 9], 10), row(0, [0; 9], 10), row(0, [0; 9], 10)];
        stage.button_window(&DftWindow { group: Some(1), kind: 9, timestamp: 0, columns: 0, rows: 0, x: &rows, y: &rows }, (1000, 0), false)
    }

    #[test]
    fn binary_window_decides_and_the_phase_tells_button_from_eraser() {
        let mut s = ButtonStage::new(&ButtonConfig::default());
        feed_binary(&mut s, &binary(1, 100, 1_000_000));
        assert_eq!(button(&mut s, -500, 5000), Buttons { barrel: true, eraser: false });
        assert_eq!(button(&mut s, 500, 5000), Buttons { barrel: false, eraser: true });
        feed_binary(&mut s, &binary(2, 1_000_000, 100));
        assert_eq!(button(&mut s, -500, 5000), Buttons::default());
    }

    #[test]
    fn mixed_binary_windows_decide_nothing() {
        let mut s = ButtonStage::new(&ButtonConfig::default());
        feed_binary(&mut s, &binary(1, 1_000_000, 100));
        feed_binary(&mut s, &binary(1, 1_000_000, 100));
        // First window of the next frame reads "set" cleanly, the second one is mixed: the
        // state of the frame before comes back.
        feed_binary(&mut s, &binary(2, 100, 1_000_000));
        assert_eq!(s.binary, Some(true));
        feed_binary(&mut s, &binary(2, 300_000, 1_000_000));
        assert_eq!(s.binary, Some(false));
        // A mixed first window leaves the state alone.
        feed_binary(&mut s, &binary(3, 300_000, 1_000_000));
        assert_eq!(s.binary, Some(false));
        // Weak windows: nothing is known.
        feed_binary(&mut s, &binary(4, 100, 100));
        assert_eq!(s.binary, None);
    }

    #[test]
    fn a_known_eraser_tone_counts_at_once() {
        let mut s = ButtonStage::new(&ButtonConfig::default());
        let rows = [row(0, [0, 0, 0, 0, 500, 0, 0, 0, 0], 250_000), row(0, [0; 9], 10), row(0, [0; 9], 10), row(0, [0; 9], 10)];
        let w = DftWindow { group: Some(1), kind: 9, timestamp: 0, columns: 0, rows: 0, x: &rows, y: &rows };
        assert_eq!(s.button_window(&w, (1000, 0), true), Buttons { barrel: false, eraser: true });
        // The frame after agrees without the hint.
        assert_eq!(s.button_window(&w, (1000, 0), false), Buttons { barrel: false, eraser: true });
        // The hint does not make a barrel button.
        let mut s = ButtonStage::new(&ButtonConfig::default());
        let rows = [row(0, [0, 0, 0, 0, -500, 0, 0, 0, 0], 250_000), row(0, [0; 9], 10), row(0, [0; 9], 10), row(0, [0; 9], 10)];
        let w = DftWindow { group: Some(1), kind: 9, timestamp: 0, columns: 0, rows: 0, x: &rows, y: &rows };
        assert_eq!(s.button_window(&w, (1000, 0), true), Buttons::default());
    }

    #[test]
    fn without_the_binary_window_two_frames_have_to_agree() {
        let mut s = ButtonStage::new(&ButtonConfig::default());
        assert_eq!(button(&mut s, -500, 5000), Buttons::default());
        assert_eq!(button(&mut s, -500, 5000), Buttons { barrel: true, eraser: false });
        // Too weak, and other tones much stronger than the first row.
        assert_eq!(button(&mut s, -500, 500), Buttons::default());
        let rows = [row(0, [0, 0, 0, 0, -500, 0, 0, 0, 0], 5000), row(0, [0; 9], 60000), row(0, [0; 9], 10), row(0, [0; 9], 10)];
        let w = DftWindow { group: Some(1), kind: 9, timestamp: 0, columns: 0, rows: 0, x: &rows, y: &rows };
        assert!(!s.is_button_tone(&w));
        assert_eq!(s.button_window(&w, (1000, 0), false), Buttons::default());
    }
}
