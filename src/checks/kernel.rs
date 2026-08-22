//! KRN layer. Kernel settings a validator needs, whether or not one is installed yet.
//! Each value is checked twice: the running value, and the file
//! that would restore it after a reboot. Correct-but-unsaved is `Ephemeral`.

use crate::{
    ctx::Ctx,
    model::{ClientKind, FixStep, Outcome, Persistence, Source, SourceKind::*},
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
pub const S_XDP_KERNEL: &[Source] = &[Source {
    kind: AnzaBlog,
    locator: "anza.xyz/blog/agave-xdp-setup-guide",
    verified_against: "2026-08",
    provisional: false,
}];
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

/// Anza's floor for the XDP transmit path, by driver.
const KERNEL_FLOOR: (u32, u32) = (6, 8);
const KERNEL_FLOOR_IGB: (u32, u32) = (6, 14);

fn kernel_version(ctx: &Ctx) -> Option<(u32, u32)> {
    let raw = ctx.fs.read_trim("/proc/sys/kernel/osrelease")?;
    let mut parts = raw.split(['.', '-']);
    Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
}

/// PF-XDP-0002. Whether the kernel is new enough for the transmit path agave
/// now uses by default.
///
/// The floor depends on the driver, so this reads the card before deciding.
pub fn xdp_floor(ctx: &Ctx) -> Outcome {
    const WHY: &str = "XDP transmit is on by default on Linux since v4.2, and Anza's setup guide \
        gives a kernel floor for it: 6.14 when the driver is igb, 6.8 otherwise. Below that the \
        path is either unavailable or unreliable, and the node quietly falls back or misbehaves \
        rather than refusing to start, so nothing tells you.";

    if ctx.client == ClientKind::Firedancer {
        return Outcome::skipped("Firedancer manages its own AF_XDP setup");
    }
    let Some(inv) = ctx.inv() else {
        return Outcome::skipped("no validator yet, so no XDP path to judge the kernel against");
    };
    let disabled = inv.has("--no-xdp");
    let explicit = ["--xdp-interface", "--xdp-cpu-cores", "--xdp-zero-copy"]
        .iter()
        .any(|f| inv.has(f));
    let default_on = ctx.at_least(4, 2);
    if disabled || !(explicit || default_on) {
        return Outcome::skipped("XDP not in use, so no kernel floor applies");
    }
    let Some((major, minor)) = kernel_version(ctx) else {
        return Outcome::unknown("cannot read the kernel version").why(WHY);
    };

    let driver = crate::checks::net::primary_interface(ctx)
        .and_then(|iface| crate::checks::net::driver_of(ctx, &iface));
    let (floor, because) = match driver.as_deref() {
        Some("igb") => (KERNEL_FLOOR_IGB, " because the driver is igb"),
        _ => (KERNEL_FLOOR, ""),
    };
    let expected = format!("kernel {}.{} or newer{because}", floor.0, floor.1);
    let observed = format!("kernel {major}.{minor}");

    if (major, minor) >= floor {
        return Outcome::pass(observed, expected).why(WHY);
    }

    // Below the floor with XDP merely available is a shortfall. Below the floor
    // with XDP on by default and no --no-xdp means the default path is live on
    // a kernel that cannot carry it, with nothing to fall back to.
    let unguarded = default_on && !explicit;
    let why = match unguarded {
        true => format!(
            "{WHY} On this box XDP is on by default, the kernel is below the floor, and the \
             invocation does not pass --no-xdp, so that path is live with no fallback."
        ),
        false => WHY.to_string(),
    };
    let fix = match unguarded {
        true => vec![
            FixStep::noted(
                "--no-xdp",
                "the immediate action: it puts the node back on UDP sockets today",
            ),
            FixStep::noted(
                format!(
                    "then upgrade the kernel to {}.{} or newer",
                    floor.0, floor.1
                ),
                "the real fix, and on an older distribution that may mean a release upgrade",
            ),
        ],
        false => vec![FixStep::cmd(format!(
            "upgrade the kernel to {}.{} or newer",
            floor.0, floor.1
        ))],
    };
    Outcome::fail(observed, expected).why(why).fix(fix)
}
