// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
//! The D-Bus service of the daemon (`io.github.joonhoekim.Sp9Digitizer1` on the system bus):
//! settings that change while it runs, the state of the pen, counters, the configuration.
//!
//! The processing loop never waits for the bus. Both sides meet in [`Shared`]: a setting
//! changed over the bus is stored there and taken by the loop before its next report; the
//! loop stores the pen state there and hands changes of proximity, contact, tool and button to
//! the thread that emits the signal. The daemon keeps nothing across restarts: whoever
//! changes a setting (the helper) also persists it.
//!
//! Setting something is for root only (the bus policy says so, and the methods check the
//! caller again); reading is open.

use sp9_digitizer::control::{self, Param, PenState, Value};
use sp9_digitizer::{Config, Geometry};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use zbus::fdo;
use zbus::message::Header;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::OwnedValue;

/// How long the service waits before it tries the bus again.
const RETRY: Duration = Duration::from_secs(5);

#[derive(Debug, Default)]
pub struct Counters {
    /// Raw data reports processed.
    pub reports: AtomicU64,
    /// Of those, the ones that ended in a parse error.
    pub parse_errors: AtomicU64,
    pub pen_samples: AtomicU64,
    pub touch_frames: AtomicU64,
    /// Failed writes to the input devices.
    pub write_errors: AtomicU64,
}

/// What the processing loop and the bus share.
#[derive(Debug)]
pub struct Shared {
    config: Mutex<Config>,
    /// The configuration changed and the loop has not taken it yet.
    changed: AtomicBool,
    pen: Mutex<PenState>,
    pub counters: Counters,
    device: Mutex<String>,
    geometry: Geometry,
    /// Skip the check that a caller who sets something is root (tests on a private connection).
    open: bool,
}

impl Shared {
    pub fn new(config: Config, device: &str, geometry: Geometry, open: bool) -> Shared {
        Shared {
            config: Mutex::new(config),
            changed: AtomicBool::new(false),
            pen: Mutex::new(PenState::default()),
            counters: Counters::default(),
            device: Mutex::new(device.to_string()),
            geometry,
            open,
        }
    }

    /// The node in use changed (it comes back under another number after a suspend).
    pub fn set_device(&self, device: &str) {
        *self.device.lock().unwrap() = device.to_string();
    }

    pub fn config(&self) -> Config {
        self.config.lock().unwrap().clone()
    }

    /// The configuration, if it changed since the last call.
    pub fn take_changed_config(&self) -> Option<Config> {
        self.changed.swap(false, Ordering::AcqRel).then(|| self.config())
    }

    pub fn pen(&self) -> PenState {
        *self.pen.lock().unwrap()
    }

    /// Stores the pen state. Returns whether proximity, contact, tool or button changed.
    pub fn store_pen(&self, state: PenState) -> bool {
        let mut pen = self.pen.lock().unwrap();
        let changed = state.differs_in_events(&pen);
        *pen = state;
        changed
    }

    fn set(&self, params: &[(Param, Value)]) -> Result<(), String> {
        let mut config = self.config.lock().unwrap();
        control::set_params(&mut config, params)?;
        self.changed.store(true, Ordering::Release);
        Ok(())
    }

    fn number(&self, param: Param) -> f64 {
        match param.get(&self.config.lock().unwrap()) {
            Value::Number(n) => n,
            Value::Bool(b) => f64::from(u8::from(b)),
        }
    }

    fn flag(&self, param: Param) -> bool {
        param.get(&self.config.lock().unwrap()) == Value::Bool(true)
    }
}

/// The keys and values of a `SetParams` call as settings of one interface.
fn params_of(given: &HashMap<String, OwnedValue>, allowed: &[Param]) -> fdo::Result<Vec<(Param, Value)>> {
    let mut out = Vec::new();
    for (name, value) in given {
        let param = Param::from_name(name)
            .filter(|p| allowed.contains(p))
            .ok_or_else(|| fdo::Error::InvalidArgs(format!("no such setting: {name}")))?;
        let value = if let Ok(b) = bool::try_from(value) {
            Value::Bool(b)
        } else if let Ok(n) = f64::try_from(value) {
            Value::Number(n)
        } else {
            return Err(fdo::Error::InvalidArgs(format!("{name}: a boolean or a double is expected")));
        };
        out.push((param, value));
    }
    // The order in a dictionary means nothing; the result must not depend on it.
    out.sort_by_key(|(p, _)| p.name());
    Ok(out)
}

/// Lets only root change settings.
async fn only_root(shared: &Shared, conn: &zbus::Connection, hdr: &Header<'_>) -> fdo::Result<()> {
    if shared.open {
        return Ok(());
    }
    let sender = hdr.sender().ok_or_else(|| fdo::Error::AccessDenied("no sender".into()))?.to_string();
    let reply = conn
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "GetConnectionUnixUser",
            &(sender.as_str(),),
        )
        .await
        .map_err(|e| fdo::Error::AccessDenied(format!("who is calling: {e}")))?;
    match reply.body().deserialize::<u32>() {
        Ok(0) => Ok(()),
        _ => Err(fdo::Error::AccessDenied("only root changes the digitizer's settings (use sp9-helperd)".into())),
    }
}

pub struct Pen(pub Arc<Shared>);

#[zbus::interface(name = "io.github.joonhoekim.Sp9Digitizer1.Pen")]
impl Pen {
    /// Contact without force, from the strength of the tip signal.
    #[zbus(property(emits_changed_signal = "false"))]
    fn zero_force(&self) -> bool {
        self.0.flag(Param::ZeroForce)
    }
    /// Levels (fractions of the reference) at which such a contact begins and ends.
    #[zbus(property(emits_changed_signal = "false"))]
    fn zero_force_enter(&self) -> f64 {
        self.0.number(Param::ZeroForceEnter)
    }
    #[zbus(property(emits_changed_signal = "false"))]
    fn zero_force_exit(&self) -> f64 {
        self.0.number(Param::ZeroForceExit)
    }
    /// The configured reference; 0 = learned (see LearnedReference).
    #[zbus(property(emits_changed_signal = "false"))]
    fn zero_force_reference(&self) -> f64 {
        self.0.number(Param::ZeroForceReference)
    }
    #[zbus(property(emits_changed_signal = "false"))]
    fn zero_force_stroke_level(&self) -> f64 {
        self.0.number(Param::ZeroForceStrokeLevel)
    }
    /// The lowest pressure reported in contact (fraction of the range).
    #[zbus(property(emits_changed_signal = "false"))]
    fn pressure_floor(&self) -> f64 {
        self.0.number(Param::PressureFloor)
    }

    #[zbus(property(emits_changed_signal = "false"))]
    fn in_range(&self) -> bool {
        self.0.pen().in_range
    }
    #[zbus(property(emits_changed_signal = "false"))]
    fn contact(&self) -> bool {
        self.0.pen().contact
    }
    /// "pen", "eraser", or empty while out of range.
    #[zbus(property(emits_changed_signal = "false"))]
    fn tool(&self) -> String {
        self.0.pen().tool().to_string()
    }
    #[zbus(property(emits_changed_signal = "false"))]
    fn button(&self) -> bool {
        self.0.pen().button
    }
    /// Energy of the tip signal relative to the reference in force; 0 while unknown.
    #[zbus(property(emits_changed_signal = "false"))]
    fn tip_level(&self) -> f64 {
        self.0.pen().tip_level
    }
    /// The reference in force: the configured one, or what was learned so far (0: nothing yet).
    #[zbus(property(emits_changed_signal = "false"))]
    fn learned_reference(&self) -> f64 {
        self.0.pen().reference
    }

    /// Changes pen settings by the names of the properties above (ZeroForce b; ZeroForceEnter,
    /// ZeroForceExit, ZeroForceReference, ZeroForceStrokeLevel, PressureFloor d): all of them
    /// or none. They are in force from the next report on; a stroke in progress goes on.
    async fn set_params(
        &self,
        params: HashMap<String, OwnedValue>,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> fdo::Result<()> {
        only_root(&self.0, conn, &hdr).await?;
        self.0.set(&params_of(&params, &Param::PEN)?).map_err(fdo::Error::InvalidArgs)
    }

    /// Proximity, contact, tool or button changed.
    #[zbus(signal)]
    pub async fn state_changed(emitter: &SignalEmitter<'_>, in_range: bool, contact: bool, tool: &str, button: bool) -> zbus::Result<()>;
}

pub struct Touch(pub Arc<Shared>);

#[zbus::interface(name = "io.github.joonhoekim.Sp9Digitizer1.Touch")]
impl Touch {
    /// Thresholds of the contact detection, in heatmap counts.
    #[zbus(property(emits_changed_signal = "false"))]
    fn activation_threshold(&self) -> f64 {
        self.0.number(Param::ActivationThreshold)
    }
    #[zbus(property(emits_changed_signal = "false"))]
    fn deactivation_threshold(&self) -> f64 {
        self.0.number(Param::DeactivationThreshold)
    }
    /// No touch while the pen is in range.
    #[zbus(property(emits_changed_signal = "false"))]
    fn disable_on_stylus(&self) -> bool {
        self.0.flag(Param::DisableOnStylus)
    }
    /// Seconds touch stays off after the pen left.
    #[zbus(property(emits_changed_signal = "false"))]
    fn pen_holdoff(&self) -> f64 {
        self.0.number(Param::PenHoldoff)
    }

    /// Changes touch settings by the names of the properties above: all of them or none.
    async fn set_params(
        &self,
        params: HashMap<String, OwnedValue>,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> fdo::Result<()> {
        only_root(&self.0, conn, &hdr).await?;
        self.0.set(&params_of(&params, &Param::TOUCH)?).map_err(fdo::Error::InvalidArgs)
    }
}

pub struct Diagnostics(pub Arc<Shared>);

#[zbus::interface(name = "io.github.joonhoekim.Sp9Digitizer1.Diagnostics")]
impl Diagnostics {
    #[zbus(property(emits_changed_signal = "const"))]
    fn version(&self) -> String {
        env!("CARGO_PKG_VERSION").to_string()
    }
    /// The hidraw node in use (or last used, while the digitizer is away).
    #[zbus(property(emits_changed_signal = "false"))]
    fn device(&self) -> String {
        self.0.device.lock().unwrap().clone()
    }
    /// Antennas on X and Y, width and height of the screen in centimeters.
    #[zbus(property(emits_changed_signal = "const"))]
    fn geometry(&self) -> (u8, u8, f64, f64) {
        let g = &self.0.geometry;
        (g.columns, g.rows, g.width_cm, g.height_cm)
    }
    #[zbus(property(emits_changed_signal = "false"))]
    fn reports(&self) -> u64 {
        self.0.counters.reports.load(Ordering::Relaxed)
    }
    #[zbus(property(emits_changed_signal = "false"))]
    fn parse_errors(&self) -> u64 {
        self.0.counters.parse_errors.load(Ordering::Relaxed)
    }
    #[zbus(property(emits_changed_signal = "false"))]
    fn pen_samples(&self) -> u64 {
        self.0.counters.pen_samples.load(Ordering::Relaxed)
    }
    #[zbus(property(emits_changed_signal = "false"))]
    fn touch_frames(&self) -> u64 {
        self.0.counters.touch_frames.load(Ordering::Relaxed)
    }
    #[zbus(property(emits_changed_signal = "false"))]
    fn write_errors(&self) -> u64 {
        self.0.counters.write_errors.load(Ordering::Relaxed)
    }

    /// The configuration in force, as the TOML the daemon reads.
    fn get_config(&self) -> fdo::Result<String> {
        toml::to_string(&self.0.config()).map_err(|e| fdo::Error::Failed(e.to_string()))
    }
}

/// Puts the three interfaces on a connection that is being built.
pub fn serve<'a>(builder: zbus::connection::Builder<'a>, shared: &Arc<Shared>) -> zbus::Result<zbus::connection::Builder<'a>> {
    builder
        .serve_at(control::OBJECT_PATH, Pen(shared.clone()))?
        .serve_at(control::OBJECT_PATH, Touch(shared.clone()))?
        .serve_at(control::OBJECT_PATH, Diagnostics(shared.clone()))
}

/// Emits the pen state as the `StateChanged` signal.
pub fn emit_state(conn: &zbus::Connection, state: &PenState) -> zbus::Result<()> {
    zbus::block_on(async {
        let emitter = SignalEmitter::new(conn, control::OBJECT_PATH)?;
        Pen::state_changed(&emitter, state.in_range, state.contact, state.tool(), state.button).await
    })
}

/// Runs the service on the system bus on its own thread, for as long as the sender of
/// `changes` lives. Without a bus (not up yet, name taken) the daemon works all the same: the
/// thread tries again every few seconds and drops the state changes in between.
pub fn spawn(shared: Arc<Shared>, changes: Receiver<PenState>) {
    std::thread::spawn(move || {
        let mut conn: Option<zbus::Connection> = None;
        let mut complained = false;
        loop {
            if conn.is_none() {
                let built =
                    zbus::block_on(async { serve(zbus::connection::Builder::system()?, &shared)?.name(control::BUS_NAME)?.build().await });
                match built {
                    Ok(c) => {
                        eprintln!("sp9-digitizerd: on the system bus as {}", control::BUS_NAME);
                        conn = Some(c);
                    }
                    Err(e) if !complained => {
                        eprintln!("sp9-digitizerd: no D-Bus service ({e}); trying again every {} s", RETRY.as_secs());
                        complained = true;
                    }
                    Err(_) => {}
                }
            }
            match changes.recv_timeout(RETRY) {
                Ok(state) => {
                    if let Some(c) = &conn {
                        if let Err(e) = emit_state(c, &state) {
                            eprintln!("sp9-digitizerd: D-Bus: {e}");
                            conn = None;
                        }
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    });
}

/// The sending side of [`spawn`], for the processing loop.
pub type Changes = Sender<PenState>;

#[cfg(test)]
mod tests {
    //! The D-Bus surface end to end on a peer-to-peer connection, the root check skipped.
    use super::*;
    use futures_lite::StreamExt;
    use zbus::connection::Builder;
    use zbus::zvariant::Value as V;

    struct Rig {
        client: zbus::Connection,
        server: zbus::Connection,
        shared: Arc<Shared>,
    }

    fn rig() -> Rig {
        let shared = Arc::new(Shared::new(Config::default(), "/dev/hidraw7", Geometry::SP9, true));
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let client_stream = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (server_stream, _) = listener.accept().unwrap();
        let guid = zbus::Guid::generate();
        let s = shared.clone();
        let server = std::thread::spawn(move || {
            let bld =
                Builder::async_io_tcp_stream(server_stream).server(guid).unwrap().p2p().auth_mechanism(zbus::AuthMechanism::Anonymous);
            zbus::block_on(serve(bld, &s).unwrap().build()).unwrap()
        });
        let client =
            zbus::block_on(Builder::async_io_tcp_stream(client_stream).p2p().auth_mechanism(zbus::AuthMechanism::Anonymous).build())
                .unwrap();
        Rig { client, server: server.join().unwrap(), shared }
    }

    impl Rig {
        fn get(&self, interface: &str, property: &str) -> OwnedValue {
            let r = zbus::block_on(self.client.call_method(
                None::<&str>,
                control::OBJECT_PATH,
                Some("org.freedesktop.DBus.Properties"),
                "Get",
                &(interface, property),
            ))
            .unwrap();
            r.body().deserialize::<OwnedValue>().unwrap()
        }

        fn number(&self, interface: &str, property: &str) -> f64 {
            f64::try_from(self.get(interface, property)).unwrap()
        }

        fn flag(&self, interface: &str, property: &str) -> bool {
            bool::try_from(self.get(interface, property)).unwrap()
        }

        fn set(&self, interface: &str, params: &[(&str, V<'_>)]) -> Result<(), String> {
            let map: HashMap<&str, &V<'_>> = params.iter().map(|(k, v)| (*k, v)).collect();
            match zbus::block_on(self.client.call_method(None::<&str>, control::OBJECT_PATH, Some(interface), "SetParams", &(map,))) {
                Ok(_) => Ok(()),
                Err(zbus::Error::MethodError(name, _, _)) => Err(name.to_string()),
                Err(e) => Err(e.to_string()),
            }
        }
    }

    #[test]
    fn pen_settings() {
        let r = rig();
        let pen = control::PEN_INTERFACE;
        assert!(!r.flag(pen, "ZeroForce"));
        assert_eq!(r.number(pen, "ZeroForceEnter"), 0.85);
        assert_eq!(r.number(pen, "ZeroForceStrokeLevel"), 0.8);
        assert!(r.shared.take_changed_config().is_none());

        r.set(pen, &[("ZeroForce", V::from(true)), ("ZeroForceReference", V::from(13.4e6)), ("PressureFloor", V::from(0.03))]).unwrap();
        assert!(r.flag(pen, "ZeroForce"));
        assert_eq!(r.number(pen, "ZeroForceReference"), 13.4e6);
        assert_eq!(r.number(pen, "PressureFloor"), 0.03);
        // The loop takes the change once.
        let taken = r.shared.take_changed_config().unwrap();
        assert!(taken.pen.contact.zero_force.enabled);
        assert!(r.shared.take_changed_config().is_none());

        // Refused as a whole: a bad combination, a wrong type, a setting of the other
        // interface, a name that does not exist.
        let invalid = "org.freedesktop.DBus.Error.InvalidArgs";
        assert_eq!(r.set(pen, &[("PressureFloor", V::from(0.1)), ("ZeroForceExit", V::from(0.95))]).unwrap_err(), invalid);
        assert_eq!(r.set(pen, &[("ZeroForce", V::from(1.0))]).unwrap_err(), invalid);
        assert_eq!(r.set(pen, &[("ZeroForceEnter", V::from("high"))]).unwrap_err(), invalid);
        assert_eq!(r.set(pen, &[("ActivationThreshold", V::from(30.0))]).unwrap_err(), invalid);
        assert_eq!(r.set(pen, &[("InkFilter", V::from(true))]).unwrap_err(), invalid);
        assert_eq!(r.number(pen, "PressureFloor"), 0.03);
        assert!(r.shared.take_changed_config().is_none());

        // Back to a learned reference.
        r.set(pen, &[("ZeroForceReference", V::from(0.0))]).unwrap();
        assert_eq!(r.number(pen, "ZeroForceReference"), 0.0);
    }

    #[test]
    fn touch_settings() {
        let r = rig();
        let touch = control::TOUCH_INTERFACE;
        assert_eq!((r.number(touch, "ActivationThreshold"), r.number(touch, "DeactivationThreshold")), (24.0, 20.0));
        assert!(r.flag(touch, "DisableOnStylus"));
        assert_eq!(r.number(touch, "PenHoldoff"), 0.2);

        r.set(
            touch,
            &[
                ("ActivationThreshold", V::from(22.0)),
                ("DeactivationThreshold", V::from(18.0)),
                ("DisableOnStylus", V::from(false)),
                ("PenHoldoff", V::from(0.3)),
            ],
        )
        .unwrap();
        let c = r.shared.take_changed_config().unwrap();
        assert_eq!((c.touch.contacts.activation_threshold, c.touch.contacts.deactivation_threshold), (22.0, 18.0));
        assert!(!c.touch.disable_on_stylus);
        assert_eq!(c.touch.pen_holdoff_s, 0.3);

        assert!(r.set(touch, &[("DeactivationThreshold", V::from(30.0))]).is_err());
        assert!(r.set(touch, &[("ZeroForce", V::from(true))]).is_err());
        assert_eq!(r.number(touch, "DeactivationThreshold"), 18.0);
    }

    #[test]
    fn pen_state_and_signal() {
        let r = rig();
        let pen = control::PEN_INTERFACE;
        assert!(!r.flag(pen, "InRange"));
        assert_eq!(String::try_from(r.get(pen, "Tool")).unwrap(), "");

        let mut signals = zbus::MessageStream::from(&r.client);
        let state = PenState { in_range: true, contact: true, eraser: true, button: false, tip_level: 1.25, reference: 13.4e6 };
        assert!(r.shared.store_pen(state));
        // The same events again, another level: stored, but not a change to announce.
        assert!(!r.shared.store_pen(PenState { tip_level: 1.5, ..state }));
        emit_state(&r.server, &state).unwrap();

        assert!(r.flag(pen, "InRange") && r.flag(pen, "Contact") && !r.flag(pen, "Button"));
        assert_eq!(String::try_from(r.get(pen, "Tool")).unwrap(), "eraser");
        assert_eq!(r.number(pen, "TipLevel"), 1.5);
        assert_eq!(r.number(pen, "LearnedReference"), 13.4e6);

        let signal = zbus::block_on(async {
            loop {
                let m = signals.next().await.unwrap().unwrap();
                if m.header().member().is_some_and(|n| n.as_str() == "StateChanged") {
                    return m;
                }
            }
        });
        assert_eq!(signal.header().interface().unwrap().as_str(), pen);
        assert_eq!(signal.body().deserialize::<(bool, bool, String, bool)>().unwrap(), (true, true, "eraser".to_string(), false));
    }

    #[test]
    fn diagnostics() {
        let r = rig();
        let diag = control::DIAGNOSTICS_INTERFACE;
        assert_eq!(String::try_from(r.get(diag, "Device")).unwrap(), "/dev/hidraw7");
        r.shared.set_device("/dev/hidraw0");
        assert_eq!(String::try_from(r.get(diag, "Device")).unwrap(), "/dev/hidraw0");
        assert_eq!(<(u8, u8, f64, f64)>::try_from(r.get(diag, "Geometry")).unwrap(), (68, 46, 27.389, 18.259));
        r.shared.counters.reports.fetch_add(41, Ordering::Relaxed);
        r.shared.counters.parse_errors.fetch_add(2, Ordering::Relaxed);
        assert_eq!(u64::try_from(r.get(diag, "Reports")).unwrap(), 41);
        assert_eq!(u64::try_from(r.get(diag, "ParseErrors")).unwrap(), 2);
        assert_eq!(u64::try_from(r.get(diag, "WriteErrors")).unwrap(), 0);

        r.set(control::PEN_INTERFACE, &[("PressureFloor", V::from(0.07))]).unwrap();
        let reply = zbus::block_on(r.client.call_method(None::<&str>, control::OBJECT_PATH, Some(diag), "GetConfig", &())).unwrap();
        let text: String = reply.body().deserialize().unwrap();
        let config: Config = toml::from_str(&text).unwrap();
        assert_eq!(config.pen.pressure.floor, 0.07);
        assert_eq!(config, r.shared.config());
    }
}
