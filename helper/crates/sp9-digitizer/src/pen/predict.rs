// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
//! Stage "prediction": reports the tip where it will be a few milliseconds from now.
//!
//! What is drawn follows the pen by the time the samples take to reach the screen. Moving
//! every reported position ahead along the pen's velocity takes a part of that away. Unlike a
//! prediction a drawing application makes for the wet end of a stroke, what is reported here
//! becomes the ink: whatever the prediction gets wrong stays on the page. So it is off unless
//! a horizon is configured, and it holds back wherever the velocity says little about the
//! next moment (measured on captures of handwriting, see the constants).
//!
//! Only the positions of the tip (MPP v2 position windows) are predicted. The eraser end is
//! located once per frame from the weaker position tone and leaves no ink; it is reported
//! as measured.
//!
//! The stage keeps the last positions as they were before prediction, whether or not a
//! horizon is set: switching it on, off or to another value in the middle of a stroke is safe.

/// Samples further apart than this (microseconds) do not give a velocity: the stream had a gap.
const MAX_INTERVAL_US: u32 = 20_000;
/// The velocity is the mean of the last two intervals, weighted like this for the older one.
/// The latest interval alone doubles the roughness of the path (noise of one position
/// difference times horizon over interval); the mean keeps it at 1.5 times for one sample of
/// horizon.
const OLDER_WEIGHT: f64 = 0.5;
/// How much of the horizon is taken away per unit of change of the velocity vector (relative
/// to the larger of the two speeds): a pen that turns or stops is not where a straight line says.
const TURN_DAMPING: f64 = 1.0;
/// The same per unit of relative pressure drop since the sample before: the pen is being
/// lifted, and a stroke must not be drawn beyond where it ends.
const PRESSURE_DROP_DAMPING: f64 = 3.0;
/// The part of the horizon used in contact without a steady force tone: the tail of a
/// stroke, or writing too light for the pen to report force, where contact ends without
/// warning.
const NO_FORCE_PART: f64 = 0.5;
/// After contact begins or ends the horizon comes back over this many samples: the velocity
/// of the pen coming down says little about the stroke.
const START_RAMP: u32 = 3;

/// A position before prediction.
#[derive(Clone, Copy, Debug)]
struct Past {
    time_us: u32,
    x: f64,
    y: f64,
    pressure: f64,
}

/// What the predictor needs to know about a sample besides its position.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Context {
    pub contact: bool,
    /// The pressure the sample reports.
    pub pressure: f64,
    /// The position is held by the lock: the pen rests.
    pub holding: bool,
    /// The pen reports force and did so in its last window.
    pub force_steady: bool,
}

#[derive(Clone, Debug)]
pub struct PredictStage {
    /// The horizon in seconds; 0 = off.
    horizon: f64,
    /// The two samples before the current one, the older first.
    past: [Option<Past>; 2],
    contact: bool,
    /// Samples since contact began or ended.
    since_change: u32,
}

impl PredictStage {
    pub fn new(horizon_ms: f64) -> PredictStage {
        PredictStage { horizon: horizon_ms.max(0.0) / 1000.0, past: [None; 2], contact: false, since_change: 0 }
    }

    pub fn set_horizon(&mut self, horizon_ms: f64) {
        self.horizon = horizon_ms.max(0.0) / 1000.0;
    }

    /// Takes a position (millimeters) sampled at `time_us` and returns the one to report.
    pub fn apply(&mut self, time_us: u32, x: f64, y: f64, context: &Context) -> (f64, f64) {
        // After a gap in the stream everything starts anew, the ramp included.
        if self.past[1].is_some_and(|last| interval(last.time_us, time_us).is_none()) {
            self.reset();
        }
        if context.contact != self.contact {
            self.contact = context.contact;
            self.since_change = 0;
        }
        self.since_change = self.since_change.saturating_add(1);

        let [older, last] = self.past;
        self.past = [last, Some(Past { time_us, x, y, pressure: context.pressure })];

        if self.horizon <= 0.0 || context.holding {
            return (x, y);
        }
        // Two velocities are needed: the latest one and the one before it, to know whether
        // the pen keeps its course.
        let (Some(older), Some(last)) = (older, last) else { return (x, y) };
        let (Some(dt), Some(dt_before)) = (interval(last.time_us, time_us), interval(older.time_us, last.time_us)) else { return (x, y) };

        let v = ((x - last.x) / dt, (y - last.y) / dt);
        let v_before = ((last.x - older.x) / dt_before, (last.y - older.y) / dt_before);
        let speed = v.0.hypot(v.1);
        let speed_before = v_before.0.hypot(v_before.1);
        let mean = (((1.0 - OLDER_WEIGHT) * v.0) + (OLDER_WEIGHT * v_before.0), ((1.0 - OLDER_WEIGHT) * v.1) + (OLDER_WEIGHT * v_before.1));

        let mut part = 1.0;
        if context.contact && !context.force_steady {
            part *= NO_FORCE_PART;
        }
        if speed.max(speed_before) > 0.0 {
            let change = (mean.0 - v_before.0).hypot(mean.1 - v_before.1) / speed.max(speed_before);
            part *= (1.0 - (TURN_DAMPING * change)).max(0.0);
        }
        part *= (f64::from(self.since_change.saturating_sub(2)) / f64::from(START_RAMP)).min(1.0);
        if context.contact && last.pressure > 0.0 && context.pressure < last.pressure {
            part *= (1.0 - (PRESSURE_DROP_DAMPING * (1.0 - (context.pressure / last.pressure)))).max(0.0);
        }

        (x + (mean.0 * self.horizon * part), y + (mean.1 * self.horizon * part))
    }

    /// The pen left, or a sample came that is not part of the stream of predicted positions.
    pub fn reset(&mut self) {
        self.past = [None; 2];
        self.contact = false;
        self.since_change = 0;
    }
}

/// The time between two samples in seconds, if they follow each other closely.
fn interval(from_us: u32, to_us: u32) -> Option<f64> {
    let dt = to_us.wrapping_sub(from_us);
    (dt > 0 && dt <= MAX_INTERVAL_US).then(|| f64::from(dt) / 1e6)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WRITING: Context = Context { contact: true, pressure: 0.3, holding: false, force_steady: true };

    /// Feeds positions 7.5 ms apart and returns what comes out.
    fn feed(stage: &mut PredictStage, start_us: u32, points: &[(f64, f64)], context: &Context) -> Vec<(f64, f64)> {
        points.iter().enumerate().map(|(i, &(x, y))| stage.apply(start_us.wrapping_add(7500 * i as u32), x, y, context)).collect()
    }

    fn line(n: usize, step: f64) -> Vec<(f64, f64)> {
        (0..n).map(|i| (10.0 + step * i as f64, 20.0)).collect()
    }

    #[test]
    fn off_changes_nothing() {
        let mut s = PredictStage::new(0.0);
        let points = line(10, 0.75);
        assert_eq!(feed(&mut s, 0, &points, &WRITING), points);
    }

    #[test]
    fn a_steady_line_is_moved_ahead_by_the_horizon() {
        // 100 mm/s, one sample of horizon: every point lands on the next one.
        let mut s = PredictStage::new(7.5);
        let points = line(12, 0.75);
        let out = feed(&mut s, 0, &points, &WRITING);
        // Nothing before there are two velocities, then the ramp of the stroke start.
        assert_eq!(&out[..2], &points[..2]);
        assert!(out[2].0 > points[2].0 - 1e-12 && out[3].0 < points[4].0);
        for i in 5..11 {
            assert!((out[i].0 - points[i + 1].0).abs() < 1e-9, "{i}: {:?}", out[i]);
            assert_eq!(out[i].1, 20.0);
        }
        // The time stamps wrap around without a hiccup.
        let mut s = PredictStage::new(7.5);
        let out = feed(&mut s, u32::MAX - 20_000, &points, &WRITING);
        assert!((out[8].0 - points[9].0).abs() < 1e-9);
    }

    #[test]
    fn a_corner_and_a_stop_are_not_overshot() {
        let mut s = PredictStage::new(7.5);
        let mut points = line(8, 0.75);
        let corner = *points.last().unwrap();
        // A right angle, then on at the same speed.
        points.extend((1..6).map(|i| (corner.0, corner.1 + 0.75 * f64::from(i))));
        let out = feed(&mut s, 0, &points, &WRITING);
        // The first sample after the corner is not carried on along the old direction by
        // more than a fraction of a step.
        assert!(out[8].0 - corner.0 < 0.2, "{:?}", out[8]);
        // A few samples later the prediction is back, along the new direction.
        assert!((out[12].1 - (points[12].1 + 0.75)).abs() < 1e-9 && (out[12].0 - corner.0).abs() < 1e-9);

        // The pen stops dead: the samples after it stay where the pen is.
        let mut s = PredictStage::new(7.5);
        let mut points = line(8, 0.75);
        points.extend([*points.last().unwrap(); 3]);
        let out = feed(&mut s, 0, &points, &WRITING);
        assert!(out[8].0 - points[8].0 < 0.2);
        assert_eq!(out[9], points[9]);
    }

    #[test]
    fn a_lifting_pen_is_held_back() {
        let points = line(10, 0.75);
        let ahead = |context: &Context| {
            let mut s = PredictStage::new(7.5);
            feed(&mut s, 0, &points[..9], &WRITING);
            s.apply(7500 * 9, points[9].0, points[9].1, context).0 - points[9].0
        };
        assert!((ahead(&WRITING) - 0.75).abs() < 1e-9);
        // The pressure falls by a fifth: 40 % of the horizon; by a third or more: none.
        assert!((ahead(&Context { pressure: 0.24, ..WRITING }) - 0.3).abs() < 1e-9);
        assert_eq!(ahead(&Context { pressure: 0.15, ..WRITING }), 0.0);
        // No steady force tone: half.
        assert!((ahead(&Context { force_steady: false, ..WRITING }) - 0.375).abs() < 1e-9);
        // The lock holds the position: none.
        assert_eq!(ahead(&Context { holding: true, ..WRITING }), 0.0);
        // Contact ends: the first hover sample is where the pen is.
        assert_eq!(ahead(&Context { contact: false, pressure: 0.0, ..WRITING }), 0.0);
    }

    #[test]
    fn gaps_and_resets_start_anew() {
        let mut s = PredictStage::new(7.5);
        let points = line(10, 0.75);
        feed(&mut s, 0, &points[..8], &WRITING);
        // 30 ms without a sample.
        assert_eq!(s.apply(7500 * 7 + 30_000, points[8].0, points[8].1, &WRITING), points[8]);
        assert_eq!(s.apply(7500 * 8 + 30_000, points[9].0, points[9].1, &WRITING), points[9]);
        // The ramp starts again as well: the third sample after the gap gets a third.
        let third = s.apply(7500 * 9 + 30_000, points[9].0 + 0.75, 20.0, &WRITING);
        assert!((third.0 - (points[9].0 + 0.75 + 0.25)).abs() < 1e-9, "{third:?}");

        let mut s = PredictStage::new(7.5);
        feed(&mut s, 0, &points[..8], &WRITING);
        s.reset();
        assert_eq!(s.apply(7500 * 8, points[8].0, points[8].1, &WRITING), points[8]);

        // The horizon changes in the middle of a stroke: the history is there.
        let mut s = PredictStage::new(0.0);
        feed(&mut s, 0, &points[..8], &WRITING);
        s.set_horizon(15.0);
        assert!((s.apply(7500 * 8, points[8].0, points[8].1, &WRITING).0 - (points[8].0 + 1.5)).abs() < 1e-9);
    }
}
