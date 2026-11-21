// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
//! What is installed, for status reports: the versions of this project's packages and of
//! linux-surface's kernel and iptsd from the package manager's database, the state of the
//! sp9-dkms kernel modules for the running kernel, and the notice a release check left.
//!
//! Only files are read (the dpkg status file, pacman's local database, DKMS's tree, sysfs).
//! Nothing here runs a package manager, opens the network or changes anything: updates come
//! through apt or pacman, never through the helper.

use crate::sys::Root;

/// Packages reported besides every package whose name starts with "sp9": linux-surface's kernel,
/// its headers (DKMS builds against them), its Secure Boot key package and iptsd.
pub const WATCHED: &[&str] =
    &["linux-image-surface", "linux-headers-surface", "linux-surface", "linux-surface-headers", "linux-surface-secureboot-mok", "iptsd"];

/// Where the optional release check (sp9-update-check.service) writes what it found.
pub const NOTICE: &str = "/var/lib/sp9-update-check/notice";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Manager {
    Dpkg,
    Pacman,
    Nix,
    Unknown,
}

impl Manager {
    pub fn detect(root: &Root) -> Manager {
        if root.exists("/var/lib/dpkg/status") {
            Manager::Dpkg
        } else if root.exists("/var/lib/pacman/local") {
            Manager::Pacman
        } else if root.exists("/etc/NIXOS") || root.exists("/nix/store") {
            Manager::Nix
        } else {
            Manager::Unknown
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Manager::Dpkg => "dpkg",
            Manager::Pacman => "pacman",
            Manager::Nix => "nix",
            Manager::Unknown => "unknown",
        }
    }

    /// The command that updates everything, this project's packages included.
    pub fn update_command(self) -> &'static str {
        match self {
            Manager::Dpkg => "sudo apt update && sudo apt full-upgrade",
            Manager::Pacman => "sudo pacman -Syu",
            Manager::Nix => "nix flake update surface-pro-9-linux (the input name docs/nixos.md uses), in the system flake,, then nixos-rebuild switch",
            Manager::Unknown => "",
        }
    }
}

fn reported(name: &str) -> bool {
    name == "sp9" || name.starts_with("sp9-") || WATCHED.contains(&name)
}

/// Installed packages that are reported, as (name, version), sorted by name.
pub fn installed(root: &Root, m: Manager) -> Vec<(String, String)> {
    let mut v = match m {
        Manager::Dpkg => root.read_opt("/var/lib/dpkg/status").map(|s| dpkg_status(&s)).unwrap_or_default(),
        Manager::Pacman => pacman_local(root),
        Manager::Nix | Manager::Unknown => Vec::new(),
    };
    v.sort();
    v
}

/// The installed, reported packages of a dpkg status file.
pub fn dpkg_status(status: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for stanza in status.split("\n\n") {
        let (mut name, mut ver, mut ok) = ("", "", false);
        for l in stanza.lines() {
            if let Some(v) = l.strip_prefix("Package: ") {
                name = v.trim();
            } else if let Some(v) = l.strip_prefix("Version: ") {
                ver = v.trim();
            } else if let Some(v) = l.strip_prefix("Status: ") {
                ok = v.trim().ends_with(" installed");
            }
        }
        if ok && reported(name) {
            out.push((name.to_string(), ver.to_string()));
        }
    }
    out
}

/// The reported packages of pacman's local database (one directory per package with a `desc`).
fn pacman_local(root: &Root) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for dir in root.list("/var/lib/pacman/local") {
        let Some(desc) = root.read_opt(&format!("/var/lib/pacman/local/{dir}/desc")) else { continue };
        let field = |key: &str| {
            let mut l = desc.lines();
            l.position(|x| x == key)?;
            l.next().map(str::to_string)
        };
        if let (Some(n), Some(v)) = (field("%NAME%"), field("%VERSION%")) {
            if reported(&n) {
                out.push((n, v));
            }
        }
    }
    out
}

/// The DKMS state of one module package for the running kernel.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Dkms {
    /// "<DKMS name>/<version>", e.g. "sp9/0.2.0"
    pub source: String,
    pub kernel: String,
    /// "installed", "built" (not installed), "failed", "no headers", "not built", "not covered"
    /// (the source names kernel releases and this one is not among them: the stock modules run)
    pub build: String,
    /// the make.log of a failed build
    pub log: String,
    /// (module, state) with state one of "loaded", "in-tree loaded", "built in", "not loaded",
    /// "missing"
    pub modules: Vec<(String, String)>,
    /// Secure Boot is on (the EFI variable): DKMS modules load only with an enrolled key
    pub secure_boot: bool,
}

/// The EFI variable SecureBoot: 4 attribute bytes, then 1 when it is on.
pub fn secure_boot(root: &Root) -> bool {
    std::fs::read(root.p("/sys/firmware/efi/efivars/SecureBoot-8be4df61-93ca-11d2-aa0d-00e098032b8c")).is_ok_and(|v| v.last() == Some(&1))
}

/// Whether a DKMS source builds for `kernel`: the release list its `kernels` file holds (the
/// sp9-dkms generator writes one), else the alternatives of BUILD_EXCLUSIVE_KERNEL="^(a|b)$".
/// None when the source does not restrict the kernel or the form is not understood.
pub fn covers(kernels_file: Option<&str>, conf: &str, kernel: &str) -> Option<bool> {
    if let Some(k) = kernels_file {
        let list: Vec<&str> = k.lines().filter(|l| !l.trim_start().starts_with('#')).filter_map(|l| l.split_whitespace().next()).collect();
        if !list.is_empty() {
            return Some(list.contains(&kernel));
        }
    }
    let re = conf.lines().find_map(|l| l.trim().strip_prefix("BUILD_EXCLUSIVE_KERNEL="))?.trim().trim_matches(|c| c == '"' || c == '\'');
    let alts = re.strip_prefix("^(")?.strip_suffix(")$")?;
    if alts.chars().any(|c| "*?[]{}()^$".contains(c)) {
        return None;
    }
    Some(alts.split('|').any(|a| a.replace('\\', "") == kernel))
}

/// Every DKMS tree whose name starts with "sp9" (the sp9-dkms package), newest version first
/// within a name.
pub fn dkms(root: &Root) -> Vec<Dkms> {
    let kernel = root.read_opt("/proc/sys/kernel/osrelease").unwrap_or_default();
    let arch = std::env::consts::ARCH;
    let sb = secure_boot(root);
    let mut out = Vec::new();
    for name in root.list("/var/lib/dkms").into_iter().filter(|n| n.starts_with("sp9")) {
        let base = format!("/var/lib/dkms/{name}");
        let mut versions: Vec<String> = root
            .list(&base)
            .into_iter()
            .filter(|v| !v.starts_with("kernel-") && root.exists(&format!("{base}/{v}/source/dkms.conf")))
            .collect();
        versions.sort_by(|a, b| version_cmp(b, a));
        for ver in versions {
            let vdir = format!("{base}/{ver}");
            let conf = root.read_opt(&format!("{vdir}/source/dkms.conf")).unwrap_or_default();
            let mods = built_module_names(&conf);
            let built = root.exists(&format!("{vdir}/{kernel}/{arch}/module"));
            let file =
                |m: &str| ["", ".xz", ".zst", ".gz"].iter().any(|z| root.exists(&format!("/lib/modules/{kernel}/updates/dkms/{m}.ko{z}")));
            let all_installed = !mods.is_empty() && mods.iter().all(|m| file(m));
            let log =
                [format!("{vdir}/build/make.log"), format!("{vdir}/{kernel}/{arch}/log/make.log")].into_iter().find(|l| root.exists(l));
            let covered = covers(root.read_opt(&format!("{vdir}/source/kernels")).as_deref(), &conf, &kernel);
            let (build, log) = if all_installed {
                ("installed", None)
            } else if built {
                ("built", None)
            } else if covered == Some(false) {
                ("not covered", None)
            } else if !root.exists(&format!("/lib/modules/{kernel}/build")) {
                ("no headers", None)
            } else if let Some(l) = log {
                ("failed", Some(l))
            } else {
                ("not built", None)
            };
            let modules = mods
                .iter()
                .map(|m| {
                    let sm = format!("/sys/module/{}", m.replace('-', "_"));
                    let state = if !root.exists(&sm) {
                        if file(m) {
                            "not loaded"
                        } else {
                            "missing"
                        }
                    } else if !root.exists(&format!("{sm}/initstate")) {
                        "built in"
                    } else if root.read_opt(&format!("{sm}/taint")).is_some_and(|t| t.contains('O')) {
                        // DKMS builds out of tree: the kernel marks such a module with O
                        "loaded"
                    } else {
                        "in-tree loaded"
                    };
                    (m.clone(), state.to_string())
                })
                .collect();
            out.push(Dkms {
                source: format!("{name}/{ver}"),
                kernel: kernel.clone(),
                build: build.into(),
                log: log.unwrap_or_default(),
                modules,
                secure_boot: sb,
            });
        }
    }
    out
}

/// BUILT_MODULE_NAME[n]="..." of a dkms.conf, in the order of n.
pub fn built_module_names(conf: &str) -> Vec<String> {
    let mut v: Vec<(u32, String)> = conf
        .lines()
        .filter_map(|l| {
            let l = l.trim();
            let rest = l.strip_prefix("BUILT_MODULE_NAME[")?;
            let (i, val) = rest.split_once("]=")?;
            let val = val.trim().trim_matches(|c| c == '"' || c == '\'');
            Some((i.trim().parse().ok()?, val.to_string()))
        })
        .filter(|(_, n)| !n.is_empty())
        .collect();
    v.sort();
    v.into_iter().map(|(_, n)| n).collect()
}

/// Version strings compared by their numeric parts ("0.10.0" > "0.9.1").
fn version_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let parts = |s: &str| {
        s.split(|c: char| !c.is_ascii_digit()).filter(|p| !p.is_empty()).map(|p| p.parse::<u64>().unwrap_or(0)).collect::<Vec<_>>()
    };
    parts(a).cmp(&parts(b)).then_with(|| a.cmp(b))
}

/// One line: what to do about the DKMS state, empty when nothing is to do.
pub fn dkms_advice(d: &Dkms) -> String {
    match d.build.as_str() {
        "failed" => format!("the sp9-dkms build for {} failed (log: {}); see docs/install-debian-ubuntu.md or docs/install-arch.md, section 4", d.kernel, d.log),
        "no headers" => {
            format!("no kernel headers for {}: install linux-headers-surface (Debian, Ubuntu) or linux-surface-headers (Arch)", d.kernel)
        }
        "not covered" => {
            format!("sp9-dkms does not cover {} yet: the stock linux-surface modules run until an sp9-dkms update covers it", d.kernel)
        }
        "not built" | "built" => format!("sp9-dkms is not installed for {}: sudo dkms autoinstall", d.kernel),
        "installed" if d.secure_boot && d.modules.iter().any(|(_, s)| s == "not loaded") => {
            "sp9-dkms modules are installed but not loaded: under Secure Boot the DKMS signing key must be enrolled \
             (dkms/README.md, Secure Boot); until then neither they nor the stock modules they replace load"
                .into()
        }
        _ if d.modules.iter().any(|(_, s)| s == "in-tree loaded") => {
            "a module of the running kernel is loaded instead of the sp9-dkms build: reboot".into()
        }
        _ => String::new(),
    }
}

/// The release check's notice ("" when there is none or the check is not enabled).
pub fn notice(root: &Root) -> String {
    root.read_opt(NOTICE).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sys::FakeTree;

    const STATUS: &str = "Package: bash\nStatus: install ok installed\nVersion: 5.2\n\n\
        Package: sp9-helper\nStatus: install ok installed\nArchitecture: amd64\nVersion: 0.2.0-1~deb13\n\n\
        Package: sp9-settings\nStatus: deinstall ok config-files\nVersion: 0.1.0-1~deb13\n\n\
        Package: linux-image-surface\nStatus: install ok installed\nVersion: 7.3.4-2\n";

    #[test]
    fn dpkg() {
        assert_eq!(
            dpkg_status(STATUS),
            vec![("sp9-helper".into(), "0.2.0-1~deb13".into()), ("linux-image-surface".into(), "7.3.4-2".into())]
        );
        let t = FakeTree::new("pkg-dpkg");
        t.file("/var/lib/dpkg/status", STATUS);
        let m = Manager::detect(&t.root);
        assert_eq!(m, Manager::Dpkg);
        assert_eq!(installed(&t.root, m)[0].0, "linux-image-surface");
        assert!(m.update_command().contains("apt"));
    }

    #[test]
    fn pacman() {
        let t = FakeTree::new("pkg-pacman");
        t.file("/var/lib/pacman/local/sp9-helper-0.2.0-1/desc", "%NAME%\nsp9-helper\n\n%VERSION%\n0.2.0-1\n\n%BASE%\nsp9-linux\n");
        t.file("/var/lib/pacman/local/glibc-2.44-1/desc", "%NAME%\nglibc\n\n%VERSION%\n2.44-1\n");
        t.file("/var/lib/pacman/local/ALPM_DB_VERSION", "9");
        let m = Manager::detect(&t.root);
        assert_eq!(m, Manager::Pacman);
        assert_eq!(installed(&t.root, m), vec![("sp9-helper".to_string(), "0.2.0-1".to_string())]);
    }

    fn dkms_tree(t: &FakeTree) {
        t.file("/proc/sys/kernel/osrelease", "7.3.4-surface\n");
        t.file(
            "/var/lib/dkms/sp9/0.2.0/source/dkms.conf",
            "PACKAGE_NAME=\"sp9\"\nBUILT_MODULE_NAME[1]=\"surface_charger\"\nBUILT_MODULE_NAME[0]=\"ov5693\"\n",
        );
        t.file("/var/lib/dkms/other/1.0/source/dkms.conf", "BUILT_MODULE_NAME[0]=\"x\"\n");
        t.file("/lib/modules/7.3.4-surface/build/Makefile", "");
    }

    #[test]
    fn dkms_states() {
        let t = FakeTree::new("pkg-dkms");
        dkms_tree(&t);
        let d = dkms(&t.root);
        assert_eq!(d.len(), 1);
        assert_eq!((d[0].source.as_str(), d[0].build.as_str()), ("sp9/0.2.0", "not built"));
        assert_eq!(d[0].modules, vec![("ov5693".into(), "missing".into()), ("surface_charger".into(), "missing".into())]);

        t.file("/var/lib/dkms/sp9/0.2.0/build/make.log", "error\n");
        let d = dkms(&t.root);
        assert_eq!(d[0].build, "failed");
        assert!(d[0].log.ends_with("/var/lib/dkms/sp9/0.2.0/build/make.log"));
        assert!(dkms_advice(&d[0]).contains("failed"));

        let arch = std::env::consts::ARCH;
        t.file(&format!("/var/lib/dkms/sp9/0.2.0/7.3.4-surface/{arch}/module/ov5693.ko.zst"), "");
        t.file("/lib/modules/7.3.4-surface/updates/dkms/ov5693.ko.zst", "");
        t.file("/lib/modules/7.3.4-surface/updates/dkms/surface_charger.ko.zst", "");
        t.file("/sys/module/ov5693/initstate", "live\n");
        t.file("/sys/module/ov5693/taint", "OE\n");
        t.file("/sys/module/surface_charger/initstate", "live\n");
        t.file("/sys/module/surface_charger/taint", "\n");
        let d = dkms(&t.root);
        assert_eq!(d[0].build, "installed");
        assert_eq!(d[0].modules, vec![("ov5693".into(), "loaded".into()), ("surface_charger".into(), "in-tree loaded".into())]);
        assert!(dkms_advice(&d[0]).contains("reboot"));
    }

    #[test]
    fn no_headers_and_versions() {
        let t = FakeTree::new("pkg-dkms-h");
        t.file("/proc/sys/kernel/osrelease", "7.3.5-surface\n");
        t.file("/var/lib/dkms/sp9/0.9.0/source/dkms.conf", "BUILT_MODULE_NAME[0]=\"a\"\n");
        t.file("/var/lib/dkms/sp9/0.10.0/source/dkms.conf", "BUILT_MODULE_NAME[0]=\"a\"\n");
        let d = dkms(&t.root);
        assert_eq!(d.iter().map(|x| x.source.as_str()).collect::<Vec<_>>(), ["sp9/0.10.0", "sp9/0.9.0"]);
        assert_eq!(d[0].build, "no headers");
    }

    #[test]
    fn coverage() {
        let conf = "BUILD_EXCLUSIVE_KERNEL=\"^(7\\.3\\.4-surface-2|7\\.3\\.5-surface-1)$\"\n";
        assert_eq!(covers(None, conf, "7.3.4-surface-2"), Some(true));
        assert_eq!(covers(None, conf, "7.3.6-surface-1"), Some(false));
        assert_eq!(covers(Some("# x\n7.3.4-arch1-1\tv1\ttag\n"), conf, "7.3.4-arch1-1"), Some(true));
        assert_eq!(covers(None, "BUILD_EXCLUSIVE_KERNEL=\"^7.*\"\n", "7.3"), None);
        assert_eq!(covers(None, "", "7.3"), None);

        let t = FakeTree::new("pkg-dkms-cover");
        dkms_tree(&t);
        t.file("/var/lib/dkms/sp9/0.2.0/source/kernels", "7.3.3-surface-1 a b\n");
        let d = dkms(&t.root);
        assert_eq!(d[0].build, "not covered");
        assert!(dkms_advice(&d[0]).contains("stock"));
    }

    #[test]
    fn secure_boot_advice() {
        let t = FakeTree::new("pkg-dkms-sb");
        dkms_tree(&t);
        t.file("/sys/firmware/efi/efivars/SecureBoot-8be4df61-93ca-11d2-aa0d-00e098032b8c", [6u8, 0, 0, 0, 1]);
        t.file("/lib/modules/7.3.4-surface/updates/dkms/ov5693.ko.zst", "");
        t.file("/lib/modules/7.3.4-surface/updates/dkms/surface_charger.ko.zst", "");
        let d = dkms(&t.root);
        assert!(d[0].secure_boot);
        assert_eq!(d[0].build, "installed");
        assert!(dkms_advice(&d[0]).contains("enrolled"));
    }

    #[test]
    fn module_names() {
        assert_eq!(built_module_names("BUILT_MODULE_NAME[0]=hid-surface\n# x\nBUILT_MODULE_NAME[2]='b'\n"), ["hid-surface", "b"]);
    }
}
