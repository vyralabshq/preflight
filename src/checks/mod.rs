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

/// Last assignment of a systemd directive in the unit and its drop-ins.
/// Drop-ins are applied in name order, matching systemd.
pub fn unit_directive(ctx: &Ctx, key: &str) -> Option<String> {
    let unit = ctx.inv()?.unit_path.as_ref()?;
    let mut texts = Vec::new();
    if let Ok(t) = ctx.fs.read(unit) {
        texts.push(t);
    }
    let mut drops = ctx.fs.list(format!("{unit}.d"));
    drops.sort();
    for p in drops {
        if p.extension().is_some_and(|e| e == "conf")
            && let Ok(t) = std::fs::read_to_string(&p)
        {
            texts.push(t);
        }
    }
    let mut found = None;
    for text in texts {
        for line in text.lines() {
            if let Some(v) = line.trim().strip_prefix(&format!("{key}=")) {
                found = Some(v.trim().trim_matches('"').to_string());
            }
        }
    }
    found.filter(|v| !v.is_empty())
}
