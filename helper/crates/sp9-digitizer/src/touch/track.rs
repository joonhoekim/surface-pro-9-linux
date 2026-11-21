// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
// Derived from iptsd (src/contacts/tracking, stability, validation), GPL-2.0-or-later,
// Copyright (C) the iptsd authors.
//! Follows contacts from image to image.
//!
//! Three steps per image, each against the previous image:
//! 1. identity: the closest pairs of old and new detections keep their number,
//! 2. stability: small changes of size, position and orientation are dropped, large ones mark
//!    the contact as unstable for this image (it is then not reported),
//! 3. palm rejection: a contact outside of the size and aspect limits of a finger is invalid
//!    and stays so until it is lifted.

use super::detect::Detection;
use crate::config::{ContactsConfig, Geometry};
use crate::num;

/// A contact of the current image.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Contact {
    /// The number the contact keeps while it is on the screen (the multi-touch slot).
    pub id: usize,
    /// Position as a fraction of the sensor.
    pub x: f64,
    pub y: f64,
    /// The axes of the ellipse as fractions of the sensor diagonal.
    pub size: [f64; 2],
    /// Orientation as a fraction of half a turn.
    pub orientation: f64,
    /// False while the contact changes too fast to be reported.
    pub stable: bool,
    /// False for a palm.
    pub valid: bool,
}

impl Contact {
    pub fn major(&self) -> f64 {
        if self.size[0] < self.size[1] {
            self.size[1]
        } else {
            self.size[0]
        }
    }

    pub fn minor(&self) -> f64 {
        if self.size[1] < self.size[0] {
            self.size[1]
        } else {
            self.size[0]
        }
    }
}

/// Lower and upper limit of a change, or of a valid value.
#[derive(Clone, Copy, Debug)]
struct Limits {
    min: f64,
    max: f64,
}

#[derive(Debug)]
pub struct Tracker {
    size_change: Limits,
    position_change: Limits,
    orientation_change: Limits,
    size: Limits,
    aspect: Limits,
    /// The detections of the previous image as they were detected: (id, x, y).
    last_raw: Vec<(usize, f64, f64)>,
    /// The contacts of the previous image as they were reported.
    last: Vec<Contact>,
    distances: Vec<f64>,
}

/// The length of a vector, computed by scaling with the larger component.
fn scaled_hypot(dx: f64, dy: f64) -> f64 {
    let (x, y) = (dx.abs(), dy.abs());
    if x.is_infinite() || y.is_infinite() {
        return f64::INFINITY;
    }
    if x.is_nan() || y.is_nan() {
        return f64::NAN;
    }
    let p = num::max(x, y);
    if p == 0.0 {
        return 0.0;
    }
    let q = num::min(y, x) / p;
    p * (1.0 + q * q).sqrt()
}

impl Tracker {
    /// The configured lengths (cm) become fractions of the screen diagonal, the angles
    /// (degrees) fractions of half a turn.
    pub fn new(config: &ContactsConfig, geometry: &Geometry) -> Tracker {
        let diagonal = geometry.width_cm.hypot(geometry.height_cm);
        Tracker {
            size_change: Limits { min: config.size_change.min / diagonal, max: config.size_change.max / diagonal },
            position_change: Limits { min: config.position_change.min / diagonal, max: config.position_change.max / diagonal },
            orientation_change: Limits { min: config.orientation_change.min / 180.0, max: config.orientation_change.max / 180.0 },
            size: Limits { min: config.size.min / diagonal, max: config.size.max / diagonal },
            aspect: Limits { min: config.aspect.min, max: config.aspect.max },
            last_raw: Vec::new(),
            last: Vec::new(),
            distances: Vec::new(),
        }
    }

    /// Changes the limits; the contacts on the screen keep their numbers and their history.
    pub fn set_config(&mut self, config: &ContactsConfig, geometry: &Geometry) {
        let fresh = Tracker::new(config, geometry);
        self.size_change = fresh.size_change;
        self.position_change = fresh.position_change;
        self.orientation_change = fresh.orientation_change;
        self.size = fresh.size;
        self.aspect = fresh.aspect;
    }

    /// Turns the detections of an image into contacts, in the order of the detections.
    pub fn update(&mut self, detections: &[Detection], out: &mut Vec<Contact>) {
        out.clear();

        const NEW: usize = usize::MAX;
        out.extend(detections.iter().map(|d| Contact {
            id: NEW,
            x: d.x,
            y: d.y,
            size: d.size,
            orientation: d.orientation,
            stable: true,
            valid: true,
        }));

        self.match_previous(out);

        // The contacts that are new get the lowest numbers that neither this image nor the
        // previous one uses (the slot of a contact that was just lifted is released first).
        let mut next = 0;
        for i in 0..out.len() {
            if out[i].id != NEW {
                continue;
            }
            while out.iter().any(|c| c.id == next) || self.last_raw.iter().any(|&(id, _, _)| id == next) {
                next += 1;
            }
            out[i].id = next;
        }

        self.last_raw.clear();
        self.last_raw.extend(out.iter().map(|c| (c.id, c.x, c.y)));

        for c in out.iter_mut() {
            if let Some(last) = self.last.iter().find(|l| l.id == c.id) {
                self.stabilize(c, last);
            }
        }

        for c in out.iter_mut() {
            c.valid = self.is_finger(c);
        }

        self.last.clear();
        self.last.extend_from_slice(out);
    }

    /// Hands the numbers of the previous image to the closest new detections: the closest
    /// pair first, then the closest of the remaining ones, and so on.
    fn match_previous(&mut self, contacts: &mut [Contact]) {
        let new = contacts.len();
        let old = self.last_raw.len();
        if old == 0 {
            return;
        }

        // One row per old detection, one column per new one.
        self.distances.clear();
        for &(_, ox, oy) in &self.last_raw {
            for c in contacts.iter() {
                self.distances.push(scaled_hypot(c.x - ox, c.y - oy));
            }
        }

        for _ in 0..new.min(old) {
            // The smallest distance; among equals the first one column by column.
            let (mut row, mut col) = (0, 0);
            let mut best = self.distances[0];
            for j in 0..new {
                for i in 0..old {
                    let d = self.distances[i * new + j];
                    if d < best {
                        best = d;
                        row = i;
                        col = j;
                    }
                }
            }

            contacts[col].id = self.last_raw[row].0;

            for j in 0..new {
                self.distances[row * new + j] = f64::INFINITY;
            }
            for i in 0..old {
                self.distances[i * new + col] = f64::INFINITY;
            }
        }
    }

    fn stabilize(&self, c: &mut Contact, last: &Contact) {
        for axis in 0..2 {
            let delta = (c.size[axis] - last.size[axis]).abs();
            if delta < self.size_change.min {
                c.size[axis] = last.size[axis];
            } else if delta > self.size_change.max {
                c.stable = false;
            }
        }

        let distance = (c.x - last.x).hypot(c.y - last.y);
        if distance < self.position_change.min {
            c.x = last.x;
            c.y = last.y;
        } else if distance > self.position_change.max {
            c.stable = false;
        }

        // The orientation of a nearly round contact is noise.
        if c.major() / c.minor() < 1.1 {
            c.orientation = 0.0;
            return;
        }

        // The shorter way around: 0 and 1 are the same orientation.
        let d1 = (c.orientation - last.orientation).abs();
        let delta = num::min(d1, 1.0 - d1);
        if delta < self.orientation_change.min {
            c.orientation = last.orientation;
        } else if delta > self.orientation_change.max {
            c.stable = false;
        }
    }

    fn is_finger(&self, c: &Contact) -> bool {
        let before = self.last.iter().find(|l| l.id == c.id);
        // An unstable contact is not judged (and not reported either): it stays what it was.
        if !c.stable {
            return before.is_none_or(|l| l.valid);
        }
        // Once a palm, always a palm.
        if before.is_some_and(|l| !l.valid) {
            return false;
        }
        let major = c.major();
        if !(major >= self.size.min.min(self.size.max) && major <= self.size.min.max(self.size.max)) {
            return false;
        }
        let aspect = major / c.minor();
        aspect >= self.aspect.min.min(self.aspect.max) && aspect <= self.aspect.min.max(self.aspect.max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn tracker() -> Tracker {
        Tracker::new(&Config::default().touch.contacts, &Geometry::SP9)
    }

    fn finger(x: f64, y: f64) -> Detection {
        Detection { x, y, size: [0.02, 0.025], orientation: 0.3 }
    }

    #[test]
    fn ids_follow_the_contacts() {
        let mut t = tracker();
        let mut out = Vec::new();
        t.update(&[finger(0.2, 0.2), finger(0.7, 0.7)], &mut out);
        assert_eq!(out.iter().map(|c| c.id).collect::<Vec<_>>(), vec![0, 1]);

        // The detections change their order, a third one appears: the lowest free number.
        t.update(&[finger(0.71, 0.7), finger(0.5, 0.1), finger(0.2, 0.21)], &mut out);
        assert_eq!(out.iter().map(|c| c.id).collect::<Vec<_>>(), vec![1, 2, 0]);

        // The first one is lifted: the others keep their numbers. Its number is free again
        // one image later.
        t.update(&[finger(0.71, 0.7), finger(0.5, 0.1)], &mut out);
        assert_eq!(out.iter().map(|c| c.id).collect::<Vec<_>>(), vec![1, 2]);
        t.update(&[finger(0.71, 0.7), finger(0.5, 0.1), finger(0.9, 0.9)], &mut out);
        assert_eq!(out.iter().map(|c| c.id).collect::<Vec<_>>(), vec![1, 2, 0]);

        // Two more at once: the lowest numbers that are free.
        t.update(&[finger(0.71, 0.7), finger(0.5, 0.1)], &mut out);
        t.update(&[finger(0.71, 0.7), finger(0.5, 0.1), finger(0.1, 0.9), finger(0.3, 0.9)], &mut out);
        assert_eq!(out.iter().map(|c| c.id).collect::<Vec<_>>(), vec![1, 2, 0, 3]);
    }

    #[test]
    fn small_movements_are_dropped_and_jumps_are_unstable() {
        let mut t = tracker();
        let mut out = Vec::new();
        t.update(&[finger(0.5, 0.5)], &mut out);
        // 0.04 cm of 32.9 cm diagonal is 0.0012.
        t.update(&[finger(0.5005, 0.5)], &mut out);
        assert_eq!((out[0].x, out[0].stable), (0.5, true));
        t.update(&[finger(0.51, 0.5)], &mut out);
        assert_eq!((out[0].x, out[0].stable), (0.51, true));
        t.update(&[finger(0.7, 0.5)], &mut out);
        assert!(!out[0].stable);
        assert!(out[0].valid);
    }

    #[test]
    fn a_palm_stays_a_palm() {
        let mut t = tracker();
        let mut out = Vec::new();
        let palm = Detection { x: 0.5, y: 0.5, size: [0.05, 0.2], orientation: 0.2 };
        t.update(&[palm], &mut out);
        assert!(!out[0].valid);
        // It shrinks to the size of a finger: within the size change limit this is stable.
        t.update(&[Detection { size: [0.05, 0.19], ..palm }], &mut out);
        assert!(out[0].stable && !out[0].valid);
        // A jump (unstable) does not make it a finger again.
        t.update(&[Detection { x: 0.6, size: [0.02, 0.025], ..palm }], &mut out);
        assert!(!out[0].stable && !out[0].valid);
        t.update(&[Detection { x: 0.6, size: [0.02, 0.025], ..palm }], &mut out);
        assert!(out[0].stable && !out[0].valid);
        t.update(&[], &mut out);
        t.update(&[finger(0.5, 0.5)], &mut out);
        assert!(out[0].valid);
    }

    #[test]
    fn round_contacts_have_no_orientation() {
        let mut t = tracker();
        let mut out = Vec::new();
        t.update(&[Detection { x: 0.5, y: 0.5, size: [0.02, 0.021], orientation: 0.4 }], &mut out);
        assert_eq!(out[0].orientation, 0.4);
        t.update(&[Detection { x: 0.5, y: 0.5, size: [0.02, 0.021], orientation: 0.4 }], &mut out);
        assert_eq!(out[0].orientation, 0.0);
    }
}
