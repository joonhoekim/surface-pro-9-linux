// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
//! Filesystem access under a root prefix. On the device the root is "/"; tests and
//! `SP9_SYSFS_ROOT` point every absolute device path (sysfs, debugfs, efivarfs, /dev, /etc
//! files the helper writes) at a fake tree.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Root(PathBuf);

impl Root {
    pub fn new(p: impl Into<PathBuf>) -> Self {
        Root(p.into())
    }

    /// `SP9_SYSFS_ROOT` when set, otherwise "/".
    pub fn from_env() -> Self {
        match std::env::var_os("SP9_SYSFS_ROOT") {
            Some(r) if !r.is_empty() => Root(PathBuf::from(r)),
            _ => Root(PathBuf::from("/")),
        }
    }

    /// An absolute device path resolved under the root.
    pub fn p(&self, abs: &str) -> PathBuf {
        self.0.join(abs.trim_start_matches('/'))
    }

    pub fn read(&self, abs: &str) -> io::Result<String> {
        read(&self.p(abs))
    }

    pub fn read_opt(&self, abs: &str) -> Option<String> {
        self.read(abs).ok()
    }

    pub fn read_i64(&self, abs: &str) -> Option<i64> {
        self.read(abs).ok()?.parse().ok()
    }

    pub fn write(&self, abs: &str, v: &str) -> io::Result<()> {
        fs::write(self.p(abs), v)
    }

    pub fn exists(&self, abs: &str) -> bool {
        self.p(abs).exists()
    }

    /// File names in a directory, sorted; empty when missing.
    pub fn list(&self, abs: &str) -> Vec<String> {
        list_dir(&self.p(abs))
    }
}

/// Read a sysfs attribute, trimmed.
pub fn read(p: &Path) -> io::Result<String> {
    Ok(fs::read_to_string(p)?.trim().to_string())
}

pub fn list_dir(p: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(p)
        .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    v.sort();
    v
}

/// The word in brackets of a sysfs choice attribute ("a [b] c" -> "b").
pub fn selected(choice: &str) -> Option<String> {
    let s = choice.find('[')?;
    let e = choice[s..].find(']')? + s;
    Some(choice[s + 1..e].to_string())
}

/// Write `content` to `path` through a temporary file and a rename, creating parents.
pub fn write_atomic(path: &Path, content: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    {
        use std::io::Write;
        let mut f = fs::File::create(&tmp)?;
        f.write_all(content)?;
        // the rename must not reach the disk before the content does
        f.sync_all()?;
    }
    fs::rename(&tmp, path)
}

/// A fake device tree in the temporary directory, removed on drop. Public so the daemon's
/// tests can use it too.
pub struct FakeTree {
    pub root: Root,
    dir: PathBuf,
}

impl FakeTree {
    pub fn new(name: &str) -> Self {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let dir =
            std::env::temp_dir().join(format!("sp9-helper-test-{}-{}-{}", name, std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create fake tree");
        FakeTree { root: Root::new(dir.clone()), dir }
    }

    /// Create a file (and its parents) with `content`.
    pub fn file(&self, abs: &str, content: impl AsRef<[u8]>) -> &Self {
        let p = self.root.p(abs);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, content).unwrap();
        self
    }

    pub fn read(&self, abs: &str) -> String {
        self.root.read(abs).unwrap_or_default()
    }
}

impl Drop for FakeTree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_word() {
        assert_eq!(selected("low-power [balanced] performance").as_deref(), Some("balanced"));
        assert_eq!(selected("none"), None);
    }

    #[test]
    fn root_prefix() {
        let t = FakeTree::new("sys");
        t.file("/sys/x/y", "42\n");
        assert_eq!(t.root.read_i64("/sys/x/y"), Some(42));
        assert_eq!(t.root.list("/sys/x"), vec!["y".to_string()]);
        write_atomic(&t.root.p("/etc/sp9/a.conf"), b"x").unwrap();
        assert_eq!(t.read("/etc/sp9/a.conf"), "x");
    }
}
