// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
// Derived from iptsd (src/contacts/detection/*, src/core/generic/application.hpp),
// GPL-2.0-or-later, Copyright (C) the iptsd authors.
//! Finds the contacts of one heatmap.
//!
//! The image is inverted and scaled to 0 .. 1, its neutral level (the most frequent value,
//! refreshed every 16 images) is subtracted and it is blurred with a 3 x 3 Gaussian. Every
//! local maximum above the activation threshold grows a cluster over the connected cells
//! above the deactivation threshold; clusters that overlap by half are merged; a Gaussian is
//! fitted to each ([`super::fit`]) and its covariance gives position, size and orientation.

use super::fit::{self, Blob};
use crate::frame::Heatmap;

/// The neutral level is recomputed once per this many images.
const NEUTRAL_INTERVAL: u32 = 16;

/// Rounds of merging overlapping clusters.
const MERGE_ROUNDS: usize = 5;

/// An inclusive rectangle of cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub min_x: isize,
    pub min_y: isize,
    pub max_x: isize,
    pub max_y: isize,
}

impl Rect {
    pub fn width(&self) -> usize {
        (self.max_x - self.min_x + 1) as usize
    }

    pub fn height(&self) -> usize {
        (self.max_y - self.min_y + 1) as usize
    }

    fn area(&self) -> isize {
        (self.max_x - self.min_x + 1) * (self.max_y - self.min_y + 1)
    }

    fn union(&self, o: &Rect) -> Rect {
        Rect {
            min_x: self.min_x.min(o.min_x),
            min_y: self.min_y.min(o.min_y),
            max_x: self.max_x.max(o.max_x),
            max_y: self.max_y.max(o.max_y),
        }
    }

    /// Intersection over union, 0 for rectangles that do not intersect.
    fn overlap(&self, o: &Rect) -> f64 {
        if self == o {
            return 1.0;
        }
        let i = Rect {
            min_x: self.min_x.max(o.min_x),
            min_y: self.min_y.max(o.min_y),
            max_x: self.max_x.min(o.max_x),
            max_y: self.max_y.min(o.max_y),
        };
        if i.min_x > i.max_x || i.min_y > i.max_y {
            return 0.0;
        }
        i.area() as f64 / (self.area() + o.area() - i.area()) as f64
    }
}

/// A contact as detected in one image, before it is tracked. Position and size are fractions
/// of the sensor (antenna 0 .. last antenna, and the diagonal of that), the orientation is a
/// fraction of half a turn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Detection {
    pub x: f64,
    pub y: f64,
    /// The two axes of the ellipse, the one of the smaller eigenvalue first.
    pub size: [f64; 2],
    pub orientation: f64,
}

#[derive(Debug)]
pub struct Detector {
    activation: f64,
    deactivation: f64,
    neutral_offset: f64,
    neutral: f64,
    until_neutral: u32,
    kernel: [f64; 9],
    image: Vec<f64>,
    blurred: Vec<f64>,
    scratch: Vec<f64>,
    visited: Vec<bool>,
    stack: Vec<(usize, usize, f64)>,
    maxima: Vec<(usize, usize)>,
    clusters: Vec<Rect>,
    merged: Vec<Rect>,
    pairs: Vec<(usize, usize)>,
    blobs: Vec<Blob>,
}

/// A 3 x 3 Gaussian kernel with sigma 0.75, normalized to sum 1.
fn blur_kernel() -> [f64; 9] {
    let sigma = 0.75f64;
    let mut k = [0.0; 9];
    let mut sum = 0.0;
    for y in 0..3 {
        let vy = y as f64 - 1.0;
        for x in 0..3 {
            let vx = x as f64 - 1.0;
            let norm = (vy / sigma) * (vy / sigma) + (vx / sigma) * (vx / sigma);
            let v = (-0.5 * norm).exp();
            k[y * 3 + x] = v;
            sum += v;
        }
    }
    k.map(|v| v / sum)
}

impl Detector {
    /// Thresholds and offset in heatmap counts (0 .. 255).
    pub fn new(activation: f64, deactivation: f64, neutral_offset: f64) -> Detector {
        Detector {
            activation: activation / 255.0,
            deactivation: deactivation / 255.0,
            neutral_offset: neutral_offset / 255.0,
            neutral: 0.0,
            until_neutral: 0,
            kernel: blur_kernel(),
            image: Vec::new(),
            blurred: Vec::new(),
            scratch: Vec::new(),
            visited: Vec::new(),
            stack: Vec::new(),
            maxima: Vec::new(),
            clusters: Vec::new(),
            merged: Vec::new(),
            pairs: Vec::new(),
            blobs: Vec::new(),
        }
    }

    /// Changes the thresholds (heatmap counts); the neutral level and its timing stay.
    pub fn set_thresholds(&mut self, activation: f64, deactivation: f64, neutral_offset: f64) {
        self.activation = activation / 255.0;
        self.deactivation = deactivation / 255.0;
        self.neutral_offset = neutral_offset / 255.0;
    }

    /// Replaces `out` with the contacts of the image. The image must not be empty.
    pub fn detect(&mut self, heatmap: &Heatmap<'_>, out: &mut Vec<Detection>) {
        let columns = usize::from(heatmap.columns);
        let rows = usize::from(heatmap.rows);
        let cells = columns * rows;
        out.clear();

        // Low values are touches: invert, and scale the value range to 0 .. 1.
        let min = f64::from(heatmap.min);
        let span = f64::from(heatmap.max) - min;
        let level = |v: u8| 1.0 - ((f64::from(v) - min) / span);

        if self.until_neutral == 0 {
            self.neutral = level(mode(heatmap.data)) + self.neutral_offset;
        }
        self.until_neutral = (self.until_neutral + 1) % NEUTRAL_INTERVAL;

        self.image.clear();
        self.image.extend(heatmap.data.iter().map(|&v| {
            let d = level(v) - self.neutral;
            if d > 0.0 {
                d
            } else {
                0.0
            }
        }));

        self.blurred.resize(cells, 0.0);
        blur(&self.image, columns, rows, &self.kernel, &mut self.blurred);

        find_maxima(&self.blurred, columns, rows, self.activation, &mut self.maxima);

        self.clusters.clear();
        for i in 0..self.maxima.len() {
            let (x, y) = self.maxima[i];
            let c = self.grow(x, y, columns, rows);
            // One more cell on every side, inside of the image.
            let c = Rect {
                min_x: (c.min_x - 1).max(0),
                min_y: (c.min_y - 1).max(0),
                max_x: (c.max_x + 1).min(columns as isize - 1),
                max_y: (c.max_y + 1).min(rows as isize - 1),
            };
            // The fit needs 3 x 3 cells.
            if c.width() >= 3 && c.height() >= 3 {
                self.clusters.push(c);
            }
        }

        self.merge();

        self.blobs.clear();
        self.blobs.extend(self.clusters.iter().map(|&c| Blob::new(c)));
        self.scratch.resize(cells, 0.0);
        fit::fit(&mut self.blobs, &self.blurred, columns, &mut self.scratch);

        let diagonal = ((columns - 1) as f64).hypot((rows - 1) as f64);

        for b in self.blobs.iter().filter(|b| b.valid) {
            let (values, first) = eigen(&invert(&b.prec));
            let size = [2.0 * values[0].abs().sqrt(), 2.0 * values[1].abs().sqrt()];

            // The direction of the first axis, as an angle in 0 .. pi.
            let mut angle = first[0].atan2(first[1]);
            if angle < 0.0 {
                angle += std::f64::consts::PI;
            } else if angle >= std::f64::consts::PI {
                angle -= std::f64::consts::PI;
            }

            out.push(Detection {
                x: b.mean[0] / (columns - 1) as f64,
                y: b.mean[1] / (rows - 1) as f64,
                size: [size[0] / diagonal, size[1] / diagonal],
                orientation: angle / std::f64::consts::PI,
            });
        }
    }

    /// The bounding box of the cells that are connected to a maximum: above the deactivation
    /// threshold, and not rising again once the level fell to the activation threshold (so
    /// that two contacts do not join through the valley between them).
    fn grow(&mut self, x: usize, y: usize, columns: usize, rows: usize) -> Rect {
        let mut c = Rect { min_x: x as isize, min_y: y as isize, max_x: x as isize, max_y: y as isize };
        self.visited.clear();
        self.visited.resize(columns * rows, false);
        self.stack.clear();
        self.stack.push((x, y, f64::MAX));

        while let Some((x, y, previous)) = self.stack.pop() {
            let i = y * columns + x;
            let value = self.blurred[i];
            if value <= self.deactivation {
                continue;
            }
            if previous <= self.activation && value > previous {
                continue;
            }
            if self.visited[i] {
                continue;
            }
            self.visited[i] = true;

            c.min_x = c.min_x.min(x as isize);
            c.max_x = c.max_x.max(x as isize);
            c.min_y = c.min_y.min(y as isize);
            c.max_y = c.max_y.max(y as isize);

            if x + 1 < columns {
                self.stack.push((x + 1, y, value));
            }
            if x > 0 {
                self.stack.push((x - 1, y, value));
            }
            if y + 1 < rows {
                self.stack.push((x, y + 1, value));
            }
            if y > 0 {
                self.stack.push((x, y - 1, value));
            }
        }
        c
    }

    /// Merges clusters that overlap by half or more, the later one into the earlier one.
    fn merge(&mut self) {
        for _ in 0..MERGE_ROUNDS {
            self.pairs.clear();
            for i in 0..self.clusters.len() {
                for j in i + 1..self.clusters.len() {
                    if self.clusters[i].overlap(&self.clusters[j]) >= 0.5 {
                        self.pairs.push((i, j));
                    }
                }
            }
            if self.pairs.is_empty() {
                break;
            }

            self.merged.clear();
            'cluster: for i in 0..self.clusters.len() {
                for &(a, b) in &self.pairs {
                    // The second of a pair went into the first one. Pairs listed before
                    // this one have still extended the cluster, which stays visible to
                    // the clusters after it.
                    if b == i {
                        continue 'cluster;
                    }
                    if a == i {
                        self.clusters[i] = self.clusters[i].union(&self.clusters[b]);
                    }
                }
                self.merged.push(self.clusters[i]);
            }
            std::mem::swap(&mut self.clusters, &mut self.merged);
        }
    }
}

/// The most frequent value, the one that reaches its count first among equals.
fn mode(data: &[u8]) -> u8 {
    let mut counts = [0u32; 256];
    let mut best = 0u8;
    let mut best_count = 0;
    for &v in data {
        let c = &mut counts[usize::from(v)];
        *c += 1;
        if *c > best_count {
            best_count = *c;
            best = v;
        }
    }
    best
}

/// 3 x 3 convolution with the border cells repeated outwards.
fn blur(src: &[f64], columns: usize, rows: usize, kernel: &[f64; 9], dst: &mut [f64]) {
    for y in 0..rows {
        let ys = [y.saturating_sub(1), y, (y + 1).min(rows - 1)];
        for x in 0..columns {
            let xs = [x.saturating_sub(1), x, (x + 1).min(columns - 1)];
            let mut v = 0.0;
            for ky in 0..3 {
                for kx in 0..3 {
                    v += src[ys[ky] * columns + xs[kx]] * kernel[ky * 3 + kx];
                }
            }
            dst[y * columns + x] = v;
        }
    }
}

/// Local maxima above a threshold. Of equal neighbours the first one in reading order counts,
/// so a plateau gives exactly one maximum.
fn find_maxima(data: &[f64], columns: usize, rows: usize, threshold: f64, out: &mut Vec<(usize, usize)>) {
    out.clear();
    let at = |x: usize, y: usize| data[y * columns + x];
    for y in 0..rows {
        let up = y > 0;
        let down = y + 1 < rows;
        for x in 0..columns {
            let v = at(x, y);
            if v <= threshold {
                continue;
            }
            let left = x > 0;
            let right = x + 1 < columns;
            let mut max = true;
            if left {
                max &= at(x - 1, y) < v;
            }
            if right {
                max &= at(x + 1, y) <= v;
            }
            if up {
                max &= at(x, y - 1) < v;
                if left {
                    max &= at(x - 1, y - 1) < v;
                }
                if right {
                    max &= at(x + 1, y - 1) <= v;
                }
            }
            if down {
                max &= at(x, y + 1) <= v;
                if left {
                    max &= at(x - 1, y + 1) < v;
                }
                if right {
                    max &= at(x + 1, y + 1) <= v;
                }
            }
            if max {
                out.push((x, y));
            }
        }
    }
}

/// The inverse of a 2 x 2 matrix.
fn invert(m: &[[f64; 2]; 2]) -> [[f64; 2]; 2] {
    let invdet = 1.0 / (m[0][0] * m[1][1] - m[1][0] * m[0][1]);
    [[m[1][1] * invdet, -m[0][1] * invdet], [-m[1][0] * invdet, m[0][0] * invdet]]
}

/// Eigenvalues (ascending) of a symmetric 2 x 2 matrix and the eigenvector of the smaller
/// one, in closed form. Only the lower triangle is read.
///
/// The steps (shift by the mean eigenvalue, scale by the largest coefficient, the choice of
/// the better conditioned row) are those of the closed-form solver of the linear algebra
/// library the reference uses; the results are compared bit for bit.
fn eigen(m: &[[f64; 2]; 2]) -> ([f64; 2], [f64; 2]) {
    let shift = (m[0][0] + m[1][1]) / 2.0;
    let mut a = m[0][0] - shift;
    let b = m[1][0];
    let mut c = m[1][1] - shift;
    let mut off = b;

    let mut scale = a.abs();
    for v in [off.abs(), off.abs(), c.abs()] {
        if scale < v {
            scale = v;
        }
    }
    if scale > 0.0 {
        a /= scale;
        off /= scale;
        c /= scale;
    }

    let t0 = 0.5 * ((a - c) * (a - c) + 4.0 * (off * off)).sqrt();
    let t1 = 0.5 * (a + c);
    let values = [t1 - t0, t1 + t0];

    let first = if (values[1] - values[0]) <= values[1].abs() * f64::EPSILON {
        [1.0, 0.0]
    } else {
        a -= values[1];
        c -= values[1];
        let a2 = a * a;
        let c2 = c * c;
        let b2 = off * off;
        // The eigenvector of the larger eigenvalue, from the row with the larger diagonal.
        let second = if a2 > c2 {
            let n = (a2 + b2).sqrt();
            [-off / n, a / n]
        } else {
            let n = (c2 + b2).sqrt();
            [-c / n, off / n]
        };
        // The other one is orthogonal to it.
        let o = [-second[1], second[0]];
        let z = o[0] * o[0] + o[1] * o[1];
        if z > 0.0 {
            let n = z.sqrt();
            [o[0] / n, o[1] / n]
        } else {
            o
        }
    };

    ([values[0] * scale + shift, values[1] * scale + shift], first)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kernel_sums_to_one() {
        let k = blur_kernel();
        assert!((k.iter().sum::<f64>() - 1.0).abs() < 1e-15);
        assert!(k[4] > k[1] && k[1] > k[0]);
        assert_eq!(k[1], k[3]);
    }

    #[test]
    fn mode_prefers_the_first_to_reach_the_count() {
        assert_eq!(mode(&[3, 5, 5, 3]), 5);
        assert_eq!(mode(&[3, 5, 3, 5]), 3);
    }

    #[test]
    fn maxima_on_a_plateau() {
        let mut data = vec![0.0; 25];
        data[6] = 1.0;
        data[7] = 1.0;
        data[12] = 1.0;
        let mut out = Vec::new();
        find_maxima(&data, 5, 5, 0.5, &mut out);
        assert_eq!(out, vec![(1, 1)]);
    }

    #[test]
    fn rect_overlap() {
        let a = Rect { min_x: 0, min_y: 0, max_x: 3, max_y: 3 };
        let b = Rect { min_x: 2, min_y: 0, max_x: 5, max_y: 3 };
        assert_eq!(a.overlap(&a), 1.0);
        assert_eq!(a.overlap(&b), 8.0 / 24.0);
        assert_eq!(a.overlap(&Rect { min_x: 4, min_y: 4, max_x: 5, max_y: 5 }), 0.0);
    }

    #[test]
    fn eigen_of_a_diagonal_and_a_rotated_matrix() {
        let (values, first) = eigen(&[[4.0, 0.0], [0.0, 1.0]]);
        assert_eq!(values, [1.0, 4.0]);
        assert_eq!((first[0].abs(), first[1].abs()), (0.0, 1.0));

        let (values, first) = eigen(&[[2.5, 1.5], [1.5, 2.5]]);
        assert!((values[0] - 1.0).abs() < 1e-12 && (values[1] - 4.0).abs() < 1e-12);
        assert!((first[0] + first[1]).abs() < 1e-12);

        assert_eq!(eigen(&[[2.0, 0.0], [0.0, 2.0]]), ([2.0, 2.0], [1.0, 0.0]));
    }

    fn finger(columns: usize, rows: usize, cx: f64, cy: f64) -> Vec<u8> {
        let mut img = vec![200u8; columns * rows];
        for y in 0..rows {
            for x in 0..columns {
                let d2 = (x as f64 - cx).powi(2) + (y as f64 - cy).powi(2);
                img[y * columns + x] = (200.0 - 120.0 * (-d2 / 3.0).exp()).round() as u8;
            }
        }
        img
    }

    #[test]
    fn one_finger() {
        let (columns, rows) = (68usize, 46usize);
        let img = finger(columns, rows, 30.4, 20.7);
        let mut d = Detector::new(24.0, 20.0, 0.0);
        let mut out = Vec::new();
        d.detect(&Heatmap { rows: rows as u8, columns: columns as u8, min: 0, max: 255, data: &img }, &mut out);
        assert_eq!(out.len(), 1);
        assert!((out[0].x * 67.0 - 30.4).abs() < 0.05 && (out[0].y * 45.0 - 20.7).abs() < 0.05, "{:?}", out[0]);
        assert!(out[0].size[0] > 0.0 && out[0].size[1] / out[0].size[0] < 1.1);

        // Nothing on a flat image, and nothing from a contact that is weaker than the threshold.
        d.detect(&Heatmap { rows: rows as u8, columns: columns as u8, min: 0, max: 255, data: &vec![200; columns * rows] }, &mut out);
        assert!(out.is_empty());
    }
}
