//! The check layers, and the one helper they share.
//!
//! Each layer is a module of functions that take the context and return an
//! outcome. They never write, never prompt, and never run anything.

use crate::{ctx::Ctx, model::Outcome};

pub mod arg;
pub mod fs;
pub mod hw;
pub mod kernel;
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
pub mod net;
