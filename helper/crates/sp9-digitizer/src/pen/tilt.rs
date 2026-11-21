// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
// Derived from iptsd (src/core/generic/dft.hpp) with the pen patches of this repository,
// GPL-2.0-or-later, Copyright (C) the iptsd authors.
//! Stage "tilt": the tilt of the pen from the horizontal distance between its tip and its
//! ring electrode, and the offset of the tip from the measured position that comes with it.

use super::position::{Mapping, TipPosition};
use super::signal;
use crate::config::{Geometry, TiltConfig};
use crate::frame::DftWindow;
use crate::num;
use std::f64::consts::PI;

/// How many measurements the median of the smoothing looks back.
const SMOOTHING_WINDOW: usize = 40;
/// The smoothed angle follows once the median is this far away (radians) ...
const SMOOTHING_BAND: f64 = 5.0 * PI / 180.0;
/// ... by this part of the distance per measurement.
const SMOOTHING_GAIN: f64 = 0.19;
/// The ring cannot be further from the tip than it is on the pen; measurements beyond this
/// sine of the tilt are dropped.
const MAX_SINE: f64 = 1.2;
/// An offset of more than this many antennas is not a tilt.
const MAX_OFFSET: f64 = 0.5;

/// Smoothing for an angle that is measured with a few degrees of noise but changes slowly:
/// the output moves towards the median of the last measurements, and only while the two are
/// apart by more than a dead band. A steady pen reports a constant angle.
#[derive(Clone, Debug)]
struct AngleSmoother {
    values: [f64; SMOOTHING_WINDOW],
    count: usize,
    next: usize,
    /// The last input, continued across the wrap.
    last: f64,
    output: f64,
}

impl AngleSmoother {
    fn new() -> Self {
        AngleSmoother { values: [0.0; SMOOTHING_WINDOW], count: 0, next: 0, last: 0.0, output: 0.0 }
    }

    /// `period`: after which the angle wraps around, or 0.
    fn update(&mut self, mut value: f64, period: f64) -> f64 {
        if self.count > 0 && period > 0.0 {
            value -= period * ((value - self.last) / period).round();
        }
        self.last = value;

        self.values[self.next] = value;
        self.next = (self.next + 1) % SMOOTHING_WINDOW;
        self.count = (self.count + 1).min(SMOOTHING_WINDOW);

        let mut sorted = self.values;
        let sorted = &mut sorted[..self.count];
        sorted.sort_unstable_by(f64::total_cmp);
        let mut median = sorted[self.count / 2];
        if self.count % 2 == 0 {
            median = (median + sorted[self.count / 2 - 1]) / 2.0;
        }

        if self.count == 1 {
            self.output = median;
        } else if (median - self.output).abs() >= SMOOTHING_BAND {
            self.output += SMOOTHING_GAIN * (median - self.output);
        }

        if period <= 0.0 {
            return self.output;
        }
        self.output - (period * (self.output / period).floor())
    }

    fn reset(&mut self) {
        self.count = 0;
        self.next = 0;
    }
}

#[derive(Clone, Debug)]
pub struct TiltStage {
    min_magnitude: u32,
    ring_distance_cm: f64,
    offset_linear: f64,
    offset_quadratic: f64,
    offset_gain: f64,
    width_cm: f64,
    height_cm: f64,

    /// The smoothed tilt (radians from the vertical, direction), if measured since the pen
    /// came into range.
    angles: Option<(f64, f64)>,
    altitude: AngleSmoother,
    azimuth: AngleSmoother,
    /// The smoothed offset of the tip in antennas, if measured.
    offset: Option<(f64, f64)>,
}

impl TiltStage {
    pub fn new(config: &TiltConfig, geometry: &Geometry) -> TiltStage {
        TiltStage {
            min_magnitude: config.min_magnitude,
            ring_distance_cm: config.ring_distance_cm,
            offset_linear: config.offset_linear,
            offset_quadratic: config.offset_quadratic,
            offset_gain: config.offset_gain,
            width_cm: geometry.width_cm,
            height_cm: geometry.height_cm,
            angles: None,
            altitude: AngleSmoother::new(),
            azimuth: AngleSmoother::new(),
            offset: None,
        }
    }

    pub fn set_config(&mut self, config: &TiltConfig) {
        self.min_magnitude = config.min_magnitude;
        self.ring_distance_cm = config.ring_distance_cm;
        self.offset_linear = config.offset_linear;
        self.offset_quadratic = config.offset_quadratic;
        self.offset_gain = config.offset_gain;
    }

    /// The tilt as (altitude, azimuth) in radians: 0 is upright, the azimuth counts
    /// counterclockwise from the X axis on a screen whose Y axis points down.
    pub fn angles(&self) -> Option<(f64, f64)> {
        self.angles
    }

    /// The offset of the tip from the measured position, in antennas.
    pub fn offset(&self) -> Option<(f64, f64)> {
        self.offset
    }

    /// Takes the ring tone (second row) of an MPP v2 position window whose tip is at `tip`.
    /// Without a usable ring signal the last tilt stays.
    pub fn update(&mut self, w: &DftWindow<'_>, tip: &TipPosition, mapping: &Mapping) {
        let (tx, ty) = (tip.centroid_x, tip.centroid_y);
        if w.x.len() < 2 || w.y.len() < 2 || tx.is_nan() || ty.is_nan() {
            return;
        }
        if w.x[1].magnitude <= self.min_magnitude || w.y[1].magnitude <= self.min_magnitude {
            return;
        }
        let rx = signal::ring_position(&w.x[1]);
        let ry = signal::ring_position(&w.y[1]);
        if rx.is_nan() || ry.is_nan() {
            return;
        }

        // Tip to ring in antennas, and in centimeters on the screen.
        let ax = rx - tx;
        let ay = ry - ty;
        let cx = (mapping.x(rx) - mapping.x(tx)) * self.width_cm;
        let cy = (mapping.y(ry) - mapping.y(ty)) * self.height_cm;

        let sine = cx.hypot(cy) / self.ring_distance_cm;
        if sine > MAX_SINE {
            return;
        }
        let altitude = num::min(1.0, sine).asin();
        let azimuth = ((-cy).atan2(cx) + (2.0 * PI)) % (2.0 * PI);
        self.angles = Some((self.altitude.update(altitude, 0.0), self.azimuth.update(azimuth, 2.0 * PI)));

        if self.offset_linear == 0.0 && self.offset_quadratic == 0.0 {
            return;
        }

        // The field of the tip electrode of a tilted pen is strongest not below the point of
        // the tip but shifted towards the body, by an amount that grows a little faster than
        // the tip-ring distance. The correction points from the ring to the tip.
        let d = ax.hypot(ay);
        if d <= 0.0 {
            return;
        }
        let length = (self.offset_quadratic * d * d) + (self.offset_linear * d);
        let ox = -ax / d * length;
        let oy = -ay / d * length;

        if ox.abs() > MAX_OFFSET || oy.abs() > MAX_OFFSET {
            self.offset = None;
            return;
        }
        self.offset = Some(match self.offset {
            Some((px, py)) => {
                let gain = num::clamp(self.offset_gain, 0.0, 1.0);
                (px + gain * (ox - px), py + gain * (oy - py))
            }
            None => (ox, oy),
        });
    }

    /// The pen left: the next tilt is measured anew.
    pub fn reset(&mut self) {
        self.angles = None;
        self.altitude.reset();
        self.azimuth.reset();
        self.offset = None;
    }
}

#[cfg(test)]
mod tests {
    use super::super::signal::testrows::*;
    use super::*;
    use crate::config::PositionConfig;

    fn window_rows(tip: f64, ring: f64) -> [crate::frame::Row; 2] {
        [bump(10, tip, 3000.0, 1.0), bump(10, ring, 2000.0, 1.6)]
    }

    #[test]
    fn upright_and_tilted() {
        let geometry = Geometry::SP9;
        let mapping = Mapping::new(&PositionConfig::default(), &geometry);
        let mut tilt = TiltStage::new(&TiltConfig::default(), &geometry);

        let x = window_rows(14.0, 14.0);
        let w = DftWindow { group: None, kind: 7, timestamp: 0, columns: 68, rows: 46, x: &x, y: &x };
        let tip = TipPosition { x: 14.0, y: 14.0, centroid_x: 14.0, centroid_y: 14.0 };
        tilt.update(&w, &tip, &mapping);
        assert_eq!(tilt.angles(), Some((0.0, 0.0)));
        assert_eq!(tilt.offset(), None);

        // The ring one antenna (4.07 mm) to the right of the tip: asin(4.07 / 9.098) = 26.6 degrees,
        // pointing along +X, and the tip is corrected to the left.
        tilt.reset();
        let x = window_rows(14.0, 15.0);
        let y = window_rows(14.0, 14.0);
        let w = DftWindow { group: None, kind: 7, timestamp: 0, columns: 68, rows: 46, x: &x, y: &y };
        tilt.update(&w, &tip, &mapping);
        let (altitude, azimuth) = tilt.angles().unwrap();
        assert!((altitude.to_degrees() - 26.6).abs() < 1.0, "{}", altitude.to_degrees());
        assert!(azimuth.abs() < 0.02 || (azimuth - 2.0 * PI).abs() < 0.02);
        let (ox, oy) = tilt.offset().unwrap();
        assert!((ox + 0.0402 + 0.0089).abs() < 0.003 && oy.abs() < 0.001, "{ox} {oy}");
    }

    #[test]
    fn steady_angle_is_constant_and_a_change_is_followed() {
        let mut s = AngleSmoother::new();
        let mut out = 0.0;
        for i in 0..100 {
            out = s.update(0.5 + if i % 2 == 0 { 0.03 } else { -0.03 }, 0.0);
        }
        assert_eq!(out, 0.53);
        for _ in 0..200 {
            out = s.update(0.9, 0.0);
        }
        assert!((out - 0.9).abs() < SMOOTHING_BAND);

        // An azimuth around the wrap is not averaged to the opposite direction.
        let mut s = AngleSmoother::new();
        for i in 0..40 {
            out = s.update(if i % 2 == 0 { 0.05 } else { 2.0 * PI - 0.05 }, 2.0 * PI);
        }
        assert!(!(0.2..2.0 * PI - 0.2).contains(&out), "{out}");
    }

    #[test]
    fn weak_ring_keeps_the_last_tilt() {
        let geometry = Geometry::SP9;
        let mapping = Mapping::new(&PositionConfig::default(), &geometry);
        let mut tilt = TiltStage::new(&TiltConfig::default(), &geometry);
        let mut x = window_rows(14.0, 15.0);
        x[1].magnitude = 100;
        let w = DftWindow { group: None, kind: 7, timestamp: 0, columns: 68, rows: 46, x: &x, y: &x };
        tilt.update(&w, &TipPosition { x: 14.0, y: 14.0, centroid_x: 14.0, centroid_y: 14.0 }, &mapping);
        assert_eq!(tilt.angles(), None);
    }
}
