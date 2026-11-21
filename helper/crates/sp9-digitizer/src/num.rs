// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
//! Comparisons with the operand order of the reference implementation.
//!
//! `f64::min`, `f64::max` and `f64::clamp` return the other operand for a NaN (or panic); the
//! processing relies on the plain `a < b` forms below, where a NaN passes through or is dropped
//! depending on its position.

/// The smaller value; `a` when the two do not compare.
#[inline]
pub fn min(a: f64, b: f64) -> f64 {
    if b < a {
        b
    } else {
        a
    }
}

/// The larger value; `a` when the two do not compare.
#[inline]
pub fn max(a: f64, b: f64) -> f64 {
    if a < b {
        b
    } else {
        a
    }
}

/// `v` limited to `lo ..= hi`; a NaN stays a NaN.
#[inline]
pub fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    if v < lo {
        lo
    } else if hi < v {
        hi
    } else {
        v
    }
}

/// The smallest of three values, the first one among equals.
#[inline]
pub fn min3(a: f64, b: f64, c: f64) -> f64 {
    let mut m = a;
    if b < m {
        m = b;
    }
    if c < m {
        m = c;
    }
    m
}

/// The largest of three values, the first one among equals.
#[inline]
pub fn max3(a: f64, b: f64, c: f64) -> f64 {
    let mut m = a;
    if m < b {
        m = b;
    }
    if m < c {
        m = c;
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nan_positions() {
        assert!(min(f64::NAN, 1.0).is_nan());
        assert_eq!(min(1.0, f64::NAN), 1.0);
        assert!(max(f64::NAN, 1.0).is_nan());
        assert_eq!(max(1.0, f64::NAN), 1.0);
        assert!(clamp(f64::NAN, 0.0, 1.0).is_nan());
        assert_eq!(clamp(-2.0, 0.0, 1.0), 0.0);
        assert_eq!(clamp(2.0, 0.0, 1.0), 1.0);
        assert_eq!(min3(3.0, 1.0, 2.0), 1.0);
        assert_eq!(max3(3.0, 1.0, 5.0), 5.0);
    }
}
