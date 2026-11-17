// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
//! The periodic policy step of the daemon (once a second): profile limits, skin guard, PSR by
//! power source, touch off while the panel is off, and whether the fan follows a request. It
//! only writes when the wanted value
//! differs from the last one written, so a person or another tool can still change a knob
//! between profile switches. The one exception is the EPP, which is written again for
//! `EPP_HOLD_S` after each apply when something else replaced it.

use crate::config::Config;
use crate::input;
use crate::perf::{self, Limits};
use crate::thermal::{self, FanRequestState, SkinGuard};
use crate::Backends;

/// How long after an EPP write the value is put back when something else replaces it.
/// power-profiles-daemon follows platform_profile (and the power source) too and writes its own
/// EPP name 1–2 s after a profile change it did not make, i.e. after the helper's write
/// (measured, EPP of cpu0 every 0.4 s: 64 for 0.8 s, then `performance`). Without the hold the
/// EPP in force depends on who changed the profile. The window is bounded so that an EPP set by
/// hand later stays until the next profile or power source change.
///
/// The window is the time in which the value may be replaced, not the time in which the helper
/// looks: a step that starts inside it compares before it counts its own `dt`, so the step
/// that ends the window still answers. Counting first loses the window whenever the steps are
/// further apart than a second (the step reads the SAM sensors and the power supplies, and a
/// step delayed by 2 s was seen on the device): two steps of 2–3 s use up the 5 s, the first
/// before power-profiles-daemon has written, and nothing is put back, without any error.
pub const EPP_HOLD_S: f64 = 5.0;

#[derive(Default)]
pub struct Runtime {
    /// (profile, ac, limits written)
    pub applied: Option<(String, bool, Limits)>,
    /// (profile, ac) the EPP was written for
    pub epp_for: Option<(String, bool)>,
    /// What energy_performance_preference read back right after the helper's write. The hold
    /// compares against this and not against the number: the kernel shows a value that has a
    /// name as the name (`performance` for 0), and which number `balance_performance` stands for
    /// depends on the CPU model.
    epp_ours: Vec<String>,
    /// seconds of `EPP_HOLD_S` left
    epp_hold_s: f64,
    pub guard: SkinGuard,
    /// the guard is lowering a limit now
    pub guard_active: bool,
    pub skin_c: Option<f64>,
    /// last value written to i915_edp_psr_debug
    pub psr_written: Option<u32>,
    /// touchscreens the helper inhibited
    pub touch_inhibited: Vec<String>,
    /// Fan rpm request the helper sent last (0 = none). The SAM may have dropped it since
    /// (resume) or may not be following it (it drives the fan itself): `fan_state` tells.
    pub fan_request: u16,
    /// seconds since `fan_request` was sent
    fan_request_age_s: f64,
    pub fan_state: FanRequestState,
    /// problems of the last step, for the journal (each reported once)
    pub errors: Vec<String>,
    reported: Vec<String>,
}

impl Runtime {
    fn error(&mut self, e: String) {
        if !self.reported.contains(&e) {
            self.reported.push(e.clone());
            self.errors.push(e);
        }
    }

    /// Errors not yet handed out.
    pub fn take_errors(&mut self) -> Vec<String> {
        std::mem::take(&mut self.errors)
    }

    /// Forget what was written so the next step writes everything again (reload, resume).
    pub fn reset(&mut self) {
        self.applied = None;
        self.epp_for = None;
        self.reported.clear();
    }

    /// Record a fan rpm request that was just sent (0 = withdrawn).
    pub fn set_fan_request(&mut self, rpm: u16) {
        self.fan_request = rpm;
        self.fan_request_age_s = 0.0;
        self.fan_state = thermal::fan_request_state(rpm, 0.0, None);
    }

    /// A failed write leaves `epp_ours` as it was, so the hold tries again on its next step.
    fn write_epp(&mut self, b: &Backends, epp: u8) {
        match b.power.set_epp(epp) {
            Ok(_) => self.epp_ours = b.power.epp(),
            Err(e) => self.error(format!("EPP: {e}")),
        }
    }
}

/// The limits the profile wants now, before the skin guard.
pub fn target(cfg: &Config, profile: &str, ac: bool) -> Option<Limits> {
    cfg.performance.limits_for(profile, ac)
}

pub fn tick(b: &Backends, cfg: &Config, rt: &mut Runtime, dt: f64) {
    let ac = b.power.on_ac().unwrap_or(true);
    performance(b, cfg, rt, ac, dt);
    psr(b, cfg, rt, ac);
    touch(b, cfg, rt);
    fan(b, rt, dt);
}

/// Only observes: a request the SAM is not following is reported, not sent again. Sending it
/// again changes nothing while the SAM drives the fan, and after a resume it would undo the
/// kernel handing the fan back to the SAM (surface_platform_profile sends 0x0b = 0).
fn fan(b: &Backends, rt: &mut Runtime, dt: f64) {
    if rt.fan_request == 0 {
        rt.fan_state = FanRequestState::None;
        return;
    }
    rt.fan_request_age_s += dt;
    rt.fan_state = thermal::fan_request_state(rt.fan_request, rt.fan_request_age_s, b.thermal.fan_rpm());
}

fn performance(b: &Backends, cfg: &Config, rt: &mut Runtime, ac: bool, dt: f64) {
    rt.skin_c = b.thermal.skin_c(&cfg.performance.skin_sensor);
    if !cfg.performance.follow_profile {
        rt.applied = None;
        rt.epp_for = None;
        rt.epp_hold_s = 0.0;
        rt.guard_active = false;
        return;
    }
    let Some(profile) = b.power.profile().map(|p| perf::normalize_profile(&p)) else { return };
    let Some(table) = target(cfg, &profile, ac) else { return };
    let mut want = table;
    rt.guard_active = false;
    if cfg.performance.skin_guard {
        match rt.skin_c {
            Some(t) => {
                want = rt.guard.step(&profile, table, t, dt);
                rt.guard_active = want != table;
            }
            None => rt.error(format!("skin guard: no {} temperature (surface_temp hwmon)", cfg.performance.skin_sensor)),
        }
    } else {
        rt.guard = SkinGuard::default();
    }
    let key = (profile.clone(), ac, want);
    if rt.applied.as_ref() != Some(&key) {
        match b.power.set_rapl(&want) {
            Ok(_) => rt.applied = Some(key),
            Err(e) => {
                rt.error(format!("RAPL: {e}"));
                rt.applied = Some(key);
            }
        }
    }
    if cfg.performance.epp {
        let k = (profile, ac);
        if rt.epp_for.as_ref() != Some(&k) {
            rt.epp_hold_s = EPP_HOLD_S;
            rt.epp_ours.clear();
            rt.write_epp(b, table.epp);
            rt.epp_for = Some(k);
        } else if rt.epp_hold_s > 0.0 {
            if b.power.epp() != rt.epp_ours {
                rt.write_epp(b, table.epp);
            }
            rt.epp_hold_s -= dt;
        }
    } else {
        rt.epp_for = None;
        rt.epp_hold_s = 0.0;
    }
}

fn psr(b: &Backends, cfg: &Config, rt: &mut Runtime, ac: bool) {
    let mode = if ac { cfg.display.psr_ac } else { cfg.display.psr_dc };
    let v = mode.debug_value();
    let wanted = match (mode, rt.psr_written) {
        (crate::display::PsrMode::Default, None) => None,
        (_, Some(w)) if w == v => None,
        _ => Some(v),
    };
    if let Some(v) = wanted {
        match b.display.set_psr_debug(v) {
            Ok(()) => rt.psr_written = Some(v),
            Err(e) => {
                rt.error(format!("PSR: {e}"));
                rt.psr_written = Some(v);
            }
        }
    }
}

fn touch(b: &Backends, cfg: &Config, rt: &mut Runtime) {
    let off = cfg.input.touch_off_when_display_off && b.display.panel_on() == Some(false);
    if off {
        for d in input::touchscreens(&*b.input) {
            if d.inhibited == Some(false) && !rt.touch_inhibited.contains(&d.id) {
                match b.input.set_inhibited(&d.id, true) {
                    Ok(()) => rt.touch_inhibited.push(d.id),
                    Err(e) => rt.error(format!("touch inhibit {}: {e}", d.id)),
                }
            }
        }
    } else {
        for id in std::mem::take(&mut rt.touch_inhibited) {
            // the device may be gone (iptsd restarted): nothing to undo then
            let _ = b.input.set_inhibited(&id, false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::display::{FakeDisplay, PsrMode};
    use crate::input::{FakeInput, InputDev};
    use crate::perf::{FakePower, PowerBackend};
    use crate::thermal::FakeThermal;

    struct Rig {
        b: Backends,
        power: FakePower,
        thermal: FakeThermal,
        display: FakeDisplay,
        input: FakeInput,
    }

    fn rig() -> Rig {
        let mut b = Backends::fake();
        let power = FakePower::default();
        let thermal = FakeThermal::default();
        let display = FakeDisplay::default();
        let input = FakeInput::default();
        b.power = Box::new(power.clone());
        b.thermal = Box::new(thermal.clone());
        b.display = Box::new(display.clone());
        b.input = Box::new(input.clone());
        Rig { b, power, thermal, display, input }
    }

    #[test]
    fn follows_profile_and_power_source() {
        let r = rig();
        let cfg = Config::default();
        let mut rt = Runtime::default();
        tick(&r.b, &cfg, &mut rt, 1.0);
        tick(&r.b, &cfg, &mut rt, 1.0);
        {
            let s = r.power.0.lock().unwrap();
            assert_eq!(s.rapl_writes, vec![perf::default_limits("balanced", true).unwrap()]);
            assert_eq!(s.epp_writes, vec![153]);
        }
        r.power.0.lock().unwrap().ac = Some(false);
        tick(&r.b, &cfg, &mut rt, 1.0);
        r.power.set_profile("performance").unwrap();
        tick(&r.b, &cfg, &mut rt, 1.0);
        let s = r.power.0.lock().unwrap();
        assert_eq!(s.rapl_writes[1], Limits::new(13.0, 35.0, 8.0, 191));
        assert_eq!(s.rapl_writes[2], Limits::new(30.0, 45.0, 28.0, 64));
        assert_eq!(s.epp_writes, vec![153, 191, 64]);
    }

    /// power-profiles-daemon writing its EPP name after the helper's write.
    fn ppd_writes(r: &Rig, name: &str) {
        r.power.0.lock().unwrap().epp_other = Some(name.into());
    }

    #[test]
    fn epp_is_put_back_inside_the_hold() {
        let r = rig();
        let cfg = Config::default();
        let mut rt = Runtime::default();
        tick(&r.b, &cfg, &mut rt, 1.0);
        // nothing interferes: one write, no more
        for _ in 0..8 {
            tick(&r.b, &cfg, &mut rt, 1.0);
        }
        assert_eq!(r.power.0.lock().unwrap().epp_writes, vec![153]);

        r.power.set_profile("performance").unwrap();
        tick(&r.b, &cfg, &mut rt, 1.0);
        ppd_writes(&r, "performance");
        tick(&r.b, &cfg, &mut rt, 1.0);
        assert_eq!(r.power.epp(), vec!["64".to_string()]);
        // a second overwrite inside the window is answered too, then the value stays
        ppd_writes(&r, "balance_performance");
        for _ in 0..8 {
            tick(&r.b, &cfg, &mut rt, 1.0);
        }
        assert_eq!(r.power.0.lock().unwrap().epp_writes, vec![153, 64, 64, 64]);
        assert_eq!(r.power.epp(), vec!["64".to_string()]);
    }

    #[test]
    fn epp_is_put_back_when_the_steps_are_slow() {
        let r = rig();
        let cfg = Config::default();
        let mut rt = Runtime::default();
        tick(&r.b, &cfg, &mut rt, 1.0);
        // steps 2–3 s apart; the other writer comes after the first of them
        tick(&r.b, &cfg, &mut rt, 1.9);
        ppd_writes(&r, "balance_performance");
        tick(&r.b, &cfg, &mut rt, 3.3);
        assert_eq!(r.power.epp(), vec!["153".to_string()]);
        // one step longer than the whole window
        r.power.set_profile("performance").unwrap();
        tick(&r.b, &cfg, &mut rt, 1.0);
        ppd_writes(&r, "performance");
        tick(&r.b, &cfg, &mut rt, 7.0);
        assert_eq!(r.power.epp(), vec!["64".to_string()]);
        // the window is over all the same: what comes now stays
        ppd_writes(&r, "power");
        tick(&r.b, &cfg, &mut rt, 1.0);
        assert_eq!(r.power.epp(), vec!["power".to_string()]);
        assert_eq!(r.power.0.lock().unwrap().epp_writes, vec![153, 153, 64, 64]);
    }

    #[test]
    fn epp_write_that_fails_is_tried_again_inside_the_hold() {
        let r = rig();
        let cfg = Config::default();
        let mut rt = Runtime::default();
        r.power.0.lock().unwrap().epp_fail = true;
        tick(&r.b, &cfg, &mut rt, 1.0);
        assert_eq!(rt.take_errors().len(), 1);
        r.power.0.lock().unwrap().epp_fail = false;
        tick(&r.b, &cfg, &mut rt, 1.0);
        assert_eq!(r.power.epp(), vec!["153".to_string()]);
        // and given up when the window is over
        r.power.set_profile("performance").unwrap();
        r.power.0.lock().unwrap().epp_fail = true;
        for _ in 0..10 {
            tick(&r.b, &cfg, &mut rt, 1.0);
        }
        r.power.0.lock().unwrap().epp_fail = false;
        tick(&r.b, &cfg, &mut rt, 1.0);
        assert_eq!(r.power.0.lock().unwrap().epp_writes, vec![153]);
    }

    #[test]
    fn epp_set_by_hand_after_the_hold_stays() {
        let r = rig();
        let cfg = Config::default();
        let mut rt = Runtime::default();
        // the apply, then the steps that start inside the window
        for _ in 0..(EPP_HOLD_S as usize + 1) {
            tick(&r.b, &cfg, &mut rt, 1.0);
        }
        ppd_writes(&r, "power");
        for _ in 0..20 {
            tick(&r.b, &cfg, &mut rt, 1.0);
        }
        assert_eq!(r.power.epp(), vec!["power".to_string()]);
        assert_eq!(r.power.0.lock().unwrap().epp_writes, vec![153]);
        // the next power source change applies the table again and holds it
        r.power.0.lock().unwrap().ac = Some(false);
        tick(&r.b, &cfg, &mut rt, 1.0);
        ppd_writes(&r, "balance_power");
        tick(&r.b, &cfg, &mut rt, 1.0);
        assert_eq!(r.power.0.lock().unwrap().epp_writes, vec![153, 191, 191]);
    }

    #[test]
    fn epp_hold_ends_with_the_setting() {
        let r = rig();
        let mut cfg = Config::default();
        let mut rt = Runtime::default();
        tick(&r.b, &cfg, &mut rt, 1.0);
        cfg.performance.epp = false;
        ppd_writes(&r, "performance");
        tick(&r.b, &cfg, &mut rt, 1.0);
        cfg.performance.epp = true;
        cfg.performance.follow_profile = false;
        tick(&r.b, &cfg, &mut rt, 1.0);
        assert_eq!(r.power.0.lock().unwrap().epp_writes, vec![153]);
        assert_eq!(r.power.epp(), vec!["performance".to_string()]);
    }

    #[test]
    fn fan_request_state_follows_the_fan() {
        use crate::thermal::FAN_SETTLE_S;
        let r = rig();
        let cfg = Config::default();
        let mut rt = Runtime::default();
        tick(&r.b, &cfg, &mut rt, 1.0);
        assert_eq!(rt.fan_state, FanRequestState::None);
        rt.set_fan_request(4000);
        assert_eq!(rt.fan_state, FanRequestState::Settling);
        r.thermal.0.lock().unwrap().fan = Some(1200);
        for _ in 0..(FAN_SETTLE_S as usize - 1) {
            tick(&r.b, &cfg, &mut rt, 1.0);
        }
        assert_eq!(rt.fan_state, FanRequestState::Settling);
        r.thermal.0.lock().unwrap().fan = Some(3992);
        tick(&r.b, &cfg, &mut rt, 1.0);
        assert_eq!(rt.fan_state, FanRequestState::Honoured);
        // the SAM takes the fan over under load
        r.thermal.0.lock().unwrap().fan = Some(5950);
        tick(&r.b, &cfg, &mut rt, 1.0);
        assert_eq!(rt.fan_state, FanRequestState::NotHonoured);
        // and lets go again
        r.thermal.0.lock().unwrap().fan = Some(4010);
        tick(&r.b, &cfg, &mut rt, 1.0);
        assert_eq!(rt.fan_state, FanRequestState::Honoured);
        // the kernel cancelled the request on resume: the fan stops, the request is not sent again
        r.thermal.0.lock().unwrap().fan = Some(0);
        tick(&r.b, &cfg, &mut rt, 1.0);
        assert_eq!(rt.fan_state, FanRequestState::NotHonoured);
        r.thermal.0.lock().unwrap().fan = None;
        tick(&r.b, &cfg, &mut rt, 1.0);
        assert_eq!(rt.fan_state, FanRequestState::Unknown);
        // a new request settles again
        rt.set_fan_request(3000);
        tick(&r.b, &cfg, &mut rt, 1.0);
        assert_eq!(rt.fan_state, FanRequestState::Settling);
        rt.set_fan_request(0);
        assert_eq!(rt.fan_state, FanRequestState::None);
    }

    #[test]
    fn follow_off_writes_nothing() {
        let r = rig();
        let mut cfg = Config::default();
        cfg.performance.follow_profile = false;
        let mut rt = Runtime::default();
        tick(&r.b, &cfg, &mut rt, 1.0);
        assert!(r.power.0.lock().unwrap().rapl_writes.is_empty());
    }

    #[test]
    fn skin_guard_lowers_and_reports_missing_sensor() {
        let r = rig();
        let mut cfg = Config::default();
        cfg.performance.skin_guard = true;
        let mut rt = Runtime::default();
        tick(&r.b, &cfg, &mut rt, 1.0);
        assert_eq!(rt.take_errors().len(), 1);
        tick(&r.b, &cfg, &mut rt, 1.0);
        assert!(rt.take_errors().is_empty(), "reported once");
        r.thermal.set_skin(50.5);
        tick(&r.b, &cfg, &mut rt, 1.0);
        assert!(rt.guard_active);
        let last = *r.power.0.lock().unwrap().rapl_writes.last().unwrap();
        assert!(last.pl1_w < 28.0 && last.pl2_w <= 30.0, "{last:?}");
    }

    #[test]
    fn psr_policy() {
        let r = rig();
        let mut cfg = Config::default();
        let mut rt = Runtime::default();
        tick(&r.b, &cfg, &mut rt, 1.0);
        assert!(r.display.0.lock().unwrap().psr_writes.is_empty(), "default never writes");
        cfg.display.psr_ac = PsrMode::Off;
        cfg.display.psr_dc = PsrMode::Psr1;
        tick(&r.b, &cfg, &mut rt, 1.0);
        tick(&r.b, &cfg, &mut rt, 1.0);
        r.power.0.lock().unwrap().ac = Some(false);
        tick(&r.b, &cfg, &mut rt, 1.0);
        cfg.display = Default::default();
        tick(&r.b, &cfg, &mut rt, 1.0);
        assert_eq!(r.display.0.lock().unwrap().psr_writes, vec![0x01, 0x03, 0x00]);
    }

    #[test]
    fn touch_off_with_panel() {
        let r = rig();
        r.input.0.lock().unwrap().devices.push(InputDev {
            id: "input5".into(),
            name: "IPTSD Virtual Touchscreen 045E:0C52".into(),
            inhibited: Some(false),
        });
        let mut cfg = Config::default();
        let mut rt = Runtime::default();
        r.display.0.lock().unwrap().panel_on = Some(false);
        tick(&r.b, &cfg, &mut rt, 1.0);
        assert_eq!(r.input.0.lock().unwrap().devices[0].inhibited, Some(false), "policy off");
        cfg.input.touch_off_when_display_off = true;
        tick(&r.b, &cfg, &mut rt, 1.0);
        assert_eq!(r.input.0.lock().unwrap().devices[0].inhibited, Some(true));
        r.display.0.lock().unwrap().panel_on = Some(true);
        tick(&r.b, &cfg, &mut rt, 1.0);
        assert_eq!(r.input.0.lock().unwrap().devices[0].inhibited, Some(false));
    }
}
