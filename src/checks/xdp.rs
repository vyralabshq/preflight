//! XDP layer. Gated on the detected binary, not the profile:
//! solana-test-validator has no XDP transmit path at all, while a non-voting
//! agave-validator on Linux hits the v4.2 capability gate like any other node.

use crate::{
    checks::unit_directive,
    ctx::Ctx,
    model::{ClientKind, FixStep, Outcome, Persistence, Source, SourceKind::*},
};

pub const S_CAPS: &[Source] = &[
    Source {
        kind: AgaveChangelog,
        locator: "v4.0 Validator/Breaking (#9133)",
        verified_against: "v4.2.1",
        provisional: false,
    },
    Source {
        kind: AgaveChangelog,
        locator: "v4.2 Validator/Breaking",
        verified_against: "v4.2.1",
        provisional: false,
    },
];
pub const S_CAP_PERSIST: &[Source] = &[Source {
    kind: AgaveChangelog,
    locator: "v4.0 Validator/Breaking (#9133)",
    verified_against: "v4.2.1",
    provisional: false,
}];

const CAP_XDP: &[&str] = &["CAP_NET_ADMIN", "CAP_NET_RAW"];
const CAP_ZERO_COPY: &[&str] = &["CAP_BPF", "CAP_PERFMON"];

const WHY_CAPS: &str = "CapabilityBoundingSet caps what a process may ever hold; it grants \
    nothing. On a unit with User= set to a non-root account the bounding set alone leaves the \
    permitted set empty. Agave's changelog pairs it with AmbientCapabilities, which is the \
    directive that grants; the Anza XDP blog post shows only the bounding set, so unit files \
    written from that post end up this way. Agave refuses to start over this only when the \
    invocation asks for XDP explicitly. On the default path it starts, reports XDP as configured, \
    and runs without it: attaching an XDP program needs CAP_NET_ADMIN and an AF_XDP socket needs \
    CAP_NET_RAW, so with an empty permitted set the kernel allows neither and nothing says so.";

struct XdpState {
    enabled: bool,
    zero_copy: bool,
    required: Vec<&'static str>,
}

fn xdp_state(ctx: &Ctx) -> Option<XdpState> {
    let inv = ctx.inv()?;
    let explicit = [
        "--xdp-interface",
        "--xdp-cpu-cores",
        "--xdp-zero-copy",
        "--experimental-retransmit-xdp-cpu-cores",
        "--experimental-retransmit-xdp-interface",
    ]
    .iter()
    .any(|f| inv.has(f));
    let default_on = ctx.at_least(4, 2);
    let disabled = inv.has("--no-xdp");
    let zero_copy =
        inv.has("--xdp-zero-copy") || inv.has("--experimental-retransmit-xdp-zero-copy");
    let mut required = CAP_XDP.to_vec();
    if zero_copy {
        required.extend_from_slice(CAP_ZERO_COPY);
    }
    Some(XdpState {
        enabled: !disabled && (explicit || default_on),
        zero_copy,
        required,
    })
}

fn client_gate(ctx: &Ctx) -> Option<Outcome> {
    match ctx.client {
        ClientKind::TestValidator => Some(Outcome::skipped(
            "solana-test-validator has no XDP transmit path",
        )),
        ClientKind::Firedancer => Some(Outcome::skipped("Firedancer manages its own AF_XDP setup")),
        _ => None,
    }
}

/// Linux capability bits we care about. CapPrm is a hex mask of these.
const CAP_BITS: &[(&str, u32)] = &[
    ("CAP_NET_ADMIN", 12),
    ("CAP_NET_RAW", 13),
    ("CAP_PERFMON", 38),
    ("CAP_BPF", 39),
];

fn cap_missing(mask: u64, required: &[&str]) -> Vec<String> {
    required
        .iter()
        .copied()
        .filter(|name| {
            CAP_BITS
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, b)| mask & (1u64 << b) == 0)
                .unwrap_or(true)
        })
        .map(|s| s.to_string())
        .collect()
}

fn cap_held(mask: u64) -> Vec<&'static str> {
    CAP_BITS
        .iter()
        .filter(|(_, b)| mask & (1u64 << b) != 0)
        .map(|(n, _)| *n)
        .collect()
}

/// PF-XDP-0001. Invocation-aware, and that is the whole check. `--xdp-*` needs
/// CAP_NET_ADMIN and CAP_NET_RAW; `--xdp-zero-copy` additionally needs CAP_BPF
/// and CAP_PERFMON. Demanding all four unconditionally would produce a false
/// FAIL on a correctly configured non-zero-copy node, and a confident wrong
/// answer about capabilities is worse than no check.
pub fn capabilities(ctx: &Ctx) -> Outcome {
    if let Some(o) = client_gate(ctx) {
        return o;
    }
    if !ctx.validator_present {
        return Outcome::skipped("no validator on this host");
    }
    let Some(inv) = ctx.inv() else {
        return Outcome::unknown(format!(
            "could not resolve a validator invocation ({})",
            ctx.invocation_trail.join(" -> ")
        ))
        .why(WHY_CAPS);
    };
    let Some(state) = xdp_state(ctx) else {
        return Outcome::unknown("could not determine XDP state").why(WHY_CAPS);
    };

    if inv.has("--no-xdp") {
        return Outcome::pass(
            "--no-xdp passed, XDP transmit disabled",
            "capabilities granted, or XDP disabled",
        )
        .why(WHY_CAPS);
    }
    if !state.enabled {
        return Outcome::skipped("XDP not enabled in the invocation and not default before v4.2");
    }

    let expected = format!(
        "{} in the permitted set{}",
        state.required.join(" and "),
        if state.zero_copy {
            " (--xdp-zero-copy is in use)"
        } else {
            ""
        }
    );

    let ambient = unit_directive(ctx, "AmbientCapabilities");
    let bounding = unit_directive(ctx, "CapabilityBoundingSet");
    let user = unit_directive(ctx, "User");

    // A live process is correctness. The unit grant is persistence (PF-XDP-0007).
    // Ambient in the unit with a process that was never restarted is a false PASS.
    if let Some(mask) = runtime_capprm(ctx) {
        let missing = cap_missing(mask, &state.required);
        if missing.is_empty() {
            return Outcome::pass(
                format!("CapPrm holds {}", cap_held(mask).join(" ")),
                expected,
            )
            .why(WHY_CAPS)
            .verify("grep CapPrm /proc/$(pgrep -f agave-validator)/status");
        }
        // The process is running, so this is never "it will not start". Whether
        // a restart is the answer depends on which came first: a process older
        // than the grant picks it up on restart, a newer one already did not.
        let stale = ambient.is_some() && grant_postdates_launch(ctx) == Some(true);
        let observed = match stale {
            true => format!(
                "the running validator holds no capabilities (CapPrm {mask:016x}), though its \
                 unit grants {}. It has not been restarted since that was added",
                state.required.join(" ")
            ),
            false if ambient.is_some() => format!(
                "the running validator holds no capabilities (CapPrm {mask:016x}), though its \
                 unit grants {}. It was started after that grant was in place, so the grant is \
                 not reaching the process",
                state.required.join(" ")
            ),
            false => format!(
                "the running validator holds no capabilities (CapPrm {mask:016x}), missing {}",
                missing.join(" ")
            ),
        };
        return Outcome::fail(observed, expected)
            .why(WHY_CAPS)
            .fix(match (stale, ambient.is_some()) {
                (true, _) => vec![FixStep::noted(
                    format!("sudo systemctl restart {}", unit_or(inv)),
                    "the unit is already correct; only the running process is not",
                )],
                // Restarting has already been tried by definition, so sending
                // them to do it again would cost an outage for nothing. File
                // capabilities on the binary clear the ambient set at execve,
                // which is the one thing that explains a bounding set that
                // applied beside an ambient grant that did not.
                (false, true) => vec![
                    FixStep::noted(
                        format!("getcap {}", inv.program),
                        "if this prints anything, the kernel wipes the ambient set when systemd \
                         execs the binary, and no restart will help until it is cleared",
                    ),
                    FixStep::noted(
                        format!("sudo setcap -r {}", inv.program),
                        "only if the line above printed something. Takes effect at the next \
                         start, so fold it into a restart you were doing anyway",
                    ),
                ],
                (false, false) => capability_fix(ctx, &state.required, bounding.as_deref()),
            })
            .verify("ip link show | grep -i xdp")
            .persists(Persistence::unit_dropin(ambient, &unit_or(inv)));
    }

    if inv.unit_path.is_none() {
        return Outcome::unknown(
            "no running validator whose CapPrm could be read, and invocation did not come from a systemd unit",
        )
        .why(WHY_CAPS)
        .verify("grep CapPrm /proc/$(pgrep -f agave-validator)/status");
    }

    let granted: Vec<String> = ambient
        .as_deref()
        .map(|v| v.split_whitespace().map(|s| s.to_uppercase()).collect())
        .unwrap_or_default();
    let missing: Vec<&str> = state
        .required
        .iter()
        .copied()
        .filter(|c| !granted.contains(&c.to_string()))
        .collect();

    if missing.is_empty() {
        return Outcome::pass(
            format!("AmbientCapabilities={}", granted.join(" ")),
            expected,
        )
        .why(WHY_CAPS)
        .verify("grep CapPrm /proc/$(pgrep -f agave-validator)/status");
    }

    let observed = match (&bounding, &ambient, &user) {
        (Some(b), None, Some(u)) => {
            format!("unit sets CapabilityBoundingSet={b} with no AmbientCapabilities, and User={u}")
        }
        (Some(b), None, None) => {
            format!("unit sets CapabilityBoundingSet={b} with no AmbientCapabilities")
        }
        (None, None, _) => "no AmbientCapabilities and no CapabilityBoundingSet in the unit".into(),
        (_, Some(a), _) => format!("AmbientCapabilities={a}, missing {}", missing.join(" ")),
    };

    Outcome::fail(observed, expected)
        .why(WHY_CAPS)
        .fix(capability_fix(ctx, &state.required, bounding.as_deref()))
        .verify("grep CapPrm /proc/$(pgrep -f agave-validator)/status")
        .persists(Persistence::unit_dropin(ambient, &unit_or(inv)))
}

fn unit_or(inv: &crate::argv::Invocation) -> String {
    inv.unit_name
        .clone()
        .unwrap_or_else(|| "<your-validator-unit>".into())
}

/// A drop-in CapabilityBoundingSet= replaces the unit's value rather than
/// adding to it, so only write it when the unit does not already permit what
/// is needed.
fn capability_fix(ctx: &Ctx, required: &[&str], bounding: Option<&str>) -> Vec<FixStep> {
    let unit = ctx
        .inv()
        .map(unit_or)
        .unwrap_or_else(|| "<your-validator-unit>".into());
    let caps = required.join(" ");
    let bounding_covers = bounding.is_some_and(|b| {
        let have: Vec<String> = b.split_whitespace().map(|s| s.to_uppercase()).collect();
        required.iter().all(|c| have.contains(&c.to_string()))
    });
    let (body, note) = match bounding_covers {
        true => (
            format!("AmbientCapabilities={caps}"),
            "Your CapabilityBoundingSet already permits these, so it is left alone. Ambient is \
             the directive that grants them, and it is the one the blog post omits.",
        ),
        false => (
            format!("AmbientCapabilities={caps}\\nCapabilityBoundingSet={caps}"),
            "Ambient grants, BoundingSet restricts. You need both, and Ambient is the one the \
             blog post omits.",
        ),
    };
    vec![
        FixStep::cmd(format!("sudo mkdir -p /etc/systemd/system/{unit}.d")),
        FixStep::noted(
            format!(
                "printf '[Service]\\n{body}\\n' | sudo tee /etc/systemd/system/{unit}.d/20-xdp-caps.conf"
            ),
            note,
        ),
        FixStep::cmd("sudo systemctl daemon-reload"),
        FixStep::cmd(format!("sudo systemctl restart {unit}")),
    ]
}

/// PF-XDP-0007. `Ephemeral` applied to capabilities. setcap writes to the
/// binary's extended attributes and agave-install replaces the binary; agave's
/// changelog says the step must be repeated every time the binary is replaced.
///
/// The distinction only exists when capabilities are actually held right now.
/// Absent both a unit grant and a running process, preflight cannot tell an
/// ephemeral grant from no grant at all, and says so instead of guessing.
pub fn capability_persistence(ctx: &Ctx) -> Outcome {
    if let Some(o) = client_gate(ctx) {
        return o;
    }
    if !ctx.validator_present {
        return Outcome::skipped("no validator on this host");
    }
    let Some(state) = xdp_state(ctx) else {
        return Outcome::unknown("could not resolve a validator invocation");
    };
    if !state.enabled {
        return Outcome::skipped("XDP not enabled, so no capability grant to persist");
    }

    let unit = ctx
        .inv()
        .and_then(|i| i.unit_name.clone())
        .unwrap_or_else(|| "<your-validator-unit>".into());
    let ambient = unit_directive(ctx, "AmbientCapabilities");
    if ambient.is_some() {
        return Outcome::pass(
            "AmbientCapabilities set in the systemd unit",
            "capabilities granted by the unit, not by setcap on the binary",
        )
        .why("A unit drop-in survives binary replacement. A setcap grant does not.")
        .persists(Persistence::unit_dropin(ambient, &unit));
    }

    match runtime_capprm(ctx) {
        None => Outcome::unknown(
            "no AmbientCapabilities in the unit, and no running validator whose permitted set could be read",
        )
        .why(
            "Whether a grant is ephemeral depends on capabilities the process actually holds. With \
             no unit directive and no running process there is nothing to compare, so preflight \
             reports that rather than assuming setcap.",
        )
        .verify("getcap $(which agave-validator)"),
        Some(0) => Outcome::fail("no AmbientCapabilities in the unit and the running validator holds no permitted capabilities",
            "capabilities granted by an AmbientCapabilities drop-in",
        )
        .why("This is not an ephemeral grant, it is no grant. See PF-XDP-0001.")
        .verify("grep CapPrm /proc/$(pgrep -f agave-validator)/status"),
        Some(_) => Outcome::ephemeral(
            "validator holds permitted capabilities, but the unit grants none, so they come from setcap on the binary",
            "capabilities granted by an AmbientCapabilities drop-in",
        )
        .why(
            "setcap writes to the binary's extended attributes, and agave-install replaces the \
             binary on every upgrade. Agave's own changelog says the command must be repeated \
             every time the binary is replaced. The node works for weeks, then fails to start \
             after a routine upgrade, and the two events look unrelated.",
        )
        .fix(vec![FixStep::noted(
            format!("move the grant into /etc/systemd/system/{unit}.d/20-xdp-caps.conf as AmbientCapabilities="),
            "or re-run setcap after every agave-install, which nobody remembers to do",
        )])
        .verify("getcap $(which agave-validator)")
        .persists(Persistence::unit_dropin(None, &unit)),
    }
}

fn runtime_capprm(ctx: &Ctx) -> Option<u64> {
    let pid = ctx.validator_pid.as_ref()?;
    if !crate::privilege::valid_pid(&ctx.fs, pid) {
        return None;
    }
    let status = ctx.fs.read(format!("/proc/{pid}/status")).ok()?;
    let line = status.lines().find(|l| l.starts_with("CapPrm:"))?;
    u64::from_str_radix(line.split_whitespace().nth(1)?, 16).ok()
}

/// True when the unit or a drop-in was written after the process started, so a
/// restart would pick up something the running process never saw. The /proc
/// entry's mtime is the process start time, which needs no exec to read.
fn grant_postdates_launch(ctx: &Ctx) -> Option<bool> {
    let inv = ctx.inv()?;
    let pid = inv.pid.as_ref()?;
    let started = std::fs::metadata(ctx.fs.at(format!("/proc/{pid}")))
        .ok()?
        .modified()
        .ok()?;
    let unit = inv.unit_path.as_ref()?;
    let mut newest = std::fs::metadata(ctx.fs.at(unit)).ok()?.modified().ok()?;
    for p in ctx.fs.list(format!("{unit}.d")) {
        if let Ok(t) = std::fs::metadata(&p).and_then(|m| m.modified()) {
            newest = newest.max(t);
        }
    }
    Some(newest > started)
}
