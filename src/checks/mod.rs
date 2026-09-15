//! The check layers, and the helpers they share.
//!
//! Each layer is a module of functions that take the context and return an
//! outcome. They never write, never prompt, and never run anything.

use crate::{ctx::Ctx, model::Outcome};

pub mod arg;
pub mod fs;
pub mod hw;
pub mod kernel;
pub mod net;
pub mod service;
pub mod xdp;

/// Host layers read /proc and /sys.
///
/// A captured tree missing /proc is an incomplete capture, so Unknown. A real
/// machine that is not Linux is Unsupported: no command turns macOS into a
/// validator host, and saying "cannot say" would be hedging on a known answer.
pub fn needs_linux(ctx: &Ctx, why: &str) -> Option<Outcome> {
    if ctx.is_linux() {
        return None;
    }
    Some(match ctx.fs.is_prefixed() {
        true => Outcome::unknown("the captured tree has no /proc")
            .expected("a capture taken from a Linux host")
            .why(why),
        false => Outcome::unsupported(
            format!("this machine runs {}, not Linux", std::env::consts::OS),
            "a Linux host",
        )
        .why(why),
    })
}

/// The unit, then every drop-in systemd merges over it. Drop-ins live in any
/// load path directory, not just beside the unit: systemctl edit writes to
/// /etc even when the unit is in /usr/lib.
fn unit_texts(ctx: &Ctx) -> Vec<String> {
    let Some(inv) = ctx.inv() else {
        return Vec::new();
    };
    let mut texts = Vec::new();
    if let Some(path) = inv.unit_path.as_ref()
        && let Ok(t) = ctx.fs.read(path)
    {
        texts.push(t);
    }
    let Some(name) = inv.unit_name.as_ref() else {
        return texts;
    };
    // Lowest precedence first, so later files override earlier ones.
    for dir in crate::argv::UNIT_DIRS.iter().rev() {
        let mut drops = ctx.fs.list(format!("{dir}/{name}.d"));
        drops.sort();
        for p in drops {
            if p.extension().is_some_and(|e| e == "conf")
                && let Ok(t) = std::fs::read_to_string(&p)
            {
                texts.push(t);
            }
        }
    }
    texts
}

/// Last assignment wins, for a directive systemd replaces.
pub fn unit_directive(ctx: &Ctx, key: &str) -> Option<String> {
    let mut found = None;
    for text in unit_texts(ctx) {
        for line in text.lines() {
            if let Some(v) = line.trim().strip_prefix(&format!("{key}=")) {
                found = Some(v.trim().trim_matches('"').to_string());
            }
        }
    }
    found.filter(|v| !v.is_empty())
}

/// Every assignment, for a directive systemd appends rather than replaces.
pub fn unit_directive_all(ctx: &Ctx, key: &str) -> Vec<String> {
    let mut out = Vec::new();
    for text in unit_texts(ctx) {
        for line in text.lines() {
            if let Some(v) = line.trim().strip_prefix(&format!("{key}=")) {
                out.push(v.trim().trim_matches('"').to_string());
            }
        }
    }
    out
}
