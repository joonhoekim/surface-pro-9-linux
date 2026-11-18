// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
//! Clocks of the device threads: seconds on the monotonic clock, and the time the machine
//! has spent suspended (CLOCK_BOOTTIME counts it, CLOCK_MONOTONIC does not), which grows at
//! every resume.

fn clock(id: libc::clockid_t) -> f64 {
    let mut t = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: t is a valid timespec.
    unsafe { libc::clock_gettime(id, &mut t) };
    t.tv_sec as f64 + t.tv_nsec as f64 * 1e-9
}

pub fn monotonic() -> f64 {
    clock(libc::CLOCK_MONOTONIC)
}

pub fn suspended() -> f64 {
    clock(libc::CLOCK_BOOTTIME) - clock(libc::CLOCK_MONOTONIC)
}

/// True once per resume: the suspended time grew by more than a second since `mark`.
pub fn resumed(mark: &mut f64) -> bool {
    let s = suspended();
    let r = s - *mark > 1.0;
    if r {
        *mark = s;
    }
    r
}
