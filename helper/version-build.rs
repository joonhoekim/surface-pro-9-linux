// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
//! Build script shared by the binary crates: sets `SP9_VERSION` for `--version`.
//!
//! The base is the workspace version (`[workspace.package] version`, the project version; a release
//! is tagged `vX.Y.Z`). A build at exactly that tag, or one without git information, reports the
//! base alone. Any other build appends `git describe` in parentheses, e.g.
//! `0.1.0 (v0.1.0-4-g1a2b3c4-dirty)`, or `0.1.0 (1a2b3c4)` before the first tag.
//! `SP9_GIT_DESCRIBE` in the environment replaces the call to git (for builds from a tarball or a
//! store path, where there is no repository).

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").arg("-C").arg(dir).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (!s.is_empty()).then_some(s)
}

fn describe() -> Option<String> {
    println!("cargo:rerun-if-env-changed=SP9_GIT_DESCRIBE");
    if let Ok(d) = env::var("SP9_GIT_DESCRIBE") {
        return (!d.trim().is_empty()).then(|| d.trim().to_string());
    }
    // helper/crates/<crate> -> the repository root
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").ok()?);
    let root = manifest.parent()?.parent()?.parent()?.canonicalize().ok()?;
    let top = PathBuf::from(git(&root, &["rev-parse", "--show-toplevel"])?).canonicalize().ok()?;
    // Sources unpacked inside some other repository (a packaging tree) describe that one: skip.
    if top != root {
        return None;
    }
    // Rebuild when the commit, the tags or the index change (a path that does not exist would make
    // cargo run this script on every build).
    let mut watch = vec!["HEAD".to_string(), "index".into(), "packed-refs".into(), "refs/tags".into()];
    watch.extend(git(&root, &["symbolic-ref", "-q", "HEAD"]));
    for p in &watch {
        if let Some(path) = git(&root, &["rev-parse", "--path-format=absolute", "--git-path", p]) {
            if Path::new(&path).exists() {
                println!("cargo:rerun-if-changed={path}");
            }
        }
    }
    git(&root, &["describe", "--tags", "--match", "v[0-9]*", "--dirty", "--always", "--abbrev=7"])
}

fn main() {
    let base = env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let full = match describe() {
        Some(d) if d == format!("v{base}") => base,
        Some(d) => format!("{base} ({d})"),
        None => base,
    };
    println!("cargo:rustc-env=SP9_VERSION={full}");
}
