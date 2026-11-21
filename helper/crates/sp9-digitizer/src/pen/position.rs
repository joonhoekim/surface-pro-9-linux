// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
// Derived from iptsd (src/core/generic/dft.hpp) with the pen patches of this repository,
// GPL-2.0-or-later, Copyright (C) the iptsd authors.
//! Stage "position": where the tip tone is on the sensor, in antennas, and the mapping from
//! antennas to the screen. No state besides the configuration.

use super::signal;
use crate::config::{Geometry, Harmonics, PositionConfig};
use crate::frame::Row;
use crate::num;

/// The tip position measured in one window, per axis in antennas.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TipPosition {
    /// The position to report.
    pub x: f64,
    pub y: f64,
    /// The uncorrected centroids (NaN at the screen edge), which the tilt is measured from.
    pub centroid_x: f64,
    pub centroid_y: f64,
}

/// Antenna positions to fractions of the screen (0 = left / top edge, 1 = right / bottom).
#[derive(Clone, Copy, Debug)]
pub struct Mapping {
    pitch_x: f64,
    pitch_y: f64,
    origin_x: f64,
    origin_y: f64,
    width_mm: f64,
    height_mm: f64,
}

impl Mapping {
    pub fn new(config: &PositionConfig, geometry: &Geometry) -> Mapping {
        Mapping {
            pitch_x: config.antenna_pitch_x,
            pitch_y: config.antenna_pitch_y,
            origin_x: config.antenna_origin_x,
            origin_y: config.antenna_origin_y,
            width_mm: geometry.width_cm * 10.0,
            height_mm: geometry.height_cm * 10.0,
        }
    }

    #[inline]
    pub fn x(&self, antennas: f64) -> f64 {
        ((antennas * self.pitch_x) + self.origin_x) / self.width_mm
    }

    #[inline]
    pub fn y(&self, antennas: f64) -> f64 {
        ((antennas * self.pitch_y) + self.origin_y) / self.height_mm
    }

    pub fn width_mm(&self) -> f64 {
        self.width_mm
    }

    pub fn height_mm(&self) -> f64 {
        self.height_mm
    }
}

/// How far the estimate of the edge half-cell is trusted as it is. Closer to the edge
/// antenna than this (in antennas) the amplitudes hardly change any more, also for a tip
/// beyond the antenna: with `edge_reach` that part of the estimate is spread over the strip
/// between here and the screen edge.
const EDGE_KNEE: f64 = 0.2;
/// The smallest estimate that occurs with the tip at or beyond the edge antenna.
const EDGE_FLOOR: f64 = 0.03;

#[derive(Clone, Debug)]
pub struct PositionStage {
    config: PositionConfig,
    mapping: Mapping,
    /// Antennas per axis.
    columns: u8,
    rows: u8,
    /// The screen beyond the first and the last antenna of each axis, in antennas.
    margin_x: (f64, f64),
    margin_y: (f64, f64),
}

impl PositionStage {
    pub fn new(config: &PositionConfig, geometry: &Geometry) -> PositionStage {
        let mapping = Mapping::new(config, geometry);
        let margins = |origin: f64, pitch: f64, size_mm: f64, antennas: u8| {
            let last = origin + (f64::from(antennas) - 1.0) * pitch;
            (num::max(origin / pitch, 0.0), num::max((size_mm - last) / pitch, 0.0))
        };
        PositionStage {
            config: config.clone(),
            mapping,
            columns: geometry.columns,
            rows: geometry.rows,
            margin_x: margins(config.antenna_origin_x, config.antenna_pitch_x, mapping.width_mm(), geometry.columns),
            margin_y: margins(config.antenna_origin_y, config.antenna_pitch_y, mapping.height_mm(), geometry.rows),
        }
    }

    pub fn mapping(&self) -> &Mapping {
        &self.mapping
    }

    pub fn config(&self) -> &PositionConfig {
        &self.config
    }

    /// The position from the weak tip tone of the position window: the parabola alone.
    pub fn coarse(&self, x: &Row, y: &Row) -> Option<(f64, f64)> {
        let axis = |row: &Row, antennas: u8| {
            let min = f64::from(self.config.min_amplitude);
            match signal::edge_of(row, antennas) {
                Some(edge) => from_edge(row, edge, signal::edge_parabola(row, edge, min, self.config.parabola_exponent)),
                None => signal::parabola_position(row, min, self.config.parabola_exponent),
            }
        };
        let px = axis(x, self.columns);
        let py = axis(y, self.rows);
        (!px.is_nan() && !py.is_nan()).then_some((px, py))
    }

    /// The position from the tip tone of an MPP v2 position window.
    ///
    /// The centroid of three antennas, with its error that repeats with the antenna pitch
    /// removed; with a weak signal (a pen hovering high), where the centroid jumps, it fades
    /// into the parabola. With the strongest antenna at the edge of the sensor there is no
    /// centroid: the position comes from the antennas on the inner side
    /// ([`signal::edge_fraction`]).
    pub fn fine(&self, x: &Row, y: &Row) -> Option<TipPosition> {
        let config = &self.config;
        let level = signal::tip_level(x, y, config.tip_signal_reference);
        let weight = centroid_weight(config, level);

        let axis = |row: &Row, antennas: u8, harmonics: &Harmonics, edge_fit: &[f64; signal::EDGE_COEFFICIENTS], margins: (f64, f64)| {
            let min = f64::from(config.min_amplitude);
            if let Some(edge) = signal::edge_of(row, antennas) {
                let inwards = signal::edge_fraction(row, edge, min, config.parabola_exponent, edge_fit);
                let margin = if edge == signal::Edge::Low { margins.0 } else { margins.1 };
                let inwards = if config.edge_reach { reach(inwards, margin) } else { inwards };
                return (from_edge(row, edge, inwards), f64::NAN);
            }

            let parabola = signal::parabola_position(row, min, config.parabola_exponent);
            let centroid = signal::centroid_position(row, config.centroid_exponent);
            if parabola.is_nan() || centroid.is_nan() {
                return (parabola, centroid);
            }
            let corrected = correct_cell(config, centroid, level, harmonics);
            (if weight >= 1.0 { corrected } else { parabola + (weight * (corrected - parabola)) }, centroid)
        };

        let (px, centroid_x) = axis(x, self.columns, &config.correction_x, &config.edge_x, self.margin_x);
        let (py, centroid_y) = axis(y, self.rows, &config.correction_y, &config.edge_y, self.margin_y);
        (!px.is_nan() && !py.is_nan()).then_some(TipPosition { x: px, y: py, centroid_x, centroid_y })
    }
}

/// The position of a tone `inwards` antennas from the edge antenna a row is centered on.
fn from_edge(row: &Row, edge: signal::Edge, inwards: f64) -> f64 {
    let k = f64::from(signal::center_antenna(row));
    match edge {
        signal::Edge::Low => k + inwards,
        signal::Edge::High => k - inwards,
    }
}

/// Spreads the part of the edge estimate that cannot tell positions apart (the tip at the
/// edge antenna or beyond it, see [`EDGE_KNEE`]) over the strip up to the screen edge,
/// `margin` antennas beyond the edge antenna. Negative results are beyond the antenna.
fn reach(inwards: f64, margin: f64) -> f64 {
    if inwards.is_nan() || inwards >= EDGE_KNEE {
        return inwards;
    }
    let part = (num::max(inwards, EDGE_FLOOR) - EDGE_FLOOR) / (EDGE_KNEE - EDGE_FLOOR);
    (part * (EDGE_KNEE + margin)) - margin
}

/// How much of the position is the centroid (the rest is the parabola) at a signal level.
fn centroid_weight(config: &PositionConfig, level: f64) -> f64 {
    let min = config.centroid_fade.min;
    let max = config.centroid_fade.max;
    if level.is_nan() || max <= 0.0 {
        return 1.0;
    }
    if max <= min {
        return if level >= max { 1.0 } else { 0.0 };
    }
    num::clamp((level - min) / (max - min), 0.0, 1.0)
}

/// Removes the error of the centroid within one antenna pitch: two harmonics of the
/// position, the first one changing with the signal level (limited to the measured range).
/// The antennas are at the integers, so the correction is continuous across them.
fn correct_cell(config: &PositionConfig, pos: f64, level: f64, h: &Harmonics) -> f64 {
    if level.is_nan() {
        return pos;
    }
    let min = config.correction_level.min;
    let max = num::max(min, config.correction_level.max);
    let l = num::clamp(level, min, max) - 1.0;
    let phase = 2.0 * std::f64::consts::PI * pos;

    let first_cos = h.cos + (h.cos_level * l);
    let first_sin = h.sin + (h.sin_level * l);

    let error = (first_cos * phase.cos()) + (first_sin * phase.sin()) + (h.cos2 * (2.0 * phase).cos()) + (h.sin2 * (2.0 * phase).sin());
    pos - error
}

#[cfg(test)]
mod tests {
    use super::super::signal::testrows::*;
    use super::*;

    #[test]
    fn mapping_uses_pitch_and_origin() {
        let m = Mapping::new(&PositionConfig::default(), &Geometry::SP9);
        assert!((m.x(0.0) * 273.89 - 0.44).abs() < 1e-12);
        assert!((m.x(67.0) * 273.89 - (0.44 + 67.0 * 4.07478)).abs() < 1e-9);
        assert!((m.y(45.0) * 182.59 - (0.94 + 45.0 * 4.01578)).abs() < 1e-9);
    }

    #[test]
    fn weak_signal_takes_the_parabola_and_strong_signal_the_corrected_centroid() {
        let config = PositionConfig::default();
        let stage = PositionStage::new(&config, &Geometry::SP9);
        let fine = |x, y| stage.fine(x, y);
        let weak = bump(10, 14.2, 400.0, 1.0);
        assert!(signal::tip_level(&weak, &weak, 3000.0) < 0.5);
        let p = fine(&weak, &weak).unwrap();
        assert_eq!(p.x, signal::parabola_position(&weak, 50.0, -0.7));

        let strong = bump(10, 14.2, 3000.0, 1.0);
        let level = signal::tip_level(&strong, &strong, 3000.0);
        assert!(level > 0.8);
        let p = fine(&strong, &strong).unwrap();
        assert_eq!(p.x, correct_cell(&config, p.centroid_x, level, &config.correction_x));
        assert!((p.x - p.centroid_x).abs() < 0.2 && p.x != p.centroid_x);
        assert_eq!(p.y, correct_cell(&config, p.centroid_y, level, &config.correction_y));
    }

    #[test]
    fn the_edge_estimate_reaches_the_screen_edge() {
        // Everything up to the floor is the screen edge, the knee stays where it is.
        assert_eq!(reach(0.0, 0.234), -0.234);
        assert_eq!(reach(EDGE_FLOOR, 0.234), -0.234);
        assert_eq!(reach(EDGE_KNEE, 0.234), EDGE_KNEE);
        assert_eq!(reach(0.37, 0.234), 0.37);
        let mid = reach((EDGE_FLOOR + EDGE_KNEE) / 2.0, 0.234);
        assert!((mid - (EDGE_KNEE - 0.234) / 2.0).abs() < 1e-12);

        // A tip on the first antenna of X, in the middle of Y: X is within 0.2 mm of the
        // left screen edge.
        let stage = PositionStage::new(&PositionConfig::default(), &Geometry::SP9);
        let x = row(-4, [0, 0, 0, 0, 2700, 320, 47, 16, 8], 1_000_000);
        let y = bump(16, 20.0, 3000.0, 1.0);
        let p = stage.fine(&x, &y).unwrap();
        assert!(p.centroid_x.is_nan() && !p.centroid_y.is_nan());
        assert!((stage.mapping().x(p.x) * 273.89).abs() < 0.2, "{}", p.x);
        // The last antenna of Y.
        let y = row(41, [8, 16, 47, 320, 2700, 0, 0, 0, 0], 1_000_000);
        let p = stage.fine(&bump(16, 20.0, 3000.0, 1.0), &y).unwrap();
        assert!(((stage.mapping().y(p.y) - 1.0) * 182.59).abs() < 0.2, "{}", p.y);
        // The weak tone of the position window keeps the pen in range there too.
        let weak = row(41, [1, 1, 3, 20, 200, 0, 0, 0, 0], 5000);
        let (_, cy) = stage.coarse(&bump(16, 20.0, 300.0, 1.0), &weak).unwrap();
        assert!((44.5..=45.0).contains(&cy), "{cy}");
    }

    #[test]
    fn correction_is_periodic_and_limited() {
        let config = PositionConfig::default();
        let h = config.correction_x;
        let e = |pos: f64, level: f64| pos - correct_cell(&config, pos, level, &h);
        assert!((e(3.25, 1.2) - e(17.25, 1.2)).abs() < 1e-12);
        assert_eq!(e(3.25, 5.0), e(3.25, 1.85));
        assert_eq!(e(3.25, 0.1), e(3.25, 0.95));
        // On an antenna the error is the sum of the cosine terms.
        assert!((e(4.0, 1.0) - (h.cos + h.cos2)).abs() < 1e-12);
    }
}
