// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
//! Blocking D-Bus client for sp9-helperd: GetAll per object, method calls with
//! interactive authorization, and a watcher thread that turns the daemon's
//! PropertiesChanged (invalidation only) and name changes into "poll now".

use crate::i18n::{n_, tr};
use std::collections::HashMap;
use zbus::blocking::Connection;
use zbus::proxy::MethodFlags;
use zbus::zvariant::{OwnedValue, Value};

pub const BUS: &str = "io.github.joonhoekim.OpenDeviceHelper1";
pub const ROOT: &str = "/io/github/joonhoekim/OpenDeviceHelper1";
/// Root.Device of this helper (the TB323FU helper owns the same bus name).
pub const DEVICE: &str = "surface-pro-9";

pub type Props = HashMap<String, OwnedValue>;

/// The system bus, or the session bus for `sp9-helperd --session --fake`.
#[derive(Clone, Copy, PartialEq)]
pub enum Bus {
    System,
    Session,
}

#[derive(Clone)]
pub struct Client {
    bus: Bus,
    conn: Option<Connection>,
}

fn path(obj: &str) -> String {
    if obj.is_empty() {
        ROOT.to_string()
    } else {
        format!("{ROOT}/{obj}")
    }
}
fn iface(obj: &str) -> String {
    if obj.is_empty() {
        BUS.to_string()
    } else {
        format!("{BUS}.{obj}")
    }
}

fn open(bus: Bus) -> Option<Connection> {
    match bus {
        Bus::System => Connection::system().ok(),
        Bus::Session => Connection::session().ok(),
    }
}

impl Client {
    pub fn connect(bus: Bus) -> Self {
        Self { bus, conn: open(bus) }
    }

    pub fn bus(&self) -> Bus {
        self.bus
    }

    pub fn connected(&self) -> bool {
        self.conn.is_some()
    }

    /// All properties of one object, or None when the daemon or the object is missing.
    pub fn get_all(&self, obj: &str) -> Option<Props> {
        let c = self.conn.as_ref()?;
        let reply = c
            .call_method(Some(BUS), path(obj).as_str(), Some("org.freedesktop.DBus.Properties"), "GetAll", &(iface(obj).as_str(),))
            .ok()?;
        reply.body().deserialize().ok()
    }

    /// Call a method with ALLOW_INTERACTIVE_AUTH (polkit may ask for a password).
    /// Ok(Some(text)) when the method returns a string or a list of strings (joined
    /// by newlines). Errors come back as text for a person. Blocking: polkit's
    /// dialog can keep the call open, so callers run it on a worker thread.
    pub fn call<B>(&self, obj: &str, method: &str, body: &B) -> Result<Option<String>, String>
    where
        B: serde::ser::Serialize + zbus::zvariant::DynamicType,
    {
        let c = self.conn.as_ref().ok_or_else(|| tr("The helper service is not running").to_string())?;
        let p = path(obj);
        let i = iface(obj);
        let proxy = zbus::blocking::Proxy::new(c, BUS, p.as_str(), i.as_str()).map_err(|e| e.to_string())?;
        let flags = MethodFlags::AllowInteractiveAuth.into();
        // the helper's methods return nothing, a string, or (Reload) a list of strings
        let res = match method {
            "Reload" => proxy.call_with_flags::<_, _, Vec<String>>(method, flags, body).map(|r| r.map(|v| v.join("\n"))),
            "ReleaseSmartCharging" | "GenerateIcc" | "Run" => proxy.call_with_flags::<_, _, String>(method, flags, body),
            _ => proxy.call_with_flags::<_, _, ()>(method, flags, body).map(|_| None),
        };
        match res {
            Ok(v) => Ok(v),
            Err(zbus::Error::MethodError(name, msg, _)) => Err(human_error(name.as_str(), msg.as_deref())),
            Err(zbus::Error::InputOutput(_)) => Err(tr("The helper service is not running").to_string()),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// Block on the bus and call `poke` for every PropertiesChanged from the helper
/// and every owner change of its name (a second thread reads those). Returns
/// when the connection fails or `poke` says stop; the caller starts it again.
pub fn watch(bus: Bus, poke: impl Fn() -> bool + Clone + Send + 'static) {
    use zbus::blocking::MessageIterator;
    use zbus::message::Type;
    use zbus::MatchRule;
    let Some(conn) = open(bus) else { return };
    let changed = MatchRule::builder()
        .msg_type(Type::Signal)
        .sender(BUS)
        .and_then(|b| b.interface("org.freedesktop.DBus.Properties"))
        .and_then(|b| b.member("PropertiesChanged"))
        .map(|b| b.build());
    let owner = MatchRule::builder()
        .msg_type(Type::Signal)
        .sender("org.freedesktop.DBus")
        .and_then(|b| b.interface("org.freedesktop.DBus"))
        .and_then(|b| b.member("NameOwnerChanged"))
        .and_then(|b| b.add_arg(BUS))
        .map(|b| b.build());
    let (Ok(changed), Ok(owner)) = (changed, owner) else { return };
    if let Ok(mut it) = MessageIterator::for_match_rule(owner, &conn, Some(16)) {
        let poke = poke.clone();
        std::thread::spawn(move || {
            while let Some(Ok(_)) = it.next() {
                if !poke() {
                    return;
                }
            }
        });
    }
    let Ok(mut it) = MessageIterator::for_match_rule(changed, &conn, Some(64)) else { return };
    while let Some(Ok(_)) = it.next() {
        if !poke() {
            return;
        }
    }
}

/// D-Bus error name + message -> text for a toast.
pub fn human_error(name: &str, msg: Option<&str>) -> String {
    let short = name.rsplit('.').next().unwrap_or(name);
    match short {
        "AccessDenied" | "AuthFailed" | "InteractiveAuthorizationRequired" => tr("Authentication was canceled or denied").into(),
        "ServiceUnknown" | "NoReply" | "NameHasNoOwner" | "UnknownObject" | "Disconnected" => {
            tr("The helper service is not running").into()
        }
        "NotSupported" => match msg {
            Some(m) if !m.is_empty() => format!("{}: {m}", tr("Not available on this device")),
            _ => tr("Not available on this device").into(),
        },
        "InvalidArgs" => match msg {
            Some(m) if !m.is_empty() => format!("{}: {m}", tr("Value out of range")),
            _ => tr("Value out of range").into(),
        },
        _ => match msg {
            Some(m) if !m.is_empty() => m.to_string(),
            _ => tr("Something went wrong").into(),
        },
    }
}

/// The setting a method changes, for error toasts.
pub fn setting_name(method: &str) -> &'static str {
    tr(match method {
        "SetProfile" => n_("Profile"),
        "SetFollowProfile" => n_("Apply Profile Limits"),
        "SetEppEnabled" => n_("Set EPP"),
        "SetLimits" | "ResetLimits" => n_("Power Limits"),
        "SetSkinGuard" => n_("Skin Temperature Guard"),
        "SetFanControlAllowed" | "RequestFanRpm" => n_("Fan"),
        "ReleaseSmartCharging" => n_("Smart Charging"),
        "SetPsrPolicy" => n_("Panel Self Refresh"),
        "GenerateIcc" => n_("Color Profile"),
        "SetKeyboardBacklight" => n_("Keyboard Backlight"),
        "SetTouchOffWhenDisplayOff" => n_("Touch Screen"),
        "SetIptsd" => n_("Touch Sensitivity"),
        "SetInkFeedback" | "SetIntensity" | "SetStrength" | "SetEraserClick" | "TestPulse" => n_("Pen Haptics"),
        "SetSpeedFeedback" | "SetSpeedFullScale" => n_("Writing Texture"),
        "SetAutoCouple" | "Couple" => n_("Pen Pairing"),
        "Run" => n_("Diagnostics"),
        "Reload" => n_("Reload Settings"),
        _ => n_("Settings"),
    })
}

// ------------------------------------------------------------- values --

fn val<'a>(p: &'a Props, k: &str) -> Option<&'a Value<'static>> {
    p.get(k).map(|v| unwrap(v))
}

fn unwrap<'a, 'b>(v: &'a Value<'b>) -> &'a Value<'b> {
    match v {
        Value::Value(b) => unwrap(b),
        o => o,
    }
}

fn str_of(v: &Value<'_>) -> Option<String> {
    match unwrap(v) {
        Value::Str(s) => Some(s.to_string()),
        _ => None,
    }
}

fn f64_of(v: &Value<'_>) -> Option<f64> {
    match unwrap(v) {
        Value::F64(x) => Some(*x),
        _ => None,
    }
}

pub fn b(p: &Props, k: &str) -> Option<bool> {
    match val(p, k)? {
        Value::Bool(x) => Some(*x),
        _ => None,
    }
}

/// Integers of any width as i64 (i, u, q, y).
pub fn n(p: &Props, k: &str) -> Option<i64> {
    match val(p, k)? {
        Value::I32(x) => Some(i64::from(*x)),
        Value::U32(x) => Some(i64::from(*x)),
        Value::U16(x) => Some(i64::from(*x)),
        Value::U8(x) => Some(i64::from(*x)),
        Value::I64(x) => Some(*x),
        Value::U64(x) => i64::try_from(*x).ok(),
        _ => None,
    }
}

pub fn f(p: &Props, k: &str) -> Option<f64> {
    f64_of(val(p, k)?)
}

pub fn s(p: &Props, k: &str) -> Option<String> {
    str_of(val(p, k)?)
}

pub fn strs(p: &Props, k: &str) -> Vec<String> {
    match val(p, k) {
        Some(Value::Array(a)) => a.iter().filter_map(str_of).collect(),
        _ => Vec::new(),
    }
}

/// au as a list.
pub fn uints(p: &Props, k: &str) -> Vec<u32> {
    match val(p, k) {
        Some(Value::Array(a)) => a.iter().filter_map(|v| if let Value::U32(x) = unwrap(v) { Some(*x) } else { None }).collect(),
        _ => Vec::new(),
    }
}

fn fields<'a>(v: &'a Value<'static>) -> Option<&'a [Value<'static>]> {
    match unwrap(v) {
        Value::Structure(st) => Some(st.fields()),
        _ => None,
    }
}

/// a(sd): Performance.Temperatures (sensor, °C).
pub fn temps(p: &Props, k: &str) -> Vec<(String, f64)> {
    let mut out = Vec::new();
    if let Some(Value::Array(a)) = val(p, k) {
        for v in a.iter() {
            if let Some([name, t]) = fields(v) {
                if let (Some(name), Some(t)) = (str_of(name), f64_of(t)) {
                    out.push((name, t));
                }
            }
        }
    }
    out
}

/// One row of Performance.LimitTable.
#[derive(Clone, Debug, PartialEq)]
pub struct Limit {
    pub profile: String,
    pub power: String,
    pub pl1: f64,
    pub pl2: f64,
    pub tau: f64,
    pub epp: u8,
}

/// a(ssdddy): Performance.LimitTable.
pub fn limits(p: &Props, k: &str) -> Vec<Limit> {
    let mut out = Vec::new();
    if let Some(Value::Array(a)) = val(p, k) {
        for v in a.iter() {
            if let Some([pr, pw, l1, l2, t, e]) = fields(v) {
                if let (Some(profile), Some(power), Some(pl1), Some(pl2), Some(tau), Value::U8(epp)) =
                    (str_of(pr), str_of(pw), f64_of(l1), f64_of(l2), f64_of(t), unwrap(e))
                {
                    out.push(Limit { profile, power, pl1, pl2, tau, epp: *epp });
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn props(k: &str, v: Value<'static>) -> Props {
        let mut p = Props::new();
        p.insert(k.into(), OwnedValue::try_from(v).unwrap());
        p
    }

    #[test]
    fn limit_table_parses() {
        let rows = vec![("balanced".to_string(), "ac".to_string(), 28.0, 35.0, 28.0, 153u8)];
        let p = props("LimitTable", Value::from(rows));
        assert_eq!(
            limits(&p, "LimitTable"),
            vec![Limit { profile: "balanced".into(), power: "ac".into(), pl1: 28.0, pl2: 35.0, tau: 28.0, epp: 153 }]
        );
    }

    #[test]
    fn temps_and_scalars() {
        let p = props("Temperatures", Value::from(vec![("VTS1".to_string(), 41.5f64)]));
        assert_eq!(temps(&p, "Temperatures"), vec![("VTS1".to_string(), 41.5)]);
        assert_eq!(n(&props("S", Value::U16(0x0440)), "S"), Some(0x0440));
        assert_eq!(uints(&props("L", Value::from(vec![0u32, 3, 6, 12])), "L"), vec![0, 3, 6, 12]);
    }
}
