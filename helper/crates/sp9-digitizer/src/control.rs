// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
//! Controlling and observing the processing while it runs: the settings that can change
//! without a restart ([`Param`]), a snapshot of the pen ([`PenState`]), and the names of the
//! D-Bus service `sp9-digitizerd` offers them on. No D-Bus here: the daemon and its clients
//! share these.

use crate::config::Config;
use crate::pen::Pen;
use serde::Serialize;

/// The well-known name on the system bus, the object, and the interfaces.
pub const BUS_NAME: &str = "io.github.joonhoekim.Sp9Digitizer1";
pub const OBJECT_PATH: &str = "/io/github/joonhoekim/Sp9Digitizer1";
pub const PEN_INTERFACE: &str = "io.github.joonhoekim.Sp9Digitizer1.Pen";
pub const TOUCH_INTERFACE: &str = "io.github.joonhoekim.Sp9Digitizer1.Touch";
pub const DIAGNOSTICS_INTERFACE: &str = "io.github.joonhoekim.Sp9Digitizer1.Diagnostics";

/// The value of a setting.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Value {
    Bool(bool),
    Number(f64),
}

/// A setting that can be changed while the daemon runs. The names are those of the D-Bus
/// properties and of the keys of `SetParams`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Param {
    /// Pen: contact without force (bool).
    ZeroForce,
    /// Pen: level at which a contact without force begins, and below which it ends
    /// (fractions of the reference).
    ZeroForceEnter,
    ZeroForceExit,
    /// Pen: the tip energy at contact; 0 = learned from strokes with force.
    ZeroForceReference,
    /// Pen: a stroke that had force goes on while its tip signal stays above this fraction.
    ZeroForceStrokeLevel,
    /// Pen: the lowest pressure reported in contact (fraction).
    PressureFloor,
    /// Touch: thresholds of the contact detection (heatmap counts).
    ActivationThreshold,
    DeactivationThreshold,
    /// Touch: no touch while the pen is in range (bool).
    DisableOnStylus,
    /// Touch: for how long after the pen left touch stays off (seconds).
    PenHoldoff,
}

impl Param {
    pub const PEN: [Param; 6] = [
        Param::ZeroForce,
        Param::ZeroForceEnter,
        Param::ZeroForceExit,
        Param::ZeroForceReference,
        Param::ZeroForceStrokeLevel,
        Param::PressureFloor,
    ];
    pub const TOUCH: [Param; 4] = [Param::ActivationThreshold, Param::DeactivationThreshold, Param::DisableOnStylus, Param::PenHoldoff];

    pub fn name(self) -> &'static str {
        match self {
            Param::ZeroForce => "ZeroForce",
            Param::ZeroForceEnter => "ZeroForceEnter",
            Param::ZeroForceExit => "ZeroForceExit",
            Param::ZeroForceReference => "ZeroForceReference",
            Param::ZeroForceStrokeLevel => "ZeroForceStrokeLevel",
            Param::PressureFloor => "PressureFloor",
            Param::ActivationThreshold => "ActivationThreshold",
            Param::DeactivationThreshold => "DeactivationThreshold",
            Param::DisableOnStylus => "DisableOnStylus",
            Param::PenHoldoff => "PenHoldoff",
        }
    }

    pub fn from_name(name: &str) -> Option<Param> {
        Param::PEN.into_iter().chain(Param::TOUCH).find(|p| p.name() == name)
    }

    pub fn get(self, config: &Config) -> Value {
        let zf = &config.pen.contact.zero_force;
        match self {
            Param::ZeroForce => Value::Bool(zf.enabled),
            Param::ZeroForceEnter => Value::Number(zf.enter),
            Param::ZeroForceExit => Value::Number(zf.exit),
            Param::ZeroForceReference => Value::Number(zf.reference),
            Param::ZeroForceStrokeLevel => Value::Number(zf.stroke_level),
            Param::PressureFloor => Value::Number(config.pen.pressure.floor),
            Param::ActivationThreshold => Value::Number(config.touch.contacts.activation_threshold),
            Param::DeactivationThreshold => Value::Number(config.touch.contacts.deactivation_threshold),
            Param::DisableOnStylus => Value::Bool(config.touch.disable_on_stylus),
            Param::PenHoldoff => Value::Number(config.touch.pen_holdoff_s),
        }
    }

    /// Writes the value into a configuration. The combination is checked by [`set_params`].
    fn put(self, config: &mut Config, value: Value) -> Result<(), String> {
        let zf = &mut config.pen.contact.zero_force;
        let wrong = || format!("{}: wrong type of value", self.name());
        let number = |v: Value| match v {
            Value::Number(n) if n.is_finite() => Ok(n),
            Value::Number(_) => Err(format!("{}: not a finite number", self.name())),
            Value::Bool(_) => Err(wrong()),
        };
        let boolean = |v: Value| match v {
            Value::Bool(b) => Ok(b),
            Value::Number(_) => Err(wrong()),
        };
        match self {
            Param::ZeroForce => zf.enabled = boolean(value)?,
            Param::ZeroForceEnter => zf.enter = number(value)?,
            Param::ZeroForceExit => zf.exit = number(value)?,
            Param::ZeroForceReference => zf.reference = number(value)?,
            Param::ZeroForceStrokeLevel => zf.stroke_level = number(value)?,
            Param::PressureFloor => config.pen.pressure.floor = number(value)?,
            Param::ActivationThreshold => config.touch.contacts.activation_threshold = number(value)?,
            Param::DeactivationThreshold => config.touch.contacts.deactivation_threshold = number(value)?,
            Param::DisableOnStylus => config.touch.disable_on_stylus = boolean(value)?,
            Param::PenHoldoff => config.touch.pen_holdoff_s = number(value)?,
        }
        Ok(())
    }
}

/// Applies a set of settings to a configuration: all of them or, if one is refused or the
/// result is not a usable configuration, none.
pub fn set_params(config: &mut Config, params: &[(Param, Value)]) -> Result<(), String> {
    let mut next = config.clone();
    for &(param, value) in params {
        param.put(&mut next, value)?;
    }

    let zf = &next.pen.contact.zero_force;
    if !(zf.enter > 0.0 && zf.exit > 0.0 && zf.exit <= zf.enter && zf.enter <= 10.0) {
        return Err("zero force: 0 < exit <= enter <= 10".into());
    }
    if zf.reference < 0.0 {
        return Err("the zero force reference must not be negative".into());
    }
    if !(0.0..=1.0).contains(&zf.stroke_level) || !(0.0..=1.0).contains(&next.pen.pressure.floor) {
        return Err("stroke level and pressure floor are fractions".into());
    }
    if !(0.0..=10.0).contains(&next.touch.pen_holdoff_s) {
        return Err("the pen hold-off is 0 .. 10 seconds".into());
    }
    let c = &next.touch.contacts;
    if !(c.deactivation_threshold >= 1.0 && c.activation_threshold <= 100.0) {
        return Err("touch thresholds are 1 .. 100".into());
    }
    next.validate()?;
    *config = next;
    Ok(())
}

/// A snapshot of the pen for clients (haptics, calibration, diagnostics).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct PenState {
    pub in_range: bool,
    pub contact: bool,
    pub eraser: bool,
    pub button: bool,
    /// Energy of the tip signal in the last MPP v2 position window relative to the
    /// reference; 0 while either is unknown.
    pub tip_level: f64,
    /// The tip energy that counts as contact (configured or learned), 0 if unknown.
    pub reference: f64,
}

impl PenState {
    pub fn of(pen: &Pen) -> PenState {
        let s = pen.sample();
        let reference = pen.zero_force_reference();
        let energy = pen.tip_signal().energy.filter(|_| s.proximity && reference > 0.0);
        PenState {
            in_range: s.proximity,
            contact: s.proximity && s.contact,
            eraser: s.proximity && s.eraser,
            button: s.proximity && s.button,
            tip_level: energy.map_or(0.0, |e| e / reference),
            reference,
        }
    }

    /// The tool as clients name it; empty while the pen is out of range.
    pub fn tool(&self) -> &'static str {
        match (self.in_range, self.eraser) {
            (false, _) => "",
            (true, false) => "pen",
            (true, true) => "eraser",
        }
    }

    /// Whether a client that follows proximity, contact, tool and button sees a change.
    pub fn differs_in_events(&self, other: &PenState) -> bool {
        (self.in_range, self.contact, self.eraser, self.button) != (other.in_range, other.contact, other.eraser, other.button)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Geometry;

    #[test]
    fn names_round_trip() {
        for p in Param::PEN.into_iter().chain(Param::TOUCH) {
            assert_eq!(Param::from_name(p.name()), Some(p));
        }
        assert_eq!(Param::from_name("InkFilter"), None);
    }

    #[test]
    fn all_or_nothing() {
        let mut config = Config::default();
        set_params(
            &mut config,
            &[
                (Param::ZeroForce, Value::Bool(true)),
                (Param::ZeroForceReference, Value::Number(5e6)),
                (Param::PressureFloor, Value::Number(0.03)),
            ],
        )
        .unwrap();
        assert_eq!(Param::ZeroForce.get(&config), Value::Bool(true));
        assert_eq!(Param::ZeroForceReference.get(&config), Value::Number(5e6));

        let before = config.clone();
        // exit above enter
        assert!(
            set_params(&mut config, &[(Param::PressureFloor, Value::Number(0.05)), (Param::ZeroForceExit, Value::Number(0.9))]).is_err()
        );
        // wrong type, not finite, out of range
        assert!(set_params(&mut config, &[(Param::ZeroForce, Value::Number(1.0))]).is_err());
        assert!(set_params(&mut config, &[(Param::PressureFloor, Value::Number(f64::NAN))]).is_err());
        assert!(set_params(
            &mut config,
            &[(Param::ActivationThreshold, Value::Number(10.0)), (Param::DeactivationThreshold, Value::Number(20.0))]
        )
        .is_err());
        assert!(set_params(&mut config, &[(Param::PenHoldoff, Value::Number(-1.0))]).is_err());
        assert_eq!(config, before);

        set_params(
            &mut config,
            &[
                (Param::ActivationThreshold, Value::Number(22.0)),
                (Param::DeactivationThreshold, Value::Number(18.0)),
                (Param::DisableOnStylus, Value::Bool(false)),
            ],
        )
        .unwrap();
        assert!(!config.touch.disable_on_stylus);
    }

    #[test]
    fn state_of_an_idle_pen() {
        let config = Config::default();
        let pen = Pen::new(&config.pen, &Geometry::SP9);
        let state = PenState::of(&pen);
        assert_eq!(state, PenState::default());
        assert_eq!(state.tool(), "");
        assert!(!state.differs_in_events(&PenState { tip_level: 1.0, ..state }));
        assert!(state.differs_in_events(&PenState { in_range: true, ..state }));
    }
}
