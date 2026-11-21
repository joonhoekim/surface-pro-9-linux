// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
//! UI text. The source strings are English (gettext-style message IDs, the
//! same wording style as tb323fu-settings) and every one goes through `tr`,
//! `trf` or, in constant tables, `n_`. Catalogs are tables per language
//! (`i18n/ko.rs`); the Korean one is shown unless the locale asks for English.
//! A test checks that every message ID in the sources has a Korean entry, so a
//! .po export or another language is a new table, not a code change.

use std::collections::HashMap;
use std::fmt::Display;
use std::sync::OnceLock;

mod ko;

/// Marks a message ID in a constant table; translated where it is shown.
pub const fn n_(id: &'static str) -> &'static str {
    id
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Lang {
    En,
    Ko,
}

/// SP9_SETTINGS_LANG, then the usual locale variables in gettext's order. The
/// app is Korean first: only an English locale (en*) shows the message IDs;
/// C and POSIX (minimal systems, services) stay Korean.
pub fn lang_from(get: impl Fn(&str) -> Option<String>) -> Lang {
    let v = ["SP9_SETTINGS_LANG", "LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|k| get(k))
        .find(|v| !v.is_empty())
        .unwrap_or_default();
    let first = v.split(':').next().unwrap_or("").to_ascii_lowercase();
    if first.starts_with("en") {
        Lang::En
    } else {
        Lang::Ko
    }
}

fn catalog() -> Option<&'static HashMap<&'static str, &'static str>> {
    static CAT: OnceLock<Option<HashMap<&'static str, &'static str>>> = OnceLock::new();
    CAT.get_or_init(|| match lang_from(|k| std::env::var(k).ok()) {
        Lang::En => None,
        Lang::Ko => Some(ko::KO.iter().copied().collect()),
    })
    .as_ref()
}

pub fn tr(id: &'static str) -> &'static str {
    catalog().and_then(|c| c.get(id).copied()).unwrap_or(id)
}

/// `tr` with `{}` placeholders filled in order (a translation may not reorder them).
pub fn trf(id: &'static str, args: &[&dyn Display]) -> String {
    fill(tr(id), args)
}

fn fill(t: &str, args: &[&dyn Display]) -> String {
    let mut out = String::with_capacity(t.len() + 16);
    let mut it = args.iter();
    let mut rest = t;
    while let Some(i) = rest.find("{}") {
        out.push_str(&rest[..i]);
        if let Some(a) = it.next() {
            out.push_str(&a.to_string());
        }
        rest = &rest[i + 2..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Message IDs in tr("…"), trf("…", …) and n_("…") in the sources.
    fn ids_in(src: &str) -> Vec<String> {
        let mut out = Vec::new();
        for pat in ["tr(\"", "trf(\"", "n_(\""] {
            let mut rest = src;
            while let Some(i) = rest.find(pat) {
                let before = rest[..i].chars().last();
                rest = &rest[i + pat.len()..];
                if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                    continue;
                }
                let end = rest.find('"').expect("unterminated message ID");
                // the one escape message IDs use
                out.push(rest[..end].replace("\\n", "\n"));
                rest = &rest[end..];
            }
        }
        out
    }

    #[test]
    fn every_message_has_korean() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let ko: HashMap<&str, &str> = ko::KO.iter().copied().collect();
        let mut missing = Vec::new();
        let mut seen = 0;
        for f in ["main.rs", "dbus.rs"] {
            let src = std::fs::read_to_string(dir.join(f)).unwrap();
            for id in ids_in(&src) {
                seen += 1;
                if !ko.contains_key(id.as_str()) && !missing.contains(&id) {
                    missing.push(id);
                }
            }
        }
        assert!(seen > 100, "scanner found only {seen} message IDs");
        assert!(missing.is_empty(), "no Korean for: {missing:#?}");
    }

    #[test]
    fn catalog_is_consistent() {
        let mut keys = std::collections::HashSet::new();
        for (id, t) in ko::KO {
            assert!(keys.insert(id), "duplicate message ID {id:?}");
            assert_eq!(id.matches("{}").count(), t.matches("{}").count(), "placeholders differ in {id:?}");
            assert!(!t.is_empty());
        }
    }

    #[test]
    fn language_choice() {
        let env = |pairs: &'static [(&'static str, &'static str)]| move |k: &str| pairs.iter().find(|p| p.0 == k).map(|p| p.1.to_string());
        assert_eq!(lang_from(env(&[])), Lang::Ko);
        assert_eq!(lang_from(env(&[("LANG", "ko_KR.UTF-8")])), Lang::Ko);
        assert_eq!(lang_from(env(&[("LANG", "en_US.UTF-8")])), Lang::En);
        assert_eq!(lang_from(env(&[("LANGUAGE", "en:ko"), ("LANG", "ko_KR.UTF-8")])), Lang::En);
        assert_eq!(lang_from(env(&[("SP9_SETTINGS_LANG", "ko"), ("LANG", "en_US.UTF-8")])), Lang::Ko);
        assert_eq!(fill("{} of {}", &[&1, &"x"]), "1 of x");
    }
}
