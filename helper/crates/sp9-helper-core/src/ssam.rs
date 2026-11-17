// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
//! Requests to the Surface Aggregator Module through `surface_aggregator_cdev`
//! (`/dev/surface/aggregator`, ioctl `SSAM_CDEV_REQUEST`).
//!
//! Only the requests below are sent (docs/research/control/battery-charging.md 1.1, power-thermal.md
//! 1.4). The battery ones are those Windows sends; the fan one was measured on Linux:
//!
//! | what | TC/TID/IID CID | payload / response |
//! |---|---|---|
//! | protection policy | SAM 0x01/1/0 0x3a | → u8 (0x10 BatteryChargeLimit, 0x01 BatteryLimit) |
//! | BCL status | SAM 0x01/1/0 0x41 | → u16, engaged = `& 0x0440` |
//! | BCL countdown | SAM 0x01/1/0 0x42 | → u32 s |
//! | BCL release | SAM 0x01/1/0 0x43 | → u32 (0 released, 1 counting down, 2 not engaged) |
//! | fan rpm request | FAN 0x05/1/1 0x0b | u16 rpm (0 = none), no response |

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub const CDEV: &str = "/dev/surface/aggregator";

pub const TC_SAM: u8 = 0x01;
pub const TC_FAN: u8 = 0x05;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rqst {
    pub tc: u8,
    pub tid: u8,
    pub iid: u8,
    pub cid: u8,
}

impl Rqst {
    pub const fn new(tc: u8, tid: u8, iid: u8, cid: u8) -> Self {
        Rqst { tc, tid, iid, cid }
    }
}

pub const POLICY: Rqst = Rqst::new(TC_SAM, 1, 0, 0x3a);
pub const BCL_STATUS: Rqst = Rqst::new(TC_SAM, 1, 0, 0x41);
pub const BCL_COUNTDOWN: Rqst = Rqst::new(TC_SAM, 1, 0, 0x42);
pub const BCL_RELEASE: Rqst = Rqst::new(TC_SAM, 1, 0, 0x43);
/// `SetMinFanRpm`. Not 0x0c: that one does not move the fan, as float32 or as u16 (measured).
pub const FAN_RPM_REQUEST: Rqst = Rqst::new(TC_FAN, 1, 1, 0x0b);

pub const POLICY_BATTERY_LIMIT: u8 = 0x01;
pub const POLICY_BCL: u8 = 0x10;
pub const BCL_ENGAGED_MASK: u16 = 0x0440;

/// Range of the fan rpm request (0 = no request): the span measured on this device. 1500 /
/// 3000 / 4600 / 5000 / 6000 / 7500 requested gave 1588 / 2977 / 4569 / 4988 / 5958 / 7407 rpm
/// (idle, 22–25 s after each request). The fan does not go below about 1600 (1500 → 1588), and
/// nothing above 7500 was tried. The lower end needs no thermal margin: the SAM ignores the
/// request while it drives the fan itself, so a low value cannot hold the fan back under load.
pub const FAN_RPM_MIN: u16 = 1600;
pub const FAN_RPM_MAX: u16 = 7500;

pub trait Ssam: Send + Sync {
    fn available(&self) -> bool;
    /// Send a request. `response` > 0: the request has a response of at most that many bytes
    /// (returned); 0: no response.
    fn request(&self, r: Rqst, payload: &[u8], response: usize) -> io::Result<Vec<u8>>;
}

/// The character device.
pub struct Cdev {
    path: PathBuf,
}

impl Cdev {
    pub fn new(path: PathBuf) -> Self {
        Cdev { path }
    }
}

/// `struct ssam_cdev_request` (include/uapi/linux/surface_aggregator/cdev.h), packed, 40 B.
#[repr(C, packed)]
#[derive(Default)]
#[allow(dead_code)]
struct CdevRequest {
    target_category: u8,
    target_id: u8,
    command_id: u8,
    instance_id: u8,
    flags: u16,
    status: i16,
    payload_data: u64,
    payload_length: u16,
    payload_pad: [u8; 6],
    response_data: u64,
    response_length: u16,
    response_pad: [u8; 6],
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
const SSAM_CDEV_REQUEST_HAS_RESPONSE: u16 = 0x01;

/// `_IOWR(0xA5, 1, struct ssam_cdev_request)`.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
const fn iowr(ty: u32, nr: u32, size: u32) -> u32 {
    (3 << 30) | (size << 16) | (ty << 8) | nr
}
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
const SSAM_CDEV_REQUEST: u32 = iowr(0xA5, 1, std::mem::size_of::<CdevRequest>() as u32);

impl Ssam for Cdev {
    fn available(&self) -> bool {
        self.path.exists()
    }

    #[cfg(target_os = "linux")]
    fn request(&self, r: Rqst, payload: &[u8], response: usize) -> io::Result<Vec<u8>> {
        use std::os::fd::AsRawFd;
        let f = std::fs::OpenOptions::new().read(true).write(true).open(&self.path)?;
        let mut rsp = vec![0u8; response];
        let mut q = CdevRequest {
            target_category: r.tc,
            target_id: r.tid,
            command_id: r.cid,
            instance_id: r.iid,
            flags: if response > 0 { SSAM_CDEV_REQUEST_HAS_RESPONSE } else { 0 },
            payload_data: if payload.is_empty() { 0 } else { payload.as_ptr() as u64 },
            payload_length: payload.len() as u16,
            response_data: if response > 0 { rsp.as_mut_ptr() as u64 } else { 0 },
            response_length: response as u16,
            ..Default::default()
        };
        // SAFETY: q is a valid ssam_cdev_request; the payload and response buffers outlive
        // the call and their lengths are given in q.
        let ret = unsafe { libc::ioctl(f.as_raw_fd(), SSAM_CDEV_REQUEST as _, &mut q as *mut CdevRequest) };
        if ret < 0 {
            return Err(io::Error::last_os_error());
        }
        let status = q.status;
        if status < 0 {
            return Err(io::Error::from_raw_os_error(-status as i32));
        }
        let n = q.response_length as usize;
        rsp.truncate(n.min(response));
        Ok(rsp)
    }

    #[cfg(not(target_os = "linux"))]
    fn request(&self, _r: Rqst, _payload: &[u8], _response: usize) -> io::Result<Vec<u8>> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "SSAM requests need Linux"))
    }
}

#[derive(Clone, Debug, Default)]
pub struct FakeSsamState {
    pub available: bool,
    pub responses: HashMap<Rqst, Vec<u8>>,
    pub sent: Vec<(Rqst, Vec<u8>)>,
}

#[derive(Clone, Default)]
pub struct FakeSsam(pub Arc<Mutex<FakeSsamState>>);

impl FakeSsam {
    /// What this SP9 answered on Windows: policy 0x10, BCL not engaged, countdown 0,
    /// release "not engaged".
    pub fn sp9() -> Self {
        let mut s = FakeSsamState { available: true, ..Default::default() };
        s.responses.insert(POLICY, vec![0x10]);
        s.responses.insert(BCL_STATUS, vec![0, 0]);
        s.responses.insert(BCL_COUNTDOWN, vec![0; 4]);
        s.responses.insert(BCL_RELEASE, 2u32.to_le_bytes().to_vec());
        FakeSsam(Arc::new(Mutex::new(s)))
    }
}

impl Ssam for FakeSsam {
    fn available(&self) -> bool {
        self.0.lock().unwrap().available
    }
    fn request(&self, r: Rqst, payload: &[u8], response: usize) -> io::Result<Vec<u8>> {
        let mut s = self.0.lock().unwrap();
        if !s.available {
            return Err(io::Error::new(io::ErrorKind::NotFound, "no cdev"));
        }
        s.sent.push((r, payload.to_vec()));
        if response == 0 {
            return Ok(vec![]);
        }
        s.responses.get(&r).cloned().ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "no response"))
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct SmartCharging {
    /// policy bit 0x10: the firmware does the 80 % hold
    pub supported: bool,
    /// policy bit 0x01: the UEFI 50 % limit is on
    pub battery_limit: bool,
    pub engaged: bool,
    pub status_raw: u16,
    pub countdown_s: u32,
}

fn le(v: &[u8], n: usize) -> io::Result<u64> {
    if v.len() < n {
        return Err(io::Error::new(io::ErrorKind::InvalidData, format!("short SAM response ({} of {n} B)", v.len())));
    }
    Ok(v[..n].iter().rev().fold(0u64, |a, b| (a << 8) | *b as u64))
}

pub fn smart_charging(s: &dyn Ssam) -> io::Result<SmartCharging> {
    let policy = le(&s.request(POLICY, &[], 1)?, 1)? as u8;
    let mut out =
        SmartCharging { supported: policy & POLICY_BCL != 0, battery_limit: policy & POLICY_BATTERY_LIMIT != 0, ..Default::default() };
    if out.supported {
        out.status_raw = le(&s.request(BCL_STATUS, &[], 2)?, 2)? as u16;
        out.engaged = out.status_raw & BCL_ENGAGED_MASK != 0;
        out.countdown_s = le(&s.request(BCL_COUNTDOWN, &[], 4)?, 4)? as u32;
    }
    Ok(out)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Release {
    Released,
    CountingDown,
    NotEngaged,
    Other(u32),
}

impl Release {
    pub fn as_str(&self) -> String {
        match self {
            Release::Released => "released".into(),
            Release::CountingDown => "counting-down".into(),
            Release::NotEngaged => "not-engaged".into(),
            Release::Other(v) => format!("other:{v}"),
        }
    }
}

/// Charge to 100 % once (the Surface app's "smart charging" off). There is no request to turn
/// it back on; the firmware engages it again by itself.
pub fn release_smart_charging(s: &dyn Ssam) -> io::Result<Release> {
    Ok(match le(&s.request(BCL_RELEASE, &[], 4)?, 4)? as u32 {
        0 => Release::Released,
        1 => Release::CountingDown,
        2 => Release::NotEngaged,
        v => Release::Other(v),
    })
}

/// FAN CID 0x0b with a little-endian u16 rpm; 0 withdraws the request and hands the fan back
/// to the SAM. **Experimental.** What the SAM does with it (measured):
///
/// - It takes effect only while the SAM's own fan target is zero. While the SAM drives the fan
///   (about 5950 rpm under load) 7000 and 2000 changed nothing, so this is neither a floor nor
///   a ceiling: it can spin an idle fan up, nothing else.
/// - The kernel's surface_platform_profile sends 0x0b = 0 at probe and on resume, which
///   cancels a request without the sender being told.
///
/// The request always returns status 0, so success here does not mean the fan follows; see
/// `thermal::fan_request_state`.
pub fn fan_rpm_request(s: &dyn Ssam, rpm: u16) -> io::Result<()> {
    if !(rpm == 0 || (FAN_RPM_MIN..=FAN_RPM_MAX).contains(&rpm)) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("rpm must be 0 or {FAN_RPM_MIN}..{FAN_RPM_MAX}")));
    }
    s.request(FAN_RPM_REQUEST, &rpm.to_le_bytes(), 0).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uapi_layout() {
        assert_eq!(std::mem::size_of::<CdevRequest>(), 40);
        assert_eq!(SSAM_CDEV_REQUEST, 0xC028_A501);
    }

    #[test]
    fn smart_charging_reads() {
        let f = FakeSsam::sp9();
        let sc = smart_charging(&f).unwrap();
        assert!(sc.supported && !sc.engaged && !sc.battery_limit);
        f.0.lock().unwrap().responses.insert(BCL_STATUS, vec![0x40, 0x00]);
        assert!(smart_charging(&f).unwrap().engaged);
        f.0.lock().unwrap().responses.insert(BCL_STATUS, vec![0x01, 0x00]);
        // #167 compared == 1; Windows masks 0x440
        assert!(!smart_charging(&f).unwrap().engaged);
        f.0.lock().unwrap().responses.insert(BCL_STATUS, vec![0x00, 0x04]);
        assert!(smart_charging(&f).unwrap().engaged);
        assert_eq!(release_smart_charging(&f).unwrap(), Release::NotEngaged);
    }

    #[test]
    fn unsupported_policy_skips_status() {
        let f = FakeSsam::sp9();
        f.0.lock().unwrap().responses.insert(POLICY, vec![0x00]);
        let sc = smart_charging(&f).unwrap();
        assert!(!sc.supported);
        assert_eq!(f.0.lock().unwrap().sent.len(), 1);
    }

    #[test]
    fn fan_request_bounds() {
        let f = FakeSsam::sp9();
        fan_rpm_request(&f, 3500).unwrap();
        fan_rpm_request(&f, 0).unwrap();
        fan_rpm_request(&f, FAN_RPM_MIN).unwrap();
        fan_rpm_request(&f, FAN_RPM_MAX).unwrap();
        assert!(fan_rpm_request(&f, FAN_RPM_MIN - 1).is_err());
        assert!(fan_rpm_request(&f, FAN_RPM_MAX + 1).is_err());
        let sent = f.0.lock().unwrap().sent.clone();
        // FAN TC 0x05, TID 1, IID 1, CID 0x0b, u16 little-endian: 3500 = 0x0dac
        assert_eq!(sent[0], (Rqst::new(0x05, 1, 1, 0x0b), vec![0xac, 0x0d]));
        assert_eq!(sent[1], (FAN_RPM_REQUEST, vec![0, 0]));
        assert_eq!(sent.len(), 4);
    }
}
