//! KRN layer. Kernel settings a validator needs, whether or not one is installed yet.
//! Each value is checked twice: the running value, and the file
//! that would restore it after a reboot. Correct-but-unsaved is `Ephemeral`.

use crate::{
    ctx::Ctx,
    model::{FixStep, Outcome, Persistence, Source, SourceKind::*},
};

pub const S_LIMITS: &[Source] = &[
    Source {
        kind: AgaveSymbol,
        locator: "INTERESTING_LIMITS",
        verified_against: "v4.2.1",
        provisional: false,
    },
    Source {
        kind: AgaveSymbol,
        locator: "check_os_network_limits()",
        verified_against: "v4.2.1",
        provisional: false,
    },
];
pub const S_NR_OPEN: &[Source] = &[Source {
    kind: AnzaDocs,
    locator: "docs.anza.xyz/operations/setup-a-validator, System Tuning",
    verified_against: "2026-08",
    provisional: false,
}];

const SYSCTL_FILE: &str = "/etc/sysctl.d/21-agave-validator.conf";

const WHY_GATED: &str = "agave calls check_os_network_limits() before it opens the ledger and \
    returns an error if this value is below its recommendation, so the validator refuses to \
    start. It is not a tuning preference. The value preflight adds is catching it before a \
    multi-hour snapshot download rather than after.";

fn sysctl(ctx: &Ctx, key: &str) -> Option<i64> {
    let path = format!("/proc/sys/{}", key.replace('.', "/"));
    ctx.fs
        .read_trim(path)?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

/// Look for a file that would restore the value after a reboot.
fn persisted_in(ctx: &Ctx, key: &str) -> Option<String> {
    let mut files: Vec<std::path::PathBuf> = ctx.fs.list("/etc/sysctl.d");
    files.push(ctx.fs.at("/etc/sysctl.conf"));
    for f in files {
        let Ok(text) = std::fs::read_to_string(&f) else {
            continue;
        };
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('#') {
                continue;
            }
            if line.split('=').next().is_some_and(|k| k.trim() == key) {
                return Some(f.display().to_string());
            }
        }
    }
    None
}

/// `kernel_default` is what this value is on an untouched Linux box. It
/// matters because Ephemeral means "correct now, gone after a reboot", and a
/// kernel default is not going anywhere. Without it an adequate default would
/// be reported as at risk, which is a false alarm and erodes the state's
/// meaning everywhere else.
fn check_value(
    ctx: &Ctx,
    key: &str,
    want: i64,
    kernel_default: i64,
    why: &str,
    source_note: &str,
) -> Outcome {
    let expected = format!("{key} at or above {want}");
    let Some(actual) = sysctl(ctx, key) else {
        return Outcome::unknown(format!("cannot read /proc/sys/{}", key.replace('.', "/")))
            .expected(expected)
            .why(why)
            .fix(vec![FixStep::noted(
                "run preflight on the prospective validator host",
                "kernel settings live in /proc/sys, which only exists on Linux",
            )]);
    };

    let persisted = persisted_in(ctx, key);
    let fix = vec![
        FixStep::cmd(format!("echo '{key} = {want}' | sudo tee -a {SYSCTL_FILE}")),
        FixStep::noted(
            format!("sudo sysctl -p {SYSCTL_FILE}"),
            "applies it now; the file is what makes it survive a reboot",
        ),
    ];

    if actual < want {
        return Outcome::fail(format!("{key} = {actual}"), expected)
            .why(format!("{why} {source_note}"))
            .fix(fix)
            .verify(format!("cat /proc/sys/{}", key.replace('.', "/")))
            .persists(Persistence {
                found: persisted,
                expected: SYSCTL_FILE.to_string(),
            });
    }

    match persisted {
        Some(f) => Outcome::pass(format!("{key} = {actual}, set by {f}"), expected)
            .why(why)
            .persists(Persistence {
                found: Some(f),
                expected: SYSCTL_FILE.to_string(),
            }),
        None if actual == kernel_default => Outcome::pass(
            format!("{key} = {actual}, which is the kernel default and already adequate"),
            expected,
        )
        .why(why),
        None => Outcome::ephemeral(
            format!("{key} = {actual}, but no file under /etc/sysctl.d sets it"),
            expected,
        )
        .why(format!(
            "{why} The running value is correct, so nothing is wrong today. Nothing on disk \
             restores it, so the next reboot returns it to the kernel default and the validator \
             stops starting for a reason that looks unrelated to the reboot."
        ))
        .fix(fix)
        .verify(format!("grep -r {key} /etc/sysctl.conf /etc/sysctl.d/"))
        .persists(Persistence {
            found: None,
            expected: SYSCTL_FILE.to_string(),
        }),
    }
}

pub fn rmem_max(ctx: &Ctx) -> Outcome {
    check_value(
        ctx,
        "net.core.rmem_max",
        134_217_728,
        212_992,
        WHY_GATED,
        "This is the receive buffer for the UDP paths the validator ingests on.",
    )
}

pub fn wmem_max(ctx: &Ctx) -> Outcome {
    check_value(
        ctx,
        "net.core.wmem_max",
        134_217_728,
        212_992,
        WHY_GATED,
        "This is the send buffer counterpart.",
    )
}

pub fn max_map_count(ctx: &Ctx) -> Outcome {
    check_value(
        ctx,
        "vm.max_map_count",
        1_000_000,
        65_530,
        WHY_GATED,
        "The accounts database uses a large number of memory mappings; the kernel default is far below what it needs.",
    )
}

pub fn nr_open(ctx: &Ctx) -> Outcome {
    check_value(
        ctx,
        "fs.nr_open",
        1_000_000,
        1_048_576,
        "agave does not check this one, but fs.nr_open is the ceiling for any process's open-file \
         limit. Setting LimitNOFILE=1000000 on a host whose fs.nr_open is lower is silently \
         clamped, and the failure then appears to be about file descriptors rather than about \
         this.",
        "",
    )
}
