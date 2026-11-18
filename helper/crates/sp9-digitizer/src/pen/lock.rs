// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
// Derived from iptsd (src/core/generic/dft.hpp) with the pen patches of this repository,
// GPL-2.0-or-later, Copyright (C) the iptsd authors.
//! Stage "lock": holds the reported position of a pen that rests on the glass.
//!
//! The position of a resting pen still changes by some hundredths of a millimeter from sample
//! to sample. Once it stayed within a radius for a number of samples it is frozen, until a
//! sample falls outside of that radius. Only while in contact.

use crate::config::LockConfig;

#[derive(Clone, Debug)]
pub struct LockStage {
    radius: f64,
    count: u32,

    /// Whether the last sample was in contact: the lock starts anew when that changes.
    contact: bool,
    anchored: bool,
    held: bool,
    resting: u32,
    /// Where the radius is measured from, and the last position that was passed on.
    anchor: (f64, f64),
    last: (f64, f64),
}

impl LockStage {
    pub fn new(config: &LockConfig) -> LockStage {
        LockStage {
            radius: config.radius,
            count: config.count,
            contact: false,
            anchored: false,
            held: false,
            resting: 0,
            anchor: (0.0, 0.0),
            last: (0.0, 0.0),
        }
    }

    pub fn set_config(&mut self, config: &LockConfig) {
        self.radius = config.radius;
        self.count = config.count;
    }

    pub fn enabled(&self) -> bool {
        self.radius > 0.0
    }

    /// Passes a position (millimeters) through the lock and returns the one to report.
    pub fn apply(&mut self, mut x: f64, mut y: f64, contact: bool) -> (f64, f64) {
        if contact != self.contact || !contact {
            self.anchored = false;
        }
        self.contact = contact;
        if !contact {
            return (x, y);
        }

        let dx = x - self.anchor.0;
        let dy = y - self.anchor.1;

        if !self.anchored || (dx * dx) + (dy * dy) >= self.radius * self.radius {
            self.anchored = true;
            self.held = false;
            self.resting = 0;
            self.anchor = (x, y);
            self.last = (x, y);
            return (x, y);
        }

        if self.resting + 1 < self.count {
            self.resting += 1;
        } else if !self.held {
            // Hold the position that was reported last, so that nothing jumps.
            self.anchor = self.last;
            self.held = true;
        }

        if self.held {
            (x, y) = self.anchor;
        } else {
            self.last = (x, y);
        }
        (x, y)
    }

    /// The pen left.
    pub fn reset(&mut self) {
        self.anchored = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn holds_a_resting_pen_and_lets_go_when_it_moves() {
        let mut lock = LockStage::new(&LockConfig { radius: 0.158, count: 6 });
        assert_eq!(lock.apply(10.0, 10.0, true), (10.0, 10.0));
        for i in 1..=5 {
            let p = 10.0 + 0.01 * f64::from(i);
            assert_eq!(lock.apply(p, 10.0, true), (p, 10.0), "sample {i}");
        }
        // The sixth resting sample: held at the last reported position.
        assert_eq!(lock.apply(10.02, 10.0, true), (10.05, 10.0));
        assert_eq!(lock.apply(10.1, 10.1, true), (10.05, 10.0));
        // Outside of the radius around the held position.
        assert_eq!(lock.apply(10.3, 10.0, true), (10.3, 10.0));
        assert_eq!(lock.apply(10.31, 10.0, true), (10.31, 10.0));
    }

    #[test]
    fn hover_is_not_held_and_contact_starts_anew() {
        let mut lock = LockStage::new(&LockConfig { radius: 0.158, count: 1 });
        for _ in 0..10 {
            assert_eq!(lock.apply(5.0, 5.0, false), (5.0, 5.0));
        }
        lock.apply(5.0, 5.0, true);
        assert_eq!(lock.apply(5.01, 5.0, true), (5.0, 5.0));
        lock.apply(5.01, 5.0, false);
        assert_eq!(lock.apply(5.01, 5.0, true), (5.01, 5.0));
    }
}
