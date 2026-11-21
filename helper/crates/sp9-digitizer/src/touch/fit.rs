// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
// Derived from iptsd (src/contacts/detection/algorithms/gaussian.hpp), GPL-2.0-or-later,
// Copyright (C) the iptsd authors.
//! Fits one two-dimensional Gaussian per cluster to the blurred heatmap.
//!
//! A blob is a Gaussian with a mean and a precision matrix (the inverse of its covariance), in
//! coordinates scaled to -1 .. 1 over the image. Where the windows of blobs overlap, each cell's
//! value is shared among them by their current estimates; then every blob is refitted to its
//! share by weighted least squares on the logarithm (a 6 x 6 linear system). Three rounds.
//!
//! The share is computed from `scale * exp(-d^T prec d) / 2`, which is the square of the
//! fitted Gaussian `exp(-d^T prec d / 2)`: overlapping blobs are separated more sharply than
//! their fitted shapes would.
//!
//! The order of the floating point operations is that of the reference implementation: the
//! results are compared bit for bit.

use super::detect::Rect;

/// Below this a pivot, a determinant or a value under the logarithm counts as zero.
const EPS: f64 = 1e-40;

const ROUNDS: usize = 3;

#[derive(Clone, Debug)]
pub struct Blob {
    pub valid: bool,
    pub scale: f64,
    pub mean: [f64; 2],
    /// The precision matrix (inverse covariance), row-major.
    pub prec: [[f64; 2]; 2],
    pub bounds: Rect,
    /// The share of this blob in every cell of its bounds, row-major.
    weights: Vec<f64>,
}

impl Blob {
    /// A blob at the center of a cluster, with unit precision in cell units.
    pub fn new(bounds: Rect) -> Blob {
        let mean = [(bounds.min_x as f64 + bounds.max_x as f64) / 2.0, (bounds.min_y as f64 + bounds.max_y as f64) / 2.0];
        Blob { valid: true, scale: 1.0, mean, prec: [[1.0, 0.0], [0.0, 1.0]], bounds, weights: vec![0.0; bounds.width() * bounds.height()] }
    }
}

/// `v^T m v` for a row-major 2 x 2 matrix.
#[inline]
fn quadratic_form(v: [f64; 2], m: &[[f64; 2]; 2]) -> f64 {
    let t0 = v[0] * m[0][0] + v[1] * m[1][0];
    let t1 = v[0] * m[0][1] + v[1] * m[1][1];
    t0 * v[0] + t1 * v[1]
}

/// Refines the blobs on `image` (row-major, `columns` wide). `total` is scratch space of the
/// size of the image. Blobs whose system cannot be solved come back invalid.
pub fn fit(blobs: &mut [Blob], image: &[f64], columns: usize, total: &mut [f64]) {
    let rows = image.len() / columns;
    let sx = 2.0 / columns as f64;
    let sy = 2.0 / rows as f64;

    for b in blobs.iter_mut().filter(|b| b.valid) {
        b.mean = [b.mean[0] * sx - 1.0, b.mean[1] * sy - 1.0];
        b.prec[0][0] /= sx;
        b.prec[0][1] /= sx;
        b.prec[1][0] /= sy;
        b.prec[1][1] /= sy;
        b.prec[0][0] /= sx;
        b.prec[1][0] /= sx;
        b.prec[0][1] /= sy;
        b.prec[1][1] /= sy;
    }

    for _ in 0..ROUNDS {
        share(blobs, columns, sx, sy, total);

        for b in blobs.iter_mut().filter(|b| b.valid) {
            let (m, rhs) = assemble(b, image, columns, sx, sy);
            b.valid = match solve(m, rhs) {
                Some(chi) => extract(&chi, b),
                None => false,
            };
        }
    }

    for b in blobs.iter_mut().filter(|b| b.valid) {
        b.mean = [(b.mean[0] + 1.0) / sx, (b.mean[1] + 1.0) / sy];
        b.prec[0][0] *= sx;
        b.prec[0][1] *= sx;
        b.prec[1][0] *= sy;
        b.prec[1][1] *= sy;
        b.prec[0][0] *= sx;
        b.prec[1][0] *= sx;
        b.prec[0][1] *= sy;
        b.prec[1][1] *= sy;
    }
}

/// Evaluates every blob in its window and divides by the sum over all blobs.
fn share(blobs: &mut [Blob], columns: usize, sx: f64, sy: f64, total: &mut [f64]) {
    total.fill(0.0);

    for b in blobs.iter_mut().filter(|b| b.valid) {
        let r = b.bounds;
        let mut i = 0;
        for iy in r.min_y..=r.max_y {
            let y = (iy as f64 * sy) - 1.0;
            for ix in r.min_x..=r.max_x {
                let x = (ix as f64 * sx) - 1.0;
                let d = [x - b.mean[0], y - b.mean[1]];
                b.weights[i] = b.scale * ((-quadratic_form(d, &b.prec)).exp() / 2.0);
                i += 1;
            }
        }
    }

    for b in blobs.iter().filter(|b| b.valid) {
        let r = b.bounds;
        let mut i = 0;
        for iy in r.min_y..=r.max_y {
            for ix in r.min_x..=r.max_x {
                total[iy as usize * columns + ix as usize] += b.weights[i];
                i += 1;
            }
        }
    }

    for b in blobs.iter_mut().filter(|b| b.valid) {
        let r = b.bounds;
        let mut i = 0;
        for iy in r.min_y..=r.max_y {
            for ix in r.min_x..=r.max_x {
                let t = total[iy as usize * columns + ix as usize];
                if t > 0.0 {
                    b.weights[i] /= t;
                }
                i += 1;
            }
        }
    }
}

/// The normal equations of the weighted fit of
/// `log(value) = chi0 x^2 + chi1 x y + chi2 y^2 + chi3 x + chi4 y + chi5`.
#[allow(clippy::needless_range_loop)]
fn assemble(b: &Blob, image: &[f64], columns: usize, sx: f64, sy: f64) -> ([[f64; 6]; 6], [f64; 6]) {
    let mut m = [[0.0f64; 6]; 6];
    let mut rhs = [0.0f64; 6];
    let r = b.bounds;
    let mut i = 0;

    for iy in r.min_y..=r.max_y {
        let y = (iy as f64 * sy) - 1.0;
        for ix in r.min_x..=r.max_x {
            let x = (ix as f64 * sx) - 1.0;
            let d = b.weights[i] * image[iy as usize * columns + ix as usize];
            i += 1;
            let v = (d + EPS).ln() * d * d;

            rhs[0] += v * x * x;
            rhs[1] += v * x * y;
            rhs[2] += v * y * y;
            rhs[3] += v * x;
            rhs[4] += v * y;
            rhs[5] += v;

            m[0][0] += d * d * x * x * x * x;
            m[0][1] += d * d * x * x * x * y;
            m[0][2] += d * d * x * x * y * y;
            m[0][3] += d * d * x * x * x;
            m[0][4] += d * d * x * x * y;
            m[0][5] += d * d * x * x;

            m[1][0] += d * d * x * x * x * y;
            m[1][1] += d * d * x * x * y * y;
            m[1][2] += d * d * x * y * y * y;
            m[1][3] += d * d * x * x * y;
            m[1][4] += d * d * x * y * y;
            m[1][5] += d * d * x * y;

            m[2][0] += d * d * x * x * y * y;
            m[2][1] += d * d * x * y * y * y;
            m[2][2] += d * d * y * y * y * y;
            m[2][3] += d * d * x * y * y;
            m[2][4] += d * d * y * y * y;
            m[2][5] += d * d * y * y;

            m[3][0] += d * d * x * x * x;
            m[3][1] += d * d * x * x * y;
            m[3][2] += d * d * x * y * y;
            m[3][3] += d * d * x * x;
            m[3][4] += d * d * x * y;
            m[3][5] += d * d * x;

            m[4][0] += d * d * x * x * y;
            m[4][1] += d * d * x * y * y;
            m[4][2] += d * d * y * y * y;
            m[4][3] += d * d * x * y;
            m[4][4] += d * d * y * y;
            m[4][5] += d * d * y;

            m[5][0] += d * d * x * x;
            m[5][1] += d * d * x * y;
            m[5][2] += d * d * y * y;
            m[5][3] += d * d * x;
            m[5][4] += d * d * y;
            m[5][5] += d * d;
        }
    }

    for c in 0..6 {
        m[1][c] *= 2.0;
    }
    (m, rhs)
}

/// Solves the system by elimination with pivoting.
///
/// The elimination runs over the transpose of the assembled matrix (the pivot search, the
/// swaps and the row operations all index `a[column][row]`), as the reference does; with the
/// doubled second row the matrix is not symmetric, so this is part of the result.
#[allow(clippy::needless_range_loop)]
fn solve(mut a: [[f64; 6]; 6], mut b: [f64; 6]) -> Option<[f64; 6]> {
    for c in 0..5 {
        let mut r = 0;
        let mut v = 0.0;
        for i in c..6 {
            let vi = a[c][i].abs();
            if v < vi {
                v = vi;
                r = i;
            }
        }
        if v <= EPS {
            return None;
        }
        if r != c {
            for i in c..6 {
                a[i].swap(r, c);
            }
            b.swap(r, c);
        }
        for r in c + 1..6 {
            let v = a[c][r] / a[c][c];
            b[r] -= v * b[c];
            for k in c + 1..6 {
                a[k][r] -= v * a[k][c];
            }
        }
    }

    if a[5][5].abs() <= EPS {
        return None;
    }

    let mut x = [0.0f64; 6];
    x[5] = b[5];
    x[5] /= a[5][5];
    x[4] = b[4] - (a[5][4] * x[5]);
    x[4] /= a[4][4];
    x[3] = b[3] - (a[5][3] * x[5]) - (a[4][3] * x[4]);
    x[3] /= a[3][3];
    x[2] = b[2] - (a[5][2] * x[5]) - (a[4][2] * x[4]) - (a[3][2] * x[3]);
    x[2] /= a[2][2];
    x[1] = b[1] - (a[5][1] * x[5]) - (a[4][1] * x[4]) - (a[3][1] * x[3]) - (a[2][1] * x[2]);
    x[1] /= a[1][1];
    x[0] = b[0] - (a[5][0] * x[5]) - (a[4][0] * x[4]) - (a[3][0] * x[3]) - (a[2][0] * x[2]) - (a[1][0] * x[1]);
    x[0] /= a[0][0];
    Some(x)
}

/// Turns the solution into scale, mean and precision. False if the precision is singular.
fn extract(chi: &[f64; 6], b: &mut Blob) -> bool {
    b.prec = [[chi[0] * -2.0, chi[1] * -2.0], [chi[1] * -2.0, chi[2] * -2.0]];
    let p = &b.prec;
    let det = p[0][0] * p[1][1] - p[1][0] * p[0][1];
    if det.abs() <= EPS {
        return false;
    }
    b.mean = [(p[1][1] * chi[3] - p[1][0] * chi[4]) / det, (p[0][0] * chi[4] - p[0][1] * chi[3]) / det];
    b.scale = (chi[5] + (quadratic_form(b.mean, &b.prec) / 2.0)).exp();
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gaussian_image(columns: usize, rows: usize, cx: f64, cy: f64, sx: f64, sy: f64, amp: f64) -> Vec<f64> {
        let mut img = vec![0.0; columns * rows];
        for y in 0..rows {
            for x in 0..columns {
                let dx = (x as f64 - cx) / sx;
                let dy = (y as f64 - cy) / sy;
                img[y * columns + x] = amp * (-0.5 * (dx * dx + dy * dy)).exp();
            }
        }
        img
    }

    #[test]
    fn recovers_a_gaussian() {
        let (columns, rows) = (20, 14);
        let img = gaussian_image(columns, rows, 9.3, 6.6, 1.5, 1.1, 0.4);
        let mut blobs = vec![Blob::new(Rect { min_x: 5, min_y: 3, max_x: 13, max_y: 10 })];
        let mut total = vec![0.0; img.len()];
        fit(&mut blobs, &img, columns, &mut total);
        let b = &blobs[0];
        assert!(b.valid);
        assert!((b.mean[0] - 9.3).abs() < 1e-6 && (b.mean[1] - 6.6).abs() < 1e-6, "{:?}", b.mean);
        // The precision is the inverse covariance.
        assert!((b.prec[0][0] - 1.0 / (1.5 * 1.5)).abs() < 1e-6, "{:?}", b.prec);
        assert!((b.prec[1][1] - 1.0 / (1.1 * 1.1)).abs() < 1e-6);
        assert!(b.prec[0][1].abs() < 1e-9);
    }

    #[test]
    fn a_flat_window_is_not_a_blob() {
        let img = vec![0.0; 100];
        let mut blobs = vec![Blob::new(Rect { min_x: 2, min_y: 2, max_x: 6, max_y: 6 })];
        fit(&mut blobs, &img, 10, &mut vec![0.0; 100]);
        assert!(!blobs[0].valid);
    }

    #[test]
    fn solve_matches_a_known_system() {
        // Diagonal: the transposed elimination does not matter.
        let mut a = [[0.0; 6]; 6];
        for (i, row) in a.iter_mut().enumerate() {
            row[i] = (i + 1) as f64;
        }
        assert_eq!(solve(a, [1.0, 4.0, 9.0, 16.0, 25.0, 36.0]), Some([1.0, 2.0, 3.0, 4.0, 5.0, 6.0]));
        assert_eq!(solve([[0.0; 6]; 6], [1.0; 6]), None);
    }
}
