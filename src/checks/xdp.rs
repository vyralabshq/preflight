//! XDP layer. Gated on the detected binary, not the profile:
//! solana-test-validator has no XDP transmit path at all, while a non-voting
//! agave-validator on Linux hits the v4.2 capability gate like any other node.

use crate::{
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

const WHY_CAPS: &str = "Since v4.0 agave exits(1) if a capability required by the current \
    configuration is not in the process's permitted set. CapabilityBoundingSet caps what a \
    process may ever hold; it grants nothing. On a unit with User= set to a non-root account the \
    bounding set alone leaves the permitted set empty. Agave's changelog pairs the bounding set \
    with AmbientCapabilities; the Anza XDP blog post shows only the bounding set, so unit files \
    written from that post fail exactly this way.";

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

fn unit_directive(ctx: &Ctx, key: &str) -> Option<String> {
    let unit = ctx.inv()?.unit_path.as_ref()?;
    let mut texts = Vec::new();
    if let Ok(t) = ctx.fs.read(unit) {
        texts.push(t);
    }
    for p in ctx.fs.list(format!("{unit}.d")) {
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

    if inv.unit_path.is_none() {
        return Outcome::unknown(
            "invocation did not come from a systemd unit, so no capability directives to read",
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

    // Never guess a unit name; see unit_or_placeholder in checks/arg.rs.
    let unit = inv
        .unit_name
        .clone()
        .unwrap_or_else(|| "<your-validator-unit>".into());
    let caps = state.required.join(" ");

    // A drop-in CapabilityBoundingSet= replaces the unit's value rather than
    // adding to it, so emitting one when the unit already permits what is
    // needed would silently narrow the operator's bounding set. Write only the
    // half that is actually missing.
    // A drop-in replaces this directive, so only write it when it is missing.
    let bounding_covers = bounding.as_deref().is_some_and(|b| {
        let have: Vec<String> = b.split_whitespace().map(|s| s.to_uppercase()).collect();
        state.required.iter().all(|c| have.contains(&c.to_string()))
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

    Outcome::fail(observed, expected)
        .why(WHY_CAPS)
        .fix(vec![
            FixStep::cmd(format!("sudo mkdir -p /etc/systemd/system/{unit}.d")),
            FixStep::noted(
                format!("printf '[Service]\\n{body}\\n' | sudo tee /etc/systemd/system/{unit}.d/20-xdp-caps.conf"),
                note,
            ),
            FixStep::cmd("sudo systemctl daemon-reload"),
            FixStep::cmd(format!("sudo systemctl restart {unit}")),
        ])
        .verify("grep CapPrm /proc/$(pgrep -f agave-validator)/status")
        .persists(Persistence::unit_dropin(ambient, &unit))
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
