//! SVC layer. The systemd unit itself, rather than the command line it runs.
//!
//! A unit can be correct about what to start and wrong about when, and systemd
//! reports neither.

use crate::{
    checks::{needs_linux, unit_directive_all},
    ctx::Ctx,
    model::{FixStep, Outcome, Source, SourceKind::*},
};

pub const S_MOUNTS: &[Source] = &[Source {
    kind: Operator,
    locator: "systemd.unit(5), RequiresMountsFor=",
    verified_against: "2026-09",
    provisional: false,
}];

/// PF-SVC-0001. The unit does not depend on the mounts it writes to.
pub fn requires_its_mounts(ctx: &Ctx) -> Outcome {
    const WHY: &str = "systemd.unit(5) says RequiresMountsFor= adds dependencies of type \
        Requires= and After= for the mount units a path needs. Without it a service is only \
        ordered after local-fs.target, which is not the same as depending on it: a mount that \
        fails, or one marked nofail, leaves boot to continue and the validator starts anyway. It \
        then writes into the empty mountpoint on the root filesystem instead of the device, which \
        nothing reports. The node runs, the accounts directory looks right, and the root \
        filesystem fills up over days.";
    const EXPECTED: &str = "the unit depends on every mount it writes to";

    if let Some(o) = needs_linux(ctx, WHY) {
        return o;
    }
    let Some(inv) = ctx.inv() else {
        return Outcome::skipped("no validator invocation to read paths from");
    };
    if inv.unit_path.is_none() {
        return Outcome::skipped("this invocation did not come from a systemd unit");
    }

    let all = crate::checks::fs::mounts(ctx);
    // Only a path on its own mount can be lost this way. A directory on the
    // root filesystem is there whether or not anything mounted.
    let separate: Vec<String> = ["--accounts", "--ledger", "--snapshots"]
        .iter()
        .filter_map(|f| inv.value(f))
        .filter(|p| crate::checks::fs::mount_for(&all, p).is_some_and(|m| m.target != "/"))
        .collect();
    if separate.is_empty() {
        return Outcome::skipped("no validator path sits on its own mount");
    }

    let declared: Vec<String> = ["RequiresMountsFor", "WantsMountsFor"]
        .iter()
        .flat_map(|k| unit_directive_all(ctx, k))
        .flat_map(|v| {
            v.split_whitespace()
                .map(str::to_string)
                .collect::<Vec<String>>()
        })
        .collect();
    let covered = |path: &str| {
        declared
            .iter()
            .any(|d| path == d || path.starts_with(&format!("{}/", d.trim_end_matches('/'))))
    };
    let uncovered: Vec<String> = separate.into_iter().filter(|p| !covered(p)).collect();

    if uncovered.is_empty() {
        return Outcome::pass(
            format!("the unit declares {}", declared.join(" ")),
            EXPECTED,
        )
        .why(WHY);
    }
    let unit = inv
        .unit_name
        .clone()
        .unwrap_or_else(|| "<your-validator-unit>".into());
    let phrasing = match uncovered.len() {
        1 => "sits on its own mount, and the unit does not depend on it",
        _ => "sit on their own mounts, and the unit depends on none of them",
    };
    Outcome::fail(format!("{} {phrasing}", uncovered.join(", ")), EXPECTED)
    .why(WHY)
    .fix(vec![
        FixStep::cmd(format!("sudo mkdir -p /etc/systemd/system/{unit}.d")),
        FixStep::noted(
            format!(
                "printf '[Unit]\\nRequiresMountsFor={}\\n' | sudo tee /etc/systemd/system/{unit}.d/10-mounts.conf",
                uncovered.join(" ")
            ),
            "a drop-in, so the packaged unit stays as it is",
        ),
        FixStep::noted(
            "sudo systemctl daemon-reload",
            "this changes only what systemd does at the next start, so the running validator is \
             untouched",
        ),
    ])
    .verify(format!("systemctl show {unit} -p RequiresMountsFor"))
}
