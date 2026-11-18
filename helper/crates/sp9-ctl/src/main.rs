// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
//! sp9-ctl: command-line client of sp9-helperd (`io.github.joonhoekim.OpenDeviceHelper1`).
//! Every change goes through the daemon's methods; polkit decides, the agent may ask for a
//! password.

use serde_json::{json, Map, Value as J};
use std::collections::HashMap;
use std::process::exit;
use zbus::blocking::{Connection, Proxy};
use zbus::proxy::MethodFlags;
use zbus::zvariant::{DynamicType, OwnedValue, Value};

const BUS: &str = "io.github.joonhoekim.OpenDeviceHelper1";
const ROOT: &str = "/io/github/joonhoekim/OpenDeviceHelper1";
/// Pen speed (mm/s) at the largest speed value that feels about as strong as on Windows: `pen strength 100`.
const FULL_SCALE_100: f64 = 290.0;

const USAGE: &str = "usage: sp9-ctl [--json] [--session] COMMAND
  status                                 everything the daemon exports
  profile [NAME]                         platform_profile: low-power balanced balanced-performance performance
  follow on|off                          apply the profile's RAPL limits on profile / power source changes
  epp on|off                             write the profile's EPP too (held for 5 s against other writers,
                                         e.g. power-profiles-daemon); off leaves the EPP to them
  limits                                 the table in force (W, s, EPP)
  limits set PROFILE ac|dc PL1 PL2 TAU EPP
  limits reset PROFILE ac|dc             back to Windows' values
  skin-guard [on|off]                    experimental: lower PL1/PL2 by the VTS1 skin temperature
  fan                                    rpm, temperatures, request state
  fan allow on|off                       experimental opt-in for the fan rpm request (SAM FAN 0x0b)
  fan request RPM | fan release          1600..7500, release = 0. The firmware follows it only while
                                         it is not driving the fan itself (idle, light load); suspend
                                         and a daemon restart cancel it. FanRequestState in `fan` tells
                                         whether the fan follows: settling, honoured, not-honoured
  battery [release]                      state; release = charge to 100 % once (smart charging)
  psr [AC DC]                            PSR policy per power source: default | off | psr1
  icc                                    write the panel's native ICC profile from the UEFI calibration
  kbd [LEVEL]                            keyboard backlight (levels: see kbd)
  touch-off [on|off]                     touchscreen off while the panel is off
  iptsd [ACT DEACT [on|off|preset]]      iptsd thresholds over the preset; iptsd preset = remove
  pen                                    Slim Pen 2: haptics, digitizer feedback, cover coupling
  pen ink on|off                         haptics as a whole: inking haptics and the speed feedback
  pen intensity N | pen test             inking intensity 1..255; one click
  pen speed on|off                       pen speed to the digitizer (continuous waveforms need it)
  pen strength PERCENT                   25..400, 100 = about as on Windows (full scale 290 mm/s)
  pen full-scale MM_S                    the same setting as the speed that gives the strongest
                                         vibration, 50..2000 mm/s
  pen auto-couple on|off                 couple a pen put into the cover slot (Bluetooth bond)
  pen couple                             couple the pen in the cover slot now, replacing its bond
  diagnostics                            run the post-install check, print the tarball path
  reload                                 read /etc/sp9/helper.toml again";

struct Ctl {
    conn: Connection,
    json: bool,
}

fn die(msg: impl std::fmt::Display) -> ! {
    eprintln!("sp9-ctl: {msg}");
    exit(1)
}

fn iface(obj: &str) -> String {
    if obj.is_empty() {
        BUS.to_string()
    } else {
        format!("{BUS}.{obj}")
    }
}

fn path(obj: &str) -> String {
    if obj.is_empty() {
        ROOT.to_string()
    } else {
        format!("{ROOT}/{obj}")
    }
}

fn to_json(v: &Value) -> J {
    match v {
        Value::U8(x) => json!(x),
        Value::Bool(x) => json!(x),
        Value::I16(x) => json!(x),
        Value::U16(x) => json!(x),
        Value::I32(x) => json!(x),
        Value::U32(x) => json!(x),
        Value::I64(x) => json!(x),
        Value::U64(x) => json!(x),
        Value::F64(x) => {
            if x.is_finite() {
                json!(x)
            } else {
                J::Null
            }
        }
        Value::Str(s) => json!(s.as_str()),
        Value::ObjectPath(p) => json!(p.as_str()),
        Value::Value(b) => to_json(b),
        Value::Array(a) => J::Array(a.iter().map(to_json).collect()),
        Value::Structure(s) => J::Array(s.fields().iter().map(to_json).collect()),
        Value::Dict(d) => {
            let mut m: Vec<(String, J)> =
                d.iter().map(|(k, v)| (if let Value::Str(s) = k { s.to_string() } else { format!("{k:?}") }, to_json(v))).collect();
            m.sort_by(|a, b| a.0.cmp(&b.0));
            J::Object(m.into_iter().collect())
        }
        other => json!(format!("{other:?}")),
    }
}

fn text(v: &J) -> String {
    match v {
        J::String(s) => s.clone(),
        J::Null => "?".into(),
        J::Number(n) => match n.as_f64() {
            Some(f) if n.is_f64() => format!("{}", (f * 100.0).round() / 100.0),
            _ => n.to_string(),
        },
        J::Array(a) => format!("[{}]", a.iter().map(text).collect::<Vec<_>>().join(", ")),
        J::Object(m) => m.iter().map(|(k, v)| format!("{k}={}", text(v))).collect::<Vec<_>>().join(" "),
        other => other.to_string(),
    }
}

impl Ctl {
    fn proxy(&self, obj: &str) -> Proxy<'_> {
        Proxy::new(&self.conn, BUS, path(obj), iface(obj)).unwrap_or_else(|e| die(e))
    }

    fn props(&self, obj: &str) -> Map<String, J> {
        let p = Proxy::new(&self.conn, BUS, path(obj), "org.freedesktop.DBus.Properties").unwrap_or_else(|e| die(e));
        let all: HashMap<String, OwnedValue> = p.call("GetAll", &(iface(obj),)).unwrap_or_else(|e| die(e));
        let mut keys: Vec<&String> = all.keys().collect();
        keys.sort();
        keys.into_iter().map(|k| (k.clone(), to_json(&all[k]))).collect()
    }

    fn call<B, R>(&self, obj: &str, method: &str, body: &B) -> Option<R>
    where
        B: zbus::export::serde::Serialize + DynamicType,
        R: for<'d> zbus::zvariant::DynamicDeserialize<'d>,
    {
        self.proxy(obj).call_with_flags::<_, B, R>(method, MethodFlags::AllowInteractiveAuth.into(), body).unwrap_or_else(|e| match e {
            zbus::Error::MethodError(name, msg, _) => die(format!("{}: {}", name.as_str(), msg.unwrap_or_default())),
            e => die(e),
        })
    }

    fn act<B: zbus::export::serde::Serialize + DynamicType>(&self, obj: &str, method: &str, body: &B) {
        let _: Option<()> = self.call(obj, method, body);
    }

    fn show(&self, obj: &str, only: &[&str]) {
        let p = self.props(obj);
        let p: Map<String, J> = p.into_iter().filter(|(k, _)| only.is_empty() || only.contains(&k.as_str())).collect();
        if self.json {
            println!("{}", J::Object(p));
        } else {
            for (k, v) in p {
                println!("{k:28} {}", text(&v));
            }
        }
    }

    fn print_string(&self, s: Option<String>) {
        let s = s.unwrap_or_default();
        if self.json {
            println!("{}", json!(s));
        } else {
            println!("{s}");
        }
    }
}

fn onoff(s: Option<&String>) -> bool {
    match s.map(String::as_str) {
        Some("on") => true,
        Some("off") => false,
        _ => die("expected on or off"),
    }
}

fn num<T: std::str::FromStr>(s: Option<&String>, what: &str) -> T {
    s.and_then(|v| v.parse().ok()).unwrap_or_else(|| die(format!("expected a number for {what}")))
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let json = args.iter().any(|a| a == "--json");
    let session = args.iter().any(|a| a == "--session");
    args.retain(|a| a != "--json" && a != "--session");
    if args.is_empty() || args[0] == "-h" || args[0] == "--help" {
        println!("{USAGE}");
        return;
    }
    let conn = if session { Connection::session() } else { Connection::system() }.unwrap_or_else(|e| die(e));
    let c = Ctl { conn, json };
    let a = |i: usize| args.get(i);
    match args[0].as_str() {
        "status" => {
            let root = c.props("");
            let features: Vec<String> = root
                .get("Features")
                .and_then(|v| v.as_array())
                .map(|v| v.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                .unwrap_or_default();
            if json {
                let mut all = Map::new();
                all.insert("Helper".into(), J::Object(root));
                for f in &features {
                    all.insert(f.clone(), J::Object(c.props(f)));
                }
                println!("{}", J::Object(all));
            } else {
                for (k, v) in &root {
                    println!("{k:28} {}", text(v));
                }
                for f in &features {
                    println!("\n[{f}]");
                    c.show(f, &[]);
                }
            }
        }
        "profile" => match a(1) {
            Some(p) => c.act("Performance", "SetProfile", &(p.as_str(),)),
            None => c.show("Performance", &["Profile", "ProfileChoices", "OnAc", "FollowProfile"]),
        },
        "follow" => c.act("Performance", "SetFollowProfile", &(onoff(a(1)),)),
        "epp" => c.act("Performance", "SetEppEnabled", &(onoff(a(1)),)),
        "limits" => match a(1).map(String::as_str) {
            None => c.show(
                "Performance",
                &["LimitTable", "Pl1W", "Pl2W", "TauS", "RaplZones", "TargetPl1W", "TargetPl2W", "TargetTauS", "TargetEpp", "Epp"],
            ),
            Some("set") => {
                let (p, s) = (a(2).cloned().unwrap_or_else(|| die("profile?")), a(3).cloned().unwrap_or_else(|| die("ac or dc?")));
                let body = (p, s, num::<f64>(a(4), "PL1"), num::<f64>(a(5), "PL2"), num::<f64>(a(6), "tau"), num::<u8>(a(7), "EPP"));
                c.act("Performance", "SetLimits", &body)
            }
            Some("reset") => {
                let (p, s) = (a(2).cloned().unwrap_or_else(|| die("profile?")), a(3).cloned().unwrap_or_else(|| die("ac or dc?")));
                c.act("Performance", "ResetLimits", &(p, s))
            }
            Some(x) => die(format!("unknown limits command {x}")),
        },
        "skin-guard" => match a(1) {
            Some(_) => c.act("Performance", "SetSkinGuard", &(onoff(a(1)),)),
            None => c.show("Performance", &["SkinGuard", "SkinGuardActive", "SkinSensor", "SkinTempC", "Pl1W", "Pl2W"]),
        },
        "fan" => match a(1).map(String::as_str) {
            None => c.show("Performance", &["FanRpm", "FanControlAllowed", "FanRpmRequest", "FanRequestState", "Temperatures"]),
            Some("allow") => c.act("Performance", "SetFanControlAllowed", &(onoff(a(2)),)),
            Some("request") => c.act("Performance", "RequestFanRpm", &(num::<f64>(a(2), "rpm"),)),
            Some("release") => c.act("Performance", "RequestFanRpm", &(0.0f64,)),
            Some(x) => die(format!("unknown fan command {x}")),
        },
        "battery" => match a(1).map(String::as_str) {
            None => c.show("Battery", &[]),
            Some("release") => {
                let r: Option<String> = c.call("Battery", "ReleaseSmartCharging", &());
                c.print_string(r)
            }
            Some(x) => die(format!("unknown battery command {x}")),
        },
        "psr" => match (a(1), a(2)) {
            (Some(ac), Some(dc)) => c.act("Display", "SetPsrPolicy", &(ac.as_str(), dc.as_str())),
            (None, _) => c.show("Display", &["EnablePsr", "PsrStatus", "PsrAc", "PsrDc", "PsrWritten"]),
            _ => die("psr AC DC"),
        },
        "icc" => {
            let r: Option<String> = c.call("Display", "GenerateIcc", &());
            c.print_string(r)
        }
        "kbd" => match a(1) {
            Some(_) => c.act("Input", "SetKeyboardBacklight", &(num::<u32>(a(1), "level"),)),
            None => c.show("Input", &["KeyboardBacklightAvailable", "KeyboardBacklight", "KeyboardBacklightLevels"]),
        },
        "touch-off" => match a(1) {
            Some(_) => c.act("Input", "SetTouchOffWhenDisplayOff", &(onoff(a(1)),)),
            None => c.show("Input", &["TouchOffWhenDisplayOff", "TouchInhibited"]),
        },
        "iptsd" => match a(1).map(String::as_str) {
            None => c.show("Input", &["IptsdActivationThreshold", "IptsdDeactivationThreshold", "IptsdDisableOnStylus"]),
            Some("preset") => c.act("Input", "SetIptsd", &(-1.0f64, -1.0f64, -1i32)),
            Some(_) => {
                let dos = match a(3).map(String::as_str) {
                    None | Some("preset") => -1,
                    Some("on") => 1,
                    Some("off") => 0,
                    Some(x) => die(format!("DisableOnStylus: on, off or preset, not {x}")),
                };
                c.act("Input", "SetIptsd", &(num::<f64>(a(1), "activation"), num::<f64>(a(2), "deactivation"), dos))
            }
        },
        "pen" => match a(1).map(String::as_str) {
            None => c.show("Pen", &[]),
            Some("ink") => c.act("Pen", "SetInkFeedback", &(onoff(a(2)),)),
            Some("intensity") => c.act("Pen", "SetIntensity", &(num::<u32>(a(2), "intensity"),)),
            Some("test") => c.act("Pen", "TestPulse", &()),
            Some("speed") => c.act("Pen", "SetSpeedFeedback", &(onoff(a(2)),)),
            Some("strength") => {
                let pct = num::<f64>(a(2), "strength");
                if !(25.0..=400.0).contains(&pct) {
                    die("strength: 25..400 percent");
                }
                c.act("Pen", "SetSpeedFullScale", &(FULL_SCALE_100 * 100.0 / pct,))
            }
            Some("full-scale") => c.act("Pen", "SetSpeedFullScale", &(num::<f64>(a(2), "full scale"),)),
            Some("auto-couple") => c.act("Pen", "SetAutoCouple", &(onoff(a(2)),)),
            Some("couple") => {
                c.act("Pen", "Couple", &());
                // the daemon works on its own thread: wait for the attempt to end
                let state = |c: &Ctl| c.props("Pen").get("CoupleState").and_then(|v| v.as_str().map(String::from)).unwrap_or_default();
                let mut last = String::new();
                for _ in 0..240 {
                    let s = state(&c);
                    if s != last && !json {
                        eprintln!("{s}");
                    }
                    if s == "idle" {
                        break;
                    }
                    last = s;
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }
                c.show("Pen", &["CoupleState", "CoupleResult", "CoupledAddress"]);
                let ok = c.props("Pen").get("CoupleResult").and_then(|v| v.as_str().map(|r| r.starts_with("coupled"))).unwrap_or(false);
                if !ok {
                    exit(1);
                }
            }
            Some(x) => die(format!("unknown pen command {x}")),
        },
        "diagnostics" => {
            let r: Option<String> = c.call("Diagnostics", "Run", &());
            c.print_string(r)
        }
        "reload" => {
            let w: Option<Vec<String>> = c.call("", "Reload", &());
            for l in w.unwrap_or_default() {
                println!("{l}");
            }
        }
        x => die(format!("unknown command {x}\n{USAGE}")),
    }
}
