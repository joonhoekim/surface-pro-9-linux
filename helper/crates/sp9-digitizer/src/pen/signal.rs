// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
// Derived from iptsd (src/core/generic/dft.hpp) with the pen patches of this repository,
// GPL-2.0-or-later, Copyright (C) the iptsd authors.
//! Measurements on the rows of a DFT window. No state: every function reads one or two rows.
//!
//! A row holds the complex amplitudes of nine neighbouring antennas at one tone, centered on
//! the strongest antenna (index [`CENTER`]). Positions are in antennas (antenna 0 is the first
//! one of the axis); a function that cannot measure returns NaN.

use crate::frame::{DftWindow, Row, CENTER, NUM_COMPONENTS};
use crate::num;

/// The side of the sensor on which the strongest antenna of a row is the outermost one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    /// Antenna 0.
    Low,
    /// The last antenna.
    High,
}

/// The number of the antenna a row is centered on.
#[inline]
pub fn center_antenna(row: &Row) -> i32 {
    i32::from(row.first) + CENTER as i32
}

/// Whether the strongest antenna of a row is the first or the last of `antennas`: then the
/// neighbour on the outer side does not exist and its components are zero.
pub fn edge_of(row: &Row, antennas: u8) -> Option<Edge> {
    let k = center_antenna(row);
    if k <= 0 {
        Some(Edge::Low)
    } else if k >= i32::from(antennas) - 1 {
        Some(Edge::High)
    } else {
        None
    }
}

/// The position of a tone from a parabola through the three antennas around the strongest
/// one, after raising their in-phase amplitudes to `exponent`.
pub fn parabola_position(row: &Row, min_amplitude: f64, exponent: f64) -> f64 {
    let mid = CENTER;
    let amp = row.amplitude(mid);
    if amp < min_amplitude {
        return f64::NAN;
    }

    // The neighbours projected onto the phase of the middle antenna.
    let re = f64::from(row.real[mid]) / amp;
    let im = f64::from(row.imag[mid]) / amp;
    let project = |i: usize| (re * f64::from(row.real[i])) + (im * f64::from(row.imag[i]));

    // A neighbour in antiphase (the barrel of a strongly tilted pen leaks in on that side)
    // counts as no signal.
    let a = num::max(project(mid - 1), 1.0).powf(exponent);
    let b = amp.powf(exponent);
    let c = num::max(project(mid + 1), 1.0).powf(exponent);

    // With a negative exponent the strongest antenna is the minimum of the parabola.
    if a + c <= 2.0 * b {
        return f64::NAN;
    }

    let d = (a - c) / (2.0 * (a - 2.0 * b + c));
    f64::from(center_antenna(row)) + num::clamp(d, -0.5, 0.5)
}

/// The strongest antenna of a row at the edge and the two antennas inwards of it: their
/// amplitudes, and the amplitudes in phase with the strongest one (at least 1).
fn inward(row: &Row, edge: Edge) -> ([f64; 3], [f64; 3]) {
    let index = |i: usize| match edge {
        Edge::Low => CENTER + i,
        Edge::High => CENTER - i,
    };
    let amp = row.amplitude(CENTER);
    let re = f64::from(row.real[CENTER]) / amp;
    let im = f64::from(row.imag[CENTER]) / amp;
    let mut amplitude = [0.0; 3];
    let mut in_phase = [0.0; 3];
    for i in 0..3 {
        amplitude[i] = num::max(row.amplitude(index(i)), 1.0);
        in_phase[i] = num::max((re * f64::from(row.real[index(i)])) + (im * f64::from(row.imag[index(i)])), 1.0);
    }
    in_phase[0] = amp;
    (amplitude, in_phase)
}

/// How far inwards of the edge antenna a tone is, 0 .. 0.5 antennas, from the parabola through
/// the edge antenna and the two next to it. It is the continuation of
/// [`parabola_position`] for the antenna next to the edge (the same three antennas), so the
/// two meet where the strongest antenna changes. NaN below `min_amplitude`.
///
/// A tone beyond the edge antenna reads as 0: the amplitudes of the inner antennas stop
/// changing there.
pub fn edge_parabola(row: &Row, edge: Edge, min_amplitude: f64, exponent: f64) -> f64 {
    if row.amplitude(CENTER) < min_amplitude {
        return f64::NAN;
    }
    let (_, p) = inward(row, edge);
    let [a, b, c] = p.map(|v| v.powf(exponent));
    if a + c <= 2.0 * b {
        return 0.0;
    }
    // The vertex relative to the middle one of the three, which is the first inner antenna.
    let d = (a - c) / (2.0 * (a - 2.0 * b + c));
    1.0 + num::clamp(d, -1.0, -0.5)
}

/// Number of coefficients of [`edge_fraction`].
pub const EDGE_COEFFICIENTS: usize = 7;

/// How far inwards of the edge antenna the tip is, 0 .. 0.5 antennas, from the shape of the
/// tip signal on the edge antenna and the two inwards of it.
///
/// With the tip between the edge antenna and the next one, the parabola of three antennas
/// that all lie on one side of the tip is off by 0.5 mm (rms, pen on the glass). The ratios
/// of the three amplitudes place the tip better: a polynomial in `l1 = ln(a1 / a0)` and
/// `l2 = ln(a2 / a0)` with the terms 1, l1, l2, l1², l1 l2, l2², l1³, fitted to interior
/// windows of this pen and sensor where the position is known from both sides (0.18 mm rms
/// on the glass, 0.45 mm hovering). Only ratios enter: the strength of the signal says
/// nothing about the position at the edge, where it falls both with the height of the pen
/// and with the tip leaving the sensor. Towards the next antenna the result hands over to
/// [`edge_parabola`], which is continuous with the interior.
pub fn edge_fraction(row: &Row, edge: Edge, min_amplitude: f64, exponent: f64, coefficients: &[f64; EDGE_COEFFICIENTS]) -> f64 {
    let parabola = edge_parabola(row, edge, min_amplitude, exponent);
    if parabola.is_nan() {
        return f64::NAN;
    }
    let (a, _) = inward(row, edge);
    let l1 = (a[1] / a[0]).ln();
    let l2 = (a[2] / a[0]).ln();
    let terms = [1.0, l1, l2, l1 * l1, l1 * l2, l2 * l2, l1 * l1 * l1];
    let mut fit = 0.0;
    for (c, t) in coefficients.iter().zip(terms) {
        fit += c * t;
    }
    let fit = num::clamp(fit, 0.0, 0.5);

    const HANDOVER_START: f64 = 0.3;
    const HANDOVER_END: f64 = 0.45;
    let weight = num::clamp((num::min(fit, parabola) - HANDOVER_START) / (HANDOVER_END - HANDOVER_START), 0.0, 1.0);
    fit + (weight * (parabola - fit))
}

/// The amplitudes of the three antennas around the strongest one, or `None` at the screen
/// edge (an antenna that does not exist reads as zero).
#[inline]
fn core_amplitudes(row: &Row) -> Option<[f64; 3]> {
    let mut amps = [0.0; 3];
    for (i, a) in amps.iter_mut().enumerate() {
        *a = row.amplitude(CENTER + i - 1);
        if *a < 1.0 {
            return None;
        }
    }
    Some(amps)
}

/// The position of a tone as the centroid of the three antennas around the strongest one.
/// The weights are the amplitudes raised to `exponent`, less the smallest of the three.
pub fn centroid_position(row: &Row, exponent: f64) -> f64 {
    let Some(amps) = core_amplitudes(row) else { return f64::NAN };
    let w = amps.map(|a| a.powf(exponent));

    let min = num::min3(w[0], w[1], w[2]);
    let sum = w[0] + w[1] + w[2] - (3.0 * min);
    if sum <= 0.0 {
        return f64::NAN;
    }
    f64::from(center_antenna(row)) + ((w[2] - w[0]) / sum)
}

/// The position of the ring tone: the centroid of the power over the five antennas around
/// the strongest group of three. The ring is further from the glass than the tip, its signal
/// is wider and, with a tilted pen, not centered in the window.
pub fn ring_position(row: &Row) -> f64 {
    let mut power = [0.0f64; NUM_COMPONENTS];
    let mut amp = [0.0f64; NUM_COMPONENTS];
    for i in 0..NUM_COMPONENTS {
        let re = f64::from(row.real[i]);
        let im = f64::from(row.imag[i]);
        power[i] = (re * re) + (im * im);
        amp[i] = power[i].sqrt();
    }

    let mut k = 1;
    let mut best = -1.0;
    for i in 1..NUM_COMPONENTS - 1 {
        let sum = amp[i - 1] + amp[i] + amp[i + 1];
        if sum > best {
            best = sum;
            k = i;
        }
    }

    // The five antennas have to be inside of the window.
    if !(2..=NUM_COMPONENTS - 3).contains(&k) {
        return f64::NAN;
    }

    let floor = num::min(power[k - 2], power[k + 2]);
    let mut sum = 0.0;
    let mut moment = 0.0;
    for o in -2i32..=2 {
        let p = power[(k as i32 + o) as usize];
        if p < 1.0 {
            return f64::NAN;
        }
        sum += p - floor;
        moment += f64::from(o) * (p - floor);
    }
    if sum <= 0.0 {
        return f64::NAN;
    }
    f64::from(i32::from(row.first) + k as i32) + (moment / sum)
}

/// The amplitude an antenna directly below the tip would measure: the vertex of a parabola
/// through the three amplitudes raised to `exponent`.
pub fn peak_amplitude(row: &Row, exponent: f64) -> f64 {
    let Some(amps) = core_amplitudes(row) else { return f64::NAN };
    let x = amps.map(|a| a.powf(exponent));

    let curvature = x[0] - (2.0 * x[1]) + x[2];
    if curvature <= 0.0 {
        return f64::NAN;
    }
    let d = num::clamp((x[0] - x[2]) / (2.0 * curvature), -0.5, 0.5);
    let peak = x[1] - ((x[0] - x[2]) * d / 4.0);
    if peak <= 0.0 {
        return f64::NAN;
    }
    peak.powf(1.0 / exponent)
}

/// The summed amplitude of the three antennas around the strongest one. Unlike the
/// interpolated peak it stays steady within a stroke of a tilted pen.
pub fn core_sum(row: &Row) -> f64 {
    match core_amplitudes(row) {
        Some(a) => {
            let mut sum = 0.0;
            for v in a {
                sum += v;
            }
            sum
        }
        None => f64::NAN,
    }
}

/// The strength of the tip signal relative to `reference`: the summed amplitude of three
/// antennas, averaged over both axes. Zero components count as they are.
pub fn tip_level(x: &Row, y: &Row, reference: f64) -> f64 {
    let mut sum = 0.0;
    for row in [x, y] {
        for i in CENTER - 1..=CENTER + 1 {
            sum += row.amplitude(i);
        }
    }
    sum / 2.0 / reference
}

/// The summed magnitude of a row over both axes, in the 32 bit arithmetic of the device data
/// (two very strong rows wrap around).
#[inline]
pub fn pair_magnitude(w: &DftWindow<'_>, row: usize) -> u32 {
    w.x[row].magnitude.wrapping_add(w.y[row].magnitude)
}

/// Reads a bit the pen sends as a tone in one of two rows: `Some(true)` if the tone is in
/// `row_set`, `Some(false)` if it is in `row_clear`, `None` if neither row reaches
/// `min_magnitude` per axis on average, or the window is too short.
pub fn tone_bit(w: &DftWindow<'_>, row_clear: usize, row_set: usize, min_magnitude: u32) -> Option<bool> {
    let last = row_clear.max(row_set);
    if w.x.len() <= last || w.y.len() <= last {
        return None;
    }
    let clear = u64::from(pair_magnitude(w, row_clear));
    let set = u64::from(pair_magnitude(w, row_set));
    let threshold = 2 * u64::from(min_magnitude);
    if clear < threshold && set < threshold {
        return None;
    }
    Some(clear < set)
}

/// The phase reference of a window: the summed center components of the first row of both axes.
#[inline]
pub fn center_phase(w: &DftWindow<'_>) -> (i32, i32) {
    (i32::from(w.x[0].real[CENTER]) + i32::from(w.y[0].real[CENTER]), i32::from(w.x[0].imag[CENTER]) + i32::from(w.y[0].imag[CENTER]))
}

#[cfg(test)]
pub(crate) mod testrows {
    use super::*;

    /// A row with real amplitudes around the center, the first antenna being number `first`.
    pub fn row(first: i8, amps: [i16; NUM_COMPONENTS], magnitude: u32) -> Row {
        Row { frequency: 0, magnitude, real: amps, imag: [0; NUM_COMPONENTS], first, last: first + 8, mid: first + 4, zero: 0 }
    }

    /// A tip-like bump whose true position is `center` (in antennas, within the window).
    pub fn bump(first: i8, center: f64, amplitude: f64, width: f64) -> Row {
        let mut amps = [0i16; NUM_COMPONENTS];
        for (i, a) in amps.iter_mut().enumerate() {
            let d = (f64::from(first) + i as f64 - center) / width;
            *a = (amplitude / (1.0 + d * d).powi(2)).round().max(1.0) as i16;
        }
        let magnitude = amps.iter().map(|&a| (a as i64 * a as i64) as u32).sum();
        row(first, amps, magnitude)
    }
}

#[cfg(test)]
mod tests {
    use super::testrows::*;
    use super::*;

    const EDGE: [f64; EDGE_COEFFICIENTS] = [0.20209, 0.85665, -0.28139, 0.21690, 0.11292, -0.04835, 0.06634];

    /// The row of a tip at `position` antennas from the low edge, mirrored for the high edge.
    fn edge_row(position: f64, amplitude: f64, edge: Edge) -> Row {
        let mut amps = [0i16; NUM_COMPONENTS];
        for i in 0..5 {
            let d = i as f64 - position;
            let a = (amplitude / (1.0 + d * d * 3.2).powf(1.43)).round().max(1.0) as i16;
            match edge {
                Edge::Low => amps[CENTER + i] = a,
                Edge::High => amps[CENTER - i] = a,
            }
        }
        row(if edge == Edge::Low { -4 } else { 63 }, amps, 1_000_000)
    }

    #[test]
    fn edge_estimates_rise_with_the_distance_from_the_edge_antenna() {
        for edge in [Edge::Low, Edge::High] {
            let mut last = (-1.0, -1.0);
            for step in 0..=9 {
                let r = edge_row(0.05 * f64::from(step), 2500.0, edge);
                let p = edge_parabola(&r, edge, 50.0, -0.7);
                let f = edge_fraction(&r, edge, 50.0, -0.7, &EDGE);
                assert!((0.0..=0.5).contains(&p) && (0.0..=0.5).contains(&f), "{p} {f}");
                assert!(p >= last.0 && f >= last.1, "step {step}: {p} {f} after {last:?}");
                last = (p, f);
            }
            // Near the next antenna the two agree.
            let r = edge_row(0.48, 2500.0, edge);
            assert_eq!(edge_fraction(&r, edge, 50.0, -0.7, &EDGE), edge_parabola(&r, edge, 50.0, -0.7));
        }
        // A weak tone is no pen, also at the edge.
        assert!(edge_parabola(&edge_row(0.1, 40.0, Edge::Low), Edge::Low, 50.0, -0.7).is_nan());
        assert!(edge_fraction(&edge_row(0.1, 40.0, Edge::Low), Edge::Low, 50.0, -0.7, &EDGE).is_nan());
        // The inner antennas may be far below the strongest one: still a position.
        let r = row(-4, [0, 0, 0, 0, 300, 20, 3, 0, 0], 0);
        assert!(edge_parabola(&r, Edge::Low, 50.0, -0.7) < 0.1);
    }

    #[test]
    fn positions_of_a_symmetric_signal() {
        let r = row(10, [1, 2, 10, 300, 1000, 300, 10, 2, 1], 0);
        assert_eq!(parabola_position(&r, 50.0, -0.7), 14.0);
        assert_eq!(centroid_position(&r, 0.6), 14.0);
        assert_eq!(ring_position(&r), 14.0);
        assert_eq!(core_sum(&r), 1600.0);
        assert_eq!(tip_level(&r, &r, 1600.0), 1.0);
        // The interpolated peak of a centered signal is the center amplitude.
        assert!((peak_amplitude(&r, -0.5) - 1000.0).abs() < 1e-9);
    }

    #[test]
    fn positions_between_antennas() {
        for center in [13.6, 13.9, 14.2, 14.45] {
            let r = bump(10, center, 3000.0, 1.0);
            // The strongest antenna has to be the middle one for these.
            assert!((parabola_position(&r, 50.0, -0.7) - center).abs() < 0.15, "{center}");
            assert!((centroid_position(&r, 0.6) - center).abs() < 0.15, "{center}");
        }
    }

    #[test]
    fn weak_signals_and_edges() {
        let weak = row(10, [0, 0, 0, 10, 40, 10, 0, 0, 0], 0);
        assert!(parabola_position(&weak, 50.0, -0.7).is_nan());

        // The left neighbour does not exist.
        let edge = row(-4, [0, 0, 0, 0, 1000, 400, 50, 0, 0], 0);
        assert_eq!(edge_of(&edge, 68), Some(Edge::Low));
        assert_eq!(edge_of(&row(63, [1; 9], 0), 68), Some(Edge::High));
        assert_eq!(edge_of(&row(62, [1; 9], 0), 68), None);
        assert!(centroid_position(&edge, 0.6).is_nan());
        assert!(peak_amplitude(&edge, -0.5).is_nan());
        assert!(core_sum(&edge).is_nan());

        // A neighbour in antiphase counts as no signal instead of breaking the fit.
        let tilted = row(10, [1, 2, 10, -200, 1000, 300, 10, 2, 1], 0);
        assert!(!parabola_position(&tilted, 50.0, -0.7).is_nan());

        // The ring signal needs five antennas inside of the window.
        assert!(ring_position(&row(10, [1000, 300, 10, 2, 1, 1, 1, 1, 1], 0)).is_nan());
    }

    #[test]
    fn tone_bits() {
        let strong = row(0, [0; 9], 200_000);
        let none = row(0, [0; 9], 100);
        let rows = [none, strong, none, none];
        let w = DftWindow { group: None, kind: 0, timestamp: 0, columns: 0, rows: 0, x: &rows, y: &rows };
        assert_eq!(tone_bit(&w, 2, 1, 10000), Some(true));
        assert_eq!(tone_bit(&w, 1, 2, 10000), Some(false));
        assert_eq!(tone_bit(&w, 2, 3, 10000), None);
        assert_eq!(tone_bit(&w, 3, 4, 10000), None);
        // The sum of both axes wraps at 32 bits.
        let huge = row(0, [0; 9], 0x8000_0000);
        let rows = [huge, strong];
        let w = DftWindow { group: None, kind: 0, timestamp: 0, columns: 0, rows: 0, x: &rows, y: &rows };
        assert_eq!(pair_magnitude(&w, 0), 0);
        assert_eq!(tone_bit(&w, 1, 0, 10000), Some(false));
    }
}
