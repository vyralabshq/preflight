//! Every command preflight may run with elevated privileges, and nothing else.
//!
//! This file is what a sceptical operator reads before running an unknown
//! binary near a validator, so it stays short and obvious. Read-only commands
//! only. Nothing is composed from user input; the sole substitution is a
//! `{}` placeholder filled with an interface name validated against
//! /sys/class/net or a pid validated against /proc.
//!
//! The ARG layer needs none of these, so a run against a command line alone
//! prompts for nothing.

pub struct Elevated {
    pub command: &'static str,
    pub used_by: &'static str,
    pub looking_for: &'static str,
}

pub static ALLOWLIST: &[Elevated] = &[Elevated {
    command: "cat /proc/{pid}/status",
    used_by: "PF-XDP-0007",
    looking_for: "CapPrm: capabilities actually held by the running validator, which \
                  distinguishes a grant that survives a restart from one that does not",
}];

/// Validate an interface name against /sys/class/net before interpolation.
/// Rejects . and .. and anything with a slash, since /sys/class/net/.. exists.
#[allow(dead_code)]
pub fn valid_interface(fs: &crate::host::Rootfs, name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains('/')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
        && fs.exists(format!("/sys/class/net/{name}"))
}

/// Validate a pid against /proc before interpolation.
pub fn valid_pid(fs: &crate::host::Rootfs, pid: &str) -> bool {
    !pid.is_empty() && pid.chars().all(|c| c.is_ascii_digit()) && fs.exists(format!("/proc/{pid}"))
}
