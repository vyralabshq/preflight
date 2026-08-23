//! ARG layer. Did the last upgrade break the command line?
//!
//! Pure functions of the resolved invocation and the detected version. No
//! privileges, no hardware, no network, which makes this the only layer that
//! runs correctly against a command line pasted from another machine.

use crate::{
    argv::Invocation,
    ctx::Ctx,
    model::{FixStep, Outcome, Source, SourceKind::*, ValueCarry},
};

pub const PORT_RANGE_MIN_WIDTH: u16 = 26;

pub const S_PORTS: &[Source] = &[
    Source {
        kind: AgaveSymbol,
        locator: "MINIMUM_VALIDATOR_PORT_RANGE_WIDTH",
        verified_against: "v4.2.1",
        provisional: false,
    },
    Source {
        kind: AgaveChangelog,
        locator: "v4.1 Validator/Breaking",
        verified_against: "v4.2.1",
        provisional: false,
    },
];
pub const S_XDP_PRIV: &[Source] = &[Source {
    kind: AgaveChangelog,
    locator: "v4.2 Validator/Changes",
    verified_against: "v4.2.1",
    provisional: false,
}];
pub const S_REMOVED_40: &[Source] = &[Source {
    kind: AgaveChangelog,
    locator: "v4.0 Validator/Breaking",
    verified_against: "v4.2.1",
    provisional: false,
}];
pub const S_BVM: &[Source] = &[Source {
    kind: AgaveChangelog,
    locator: "v4.0 Validator/Breaking",
    verified_against: "v4.2.1",
    provisional: false,
}];
pub const S_BPM: &[Source] = &[Source {
    kind: AgaveChangelog,
    locator: "v4.1 Validator/Breaking",
    verified_against: "v4.2.1",
    provisional: false,
}];
pub const S_XDP_DEP: &[Source] = &[Source {
    kind: AgaveChangelog,
    locator: "v4.1 Validator/Deprecations",
    verified_against: "v4.2.1",
    provisional: false,
}];
pub const S_POH: &[Source] = &[Source {
    kind: AgaveChangelog,
    locator: "v4.2 Validator/Deprecations",
    verified_against: "v4.2.1",
    provisional: false,
}];
pub const S_ADB: &[Source] = &[
    Source {
        kind: AgaveChangelog,
        locator: "v4.1 Validator/Deprecations",
        verified_against: "v4.2.1",
        provisional: false,
    },
    Source {
        kind: AgaveChangelog,
        locator: "v4.2 Validator/Deprecations",
        verified_against: "v4.2.1",
        provisional: false,
    },
];
pub const S_INDEX: &[Source] = &[Source {
    kind: AgaveChangelog,
    locator: "v4.1 Validator/Deprecations",
    verified_against: "v4.2.1",
    provisional: false,
}];
pub const S_DIO: &[Source] = &[Source {
    kind: AgaveChangelog,
    locator: "v4.0 Validator/Changes",
    verified_against: "v4.2.1",
    provisional: false,
}];
pub const S_43_LEDGER: &[Source] = &[
    Source {
        kind: AgaveSymbol,
        locator: "BlockstoreCleanupStrategy, CountDataShreds vs CountDataAndCodingShreds",
        verified_against: "agave master",
        provisional: false,
    },
    Source {
        kind: AgaveSymbol,
        locator: "LEGACY_DEFAULT_MAX_LEDGER_SHREDS 200000000, DEFAULT_MAX_BLOCKSTORE_SHREDS 400000000",
        verified_against: "agave master",
        provisional: false,
    },
    Source {
        kind: AgaveSymbol,
        locator: "--limit-blockstore-size, present in agave-validator --help",
        verified_against: "4.3.0-beta.0",
        provisional: false,
    },
];
pub const S_43_TRACE: &[Source] = &[Source {
    kind: AgaveChangelog,
    locator: "4.3.0-Unreleased Validator/Deprecations",
    verified_against: "master 2026-08",
    provisional: true,
}];
pub const S_43_POOL: &[Source] = &[Source {
    kind: AgaveSymbol,
    locator: "--tpu-connection-pool-size, absent from agave-validator --help",
    verified_against: "4.3.0-beta.0",
    provisional: false,
}];

const REMOVED_IN_40: &[&str] = &[
    "--accounts-db-clean-threads",
    "--accounts-db-hash-threads",
    "--accounts-db-read-cache-limit-mb",
    "--accounts-hash-cache-path",
    "--cuda",
    "--disable-accounts-disk-index",
    "--dev-halt-at-slot",
    "--transaction-struct",
    "--tpu-coalesce-ms",
    "--tpu-disable-quic",
    "--tpu-enable-udp",
];

const DEPRECATED_XDP: &[&str] = &[
    "--experimental-retransmit-xdp-interface",
    "--experimental-retransmit-xdp-cpu-cores",
    "--experimental-retransmit-xdp-zero-copy",
];

const DEPRECATED_ADB: &[&str] = &[
    "--accounts-db-cache-limit-mb",
    "--accounts-db-access-storages-method",
    "--account-shrink-path",
];

/// An unresolved invocation is Unknown with the trail printed, never an empty
/// flag set treated as "no flags set".
fn require_invocation(ctx: &Ctx) -> Result<&Invocation, Box<Outcome>> {
    if !ctx.validator_present {
        return Err(Box::new(Outcome::skipped("no validator on this host")));
    }
    // A bare $FLAGS leaves no flags to read, which is not the same as no flags
    // set. Passing thirteen checks off an unexpanded token is the worst answer
    // this layer can give.
    if let Some(inv) = ctx.inv()
        && let Some(tok) = inv.unresolved.iter().find(|t| !t.starts_with('-'))
    {
        return Err(Box::new(
            Outcome::unknown(format!(
                "the command line still contains {tok}, so preflight never saw the real flags"
            ))
            .expected("a command line with every token expanded")
            .why(
                "preflight reads the invocation as text and does not run a shell, so a variable \
                 the shell would expand stays literal here. An unexpanded token that is not \
                 itself a flag could hold any number of arguments, and treating what is left as \
                 the whole command line would report every check in this layer as passing on \
                 flags nobody read.",
            )
            .verify("tr '\\0' ' ' < /proc/$(pgrep -n agave-validator)/cmdline"),
        ));
    }
    ctx.inv().ok_or_else(|| {
        Box::new(
            Outcome::unknown(format!(
                "could not resolve a validator invocation ({})",
                ctx.invocation_trail.join(" -> ")
            ))
            .expected("a resolvable command line")
            .why(
                "Every check in this layer reads the validator's command line. preflight looks \
                 for a running process, then a systemd unit's ExecStart, then the wrapper script \
                 ExecStart points at. None resolved, so there is nothing to check rather than \
                 nothing wrong. Pass --invocation <file> to check a command line directly.",
            ),
        )
    })
}

/// Version gate. Skipped when the client predates the release that introduced
/// the requirement, Unknown when no version was detected. Warning a stable
/// operator about an alpha-channel change is worse than saying nothing.
fn require_version(ctx: &Ctx, major: u64, minor: u64) -> Result<(), Box<Outcome>> {
    match &ctx.version {
        Some(v) if v.at_least(major, minor) => Ok(()),
        Some(v) => Err(Box::new(Outcome::skipped(format!(
            "client is {}, requirement starts at v{major}.{minor}",
            v.short()
        )))),
        None => Err(Box::new(
            Outcome::unknown("client version not detected")
                .expected("a client version, so preflight knows which releases apply")
                .why(
                    "This requirement applies from a specific release onward. Without a version \
             preflight cannot tell whether it applies to you, and guessing would mean warning \
             operators about changes that do not affect them.",
                )
                .fix(vec![FixStep::noted(
                    "preflight --client agave-validator@<version>",
                    "get it with: agave-validator --version",
                )]),
        )),
    }
}

/// Open the file that holds the command line, apply the change, restart the unit.
fn edit_steps(ctx: &Ctx, changes: Vec<FixStep>) -> Vec<FixStep> {
    let mut steps = vec![match ctx.inv().and_then(|i| i.edit_target.clone()) {
        Some(f) => FixStep::cmd(format!("edit {f}")),
        None => FixStep::noted(
            "edit your validator command line",
            "preflight could not resolve which file holds it; see the trail in the header",
        ),
    }];
    steps.extend(changes.into_iter().map(|c| FixStep {
        command: format!("  {}", c.command),
        note: c.note,
    }));
    if let Some(u) = ctx.inv().and_then(|i| i.unit_name.clone()) {
        steps.push(FixStep::cmd(format!("sudo systemctl restart {u}")));
    }
    steps
}

fn edit_target_or(ctx: &Ctx) -> String {
    ctx.inv()
        .and_then(|i| i.edit_target.clone())
        .unwrap_or_else(|| "<your validator command line>".to_string())
}

/// Never print `sol.service` as a guess: it is only Anza's example unit name.
fn unit_or_placeholder(ctx: &Ctx) -> String {
    ctx.inv()
        .and_then(|i| i.unit_name.clone())
        .unwrap_or_else(|| "<your-validator-unit>".to_string())
}

/// Every check in this layer opens the same way: resolve the command line,
/// then decide whether this release is even in scope. Written out thirteen
/// times it was ~80 lines of identical prelude that obscured the actual check.
macro_rules! gate {
    ($ctx:expr, $major:literal, $minor:literal, $why:expr) => {{
        match require_invocation($ctx) {
            Err(o) => return o.why($why),
            Ok(inv) => match require_version($ctx, $major, $minor) {
                Err(o) => return o.why($why),
                Ok(()) => inv,
            },
        }
    }};
}

/// A flag whose value could not be expanded is Unknown, never guessed.
fn unexpanded(inv: &Invocation, flag: &str) -> bool {
    inv.value(flag).is_some_and(|v| v.contains('$'))
}

fn deprecated(
    ctx: &Ctx,
    found: Vec<String>,
    expected: &str,
    why: &str,
    changes: Vec<String>,
) -> Outcome {
    if found.is_empty() {
        return Outcome::pass("none present", expected).why(why);
    }
    let steps = edit_steps(ctx, changes.into_iter().map(FixStep::cmd).collect());
    Outcome::fail(format!("present: {}", found.join(", ")), expected)
        .why(why)
        .fix(steps)
}

/// PF-ARG-0001. The demonstration that this layer matters: Anza's own
/// validator-start guide example is `--dynamic-port-range 11000-11020`.
///
/// The range is half-open, `[start, end)`, per agave's own comment in
/// `port_range_validator`. Width is `end - start`, so 11000-11020 is 20, not
/// 21. Computing `end - start + 1` produces a false PASS at the boundary:
/// 11000-11025 would read as 26 while agave reads 25 and refuses to bind, which
/// is a green result on a box that will not start.
pub fn port_range(ctx: &Ctx) -> Outcome {
    const WHY: &str = "The range is half-open: [start, end), so the end port is not one of \
        yours. 8000-8026 gives you 26 ports, not 27, and 8000-8020 gives 20. Since v4.1 the \
        validator needs 26, and agave checks end - start against \
        MINIMUM_VALIDATOR_PORT_RANGE_WIDTH at startup and refuses to run when it falls short, so \
        the node does not start at all. The off-by-one is why this is worth checking: every \
        widely copied example invocation, including the one in Anza's own validator-start guide, \
        predates the change and is 20 wide.";
    const EXPECTED: &str = "a range at least 26 wide, or unset";

    let inv = gate!(ctx, 4, 1, WHY);
    let Some(raw) = inv.value("--dynamic-port-range") else {
        return Outcome::pass("not set, agave default 8000-10000 is 2000 wide", EXPECTED).why(WHY);
    };
    if unexpanded(inv, "--dynamic-port-range") {
        return Outcome::unknown(format!("--dynamic-port-range {raw} (unexpanded variable)"))
            .why(WHY);
    }
    let parsed = raw
        .split_once('-')
        .and_then(|(a, b)| Some((a.trim().parse::<u32>().ok()?, b.trim().parse::<u32>().ok()?)));
    let Some((start, end)) = parsed else {
        return Outcome::unknown(format!("--dynamic-port-range {raw} (unparseable)")).why(WHY);
    };

    let width = end.saturating_sub(start);
    if width >= PORT_RANGE_MIN_WIDTH as u32 {
        return Outcome::pass(
            format!("--dynamic-port-range {raw} ({width} wide)"),
            EXPECTED,
        )
        .why(WHY)
        .verify("ss -lntup | grep -c agave-validator");
    }
    // 30 leaves headroom above the 26 required, so the next bump does not break
    // the operator again.
    Outcome::fail(
        format!("--dynamic-port-range {raw} ({width} wide)"),
        EXPECTED,
    )
    .why(WHY)
    .fix(edit_steps(
        ctx,
        vec![FixStep::noted(
            format!("--dynamic-port-range {raw}   ->   {start}-{}", start + 30),
            "30 leaves headroom above the 26 required",
        )],
    ))
    .verify(format!(
        "grep -- --dynamic-port-range {}",
        edit_target_or(ctx)
    ))
}

/// PF-ARG-0002. The check that catches a fresh Lima VM, which is exactly the
/// configuration most likely to use private addressing.
pub fn private_addr_xdp(ctx: &Ctx) -> Outcome {
    const WHY: &str = "XDP transmit is on by default on Linux since v4.2, and gossip egress \
        under XDP does not support private or loopback addresses. A node using private \
        addressing must opt out of XDP explicitly or it will not gossip.";
    const EXPECTED: &str = "--no-xdp alongside --allow-private-addr";

    let inv = gate!(ctx, 4, 2, WHY);
    if !inv.has("--allow-private-addr") {
        return Outcome::pass("--allow-private-addr not in use", EXPECTED).why(WHY);
    }
    if inv.has("--no-xdp") {
        return Outcome::pass("--allow-private-addr paired with --no-xdp", EXPECTED).why(WHY);
    }
    Outcome::fail("--allow-private-addr set, --no-xdp absent", EXPECTED)
        .why(WHY)
        .fix(edit_steps(
            ctx,
            vec![FixStep::noted(
                "add --no-xdp",
                "alongside --allow-private-addr",
            )],
        ))
}

/// PF-ARG-0003. The eleven arguments removed under v4.0 **Validator**/Breaking.
///
/// Deliberately excludes `--use-quic` and `--use-udp`: those were removed under
/// v4.0 **CLI**, meaning the `solana` binary, not `agave-validator`. Putting
/// them here would name flags that never appeared in a validator invocation.
/// Also excludes `--snapshot-interval-slots 0`, removed in v3.0 and below any
/// version this layer gates on, and `--monitor` / `--wait-for-exit`, which
/// belong to the `exit` subcommand rather than a run invocation.
pub fn removed_in_40(ctx: &Ctx) -> Outcome {
    const WHY: &str = "These arguments were removed in v4.0. They are not deprecated: they no \
        longer parse, so the validator exits at startup rather than running with a different \
        setting. An upgrade turns a working node into one that will not boot.";
    const EXPECTED: &str = "none of the arguments removed in v4.0";

    let inv = gate!(ctx, 4, 0, WHY);
    let removed = inv.present_from(REMOVED_IN_40);
    let changes = removed.iter().map(|f| format!("remove {f}")).collect();
    deprecated(ctx, removed.clone(), EXPECTED, WHY, changes).verify(format!(
        "grep -c -- {} {}   # expect 0",
        removed.first().map(String::as_str).unwrap_or("--cuda"),
        edit_target_or(ctx)
    ))
}

/// PF-ARG-0004.
pub fn block_verification_method(ctx: &Ctx) -> Outcome {
    const WHY: &str = "blockstore-processor stopped being supported in v4.0. unified-scheduler \
        is the replacement, and removing the argument entirely selects the current default.";
    const EXPECTED: &str = "--block-verification-method unified-scheduler, or the argument removed";

    let inv = gate!(ctx, 4, 0, WHY);
    match inv.value("--block-verification-method").as_deref() {
        Some("blockstore-processor") => {
            Outcome::fail("--block-verification-method blockstore-processor", EXPECTED)
                .why(WHY)
                .fix(edit_steps(
                    ctx,
                    vec![FixStep::cmd(
                        "--block-verification-method blockstore-processor   ->   unified-scheduler",
                    )],
                ))
        }
        Some(other) => {
            Outcome::pass(format!("--block-verification-method {other}"), EXPECTED).why(WHY)
        }
        None => Outcome::pass("not set, using the current default", EXPECTED).why(WHY),
    }
}

/// PF-ARG-0005. Cited to v4.1 Breaking, not v4.0 Changes. v4.0 deprecated the
/// value; v4.1 stopped supporting it and made it silently fall back. Citing the
/// earlier entry would be one release stale and materially wrong about what the
/// validator does.
pub fn block_production_method(ctx: &Ctx) -> Outcome {
    const WHY: &str = "central-scheduler is no longer supported as of v4.1. If passed, a warning \
        is emitted and behaviour defaults to the greedy scheduler. The node runs, so nothing \
        looks broken, but the scheduler you selected is not the one producing your blocks.";
    const EXPECTED: &str =
        "--block-production-method central-scheduler-greedy, or the argument removed";

    let inv = gate!(ctx, 4, 1, WHY);
    match inv.value("--block-production-method").as_deref() {
        Some("central-scheduler") => Outcome::fail(
            "--block-production-method central-scheduler",
            EXPECTED,
        )
        .why(WHY)
        .fix(edit_steps(
            ctx,
            vec![FixStep::cmd(
                "--block-production-method central-scheduler   ->   central-scheduler-greedy",
            )],
        ))
        .verify(format!(
            "journalctl -u {} -n 200 | grep -i central-scheduler",
            unit_or_placeholder(ctx)
        )),
        Some(other) => {
            Outcome::pass(format!("--block-production-method {other}"), EXPECTED).why(WHY)
        }
        None => Outcome::pass("not set, greedy scheduler is the default", EXPECTED).why(WHY),
    }
}

/// PF-ARG-0006. Anza's XDP setup guide still documents the old names, so a unit
/// written from that post carries all three.
pub fn deprecated_xdp_flags(ctx: &Ctx) -> Outcome {
    const WHY: &str = "XDP stopped being experimental in v4.1 and the \
        --experimental-retransmit-xdp-* flags were deprecated in favour of --xdp-interface, \
        --xdp-cpu-cores and --xdp-zero-copy. Behaviour is unchanged for now, but Anza's XDP \
        setup guide still documents the old names, so units written from that post carry all \
        three and will break when the aliases are removed.";
    const EXPECTED: &str = "--xdp-interface, --xdp-cpu-cores, --xdp-zero-copy";

    let inv = gate!(ctx, 4, 1, WHY);
    let found = inv.present_from(DEPRECATED_XDP);
    let changes = found
        .iter()
        .map(|f| {
            format!(
                "{f}   ->   {}",
                f.replace("--experimental-retransmit-xdp-", "--xdp-")
            )
        })
        .collect();
    deprecated(ctx, found, EXPECTED, WHY, changes)
}

/// PF-ARG-0007.
pub fn deprecated_poh_flag(ctx: &Ctx) -> Outcome {
    const WHY: &str = "Deprecated in v4.2 in favour of --poh-pinned-cpu-core. The experimental \
        name still parses, so nothing breaks today, but it is on the same removal path the \
        experimental XDP flags are on, and the two are usually set together by operators who \
        followed the same guide.";
    const EXPECTED: &str = "--poh-pinned-cpu-core";

    let inv = gate!(ctx, 4, 2, WHY);
    deprecated(
        ctx,
        inv.present_from(&["--experimental-poh-pinned-cpu-core"]),
        EXPECTED,
        WHY,
        vec!["--experimental-poh-pinned-cpu-core   ->   --poh-pinned-cpu-core".to_string()],
    )
}

/// PF-ARG-0008.
pub fn deprecated_accounts_db(ctx: &Ctx) -> Outcome {
    const WHY: &str = "Deprecated across v4.1 and v4.2. They still parse, so the node starts and \
        nothing looks wrong, but a deprecated flag no longer necessarily does what its name says.";
    const WHY_CACHE: &str = " --accounts-db-cache-limit-mb is superseded by \
        --accounts-db-write-cache-limit, so the size you set here is not the one in effect. The \
        old name carries its unit and the new one does not, and preflight has not read the \
        replacement's unit from your binary, so it will not tell you what number to put there.";
    const WHY_NOOP: &str = " --accounts-db-access-storages-method is a no-op since v4.2, because \
        mmap mode was removed entirely: whoever set it believes they chose a storage access mode \
        and did not.";
    const EXPECTED: &str = "none of the deprecated accounts-db arguments";

    let inv = gate!(ctx, 4, 1, WHY);
    let found = inv.present_from(DEPRECATED_ADB);
    let changes: Vec<String> = found
        .iter()
        .filter(|f| f.as_str() != "--accounts-db-cache-limit-mb")
        .map(|f| match f.as_str() {
            "--accounts-db-access-storages-method" => {
                "remove --accounts-db-access-storages-method   (no-op since v4.2)".to_string()
            }
            other => format!("remove {other}"),
        })
        .collect();

    // Explain the flags that actually fired, not the whole family. Describing a
    // flag the operator does not have reads as a bug in the tool.
    let mut why = WHY.to_string();
    let has = |f: &str| found.iter().any(|x| x == f);
    if has("--accounts-db-cache-limit-mb") {
        why.push_str(WHY_CACHE);
    }
    if has("--accounts-db-access-storages-method") {
        why.push_str(WHY_NOOP);
    }
    // The old name carries its unit and the new one does not. preflight has not
    // read the replacement's unit, so it names both flags and refuses to
    // suggest a number.
    let cache_value = inv.value("--accounts-db-cache-limit-mb");
    let mut out = deprecated(ctx, found, EXPECTED, &why, changes);
    if cache_value.is_some() {
        let step = FixStep::rename(
            "--accounts-db-cache-limit-mb",
            "--accounts-db-write-cache-limit",
            cache_value.as_deref(),
            ValueCarry::Unverified,
        );
        // Before the restart, or the block reads "restart, then edit".
        let at = out
            .fix
            .iter()
            .position(|s| s.command.starts_with("sudo systemctl restart"))
            .unwrap_or(out.fix.len());
        out.fix.insert(
            at,
            FixStep {
                command: format!("  {}", step.command),
                note: step.note,
            },
        );
    }
    out
}

/// PF-ARG-0009. Reported as a deprecation on released channels. The behaviour
/// change, where `minimal` comes to mean 25 GB, is an unreleased-channel entry
/// and is not stated as advice to a v4.1 or v4.2 operator.
pub fn accounts_index_limit(ctx: &Ctx) -> Outcome {
    const WHY: &str = "The value 'minimal' is deprecated as of v4.1. It is a relative word whose \
        meaning is set by the release, not by you, so the allocation you get is not necessarily \
        the allocation you chose. Set a size.";
    const EXPECTED: &str = "an explicit size, e.g. --accounts-index-limit 25GB";

    let inv = gate!(ctx, 4, 1, WHY);
    match inv.value("--accounts-index-limit").as_deref() {
        Some("minimal") => Outcome::fail("--accounts-index-limit minimal", EXPECTED)
            .why(WHY)
            .fix(edit_steps(ctx, vec![FixStep::noted("--accounts-index-limit minimal   ->   --accounts-index-limit <size>", "pick a size for your box; 'minimal' does not mean the same thing across releases")])),
        Some(other) => Outcome::pass(format!("--accounts-index-limit {other}"), EXPECTED).why(WHY),
        None => Outcome::pass("not set", EXPECTED).why(WHY),
    }
}

/// PF-ARG-0010. The one check in this layer that is not pure argv: it
/// cross-references the filesystem holding the accounts path. It stays here
/// because the remediation is a flag. Paired with PF-FS-0007.
pub fn direct_io(ctx: &Ctx) -> Outcome {
    const WHY: &str = "Snapshot archive unpacking uses direct I/O by default since v4.0 to \
        bypass the page cache. On a filesystem without O_DIRECT the unpack fails, and the \
        failure surfaces during a snapshot restore rather than at startup, which is the worst \
        time to discover it.";
    const EXPECTED: &str = "--no-accounts-db-snapshots-direct-io present only when the accounts filesystem lacks O_DIRECT";

    let inv = gate!(ctx, 4, 0, WHY);
    let opted_out = inv.has("--no-accounts-db-snapshots-direct-io");
    let Some(accounts) = inv.value("--accounts") else {
        return Outcome::unknown(
            "--accounts not set in the invocation, cannot resolve its filesystem",
        )
        .why(WHY);
    };
    let all = crate::checks::fs::mounts(ctx);
    if all.is_empty() {
        return Outcome::unknown(
            "cannot read /proc/mounts: this check needs a Linux host, or --root pointed at one",
        )
        .why(WHY);
    }
    let Some(best) = crate::checks::fs::mount_for(&all, &accounts) else {
        return Outcome::unknown(format!("no mount found for {accounts}")).why(WHY);
    };
    let no_odirect = matches!(
        best.fstype.as_str(),
        "tmpfs" | "zfs" | "nfs" | "nfs4" | "overlay"
    );
    match (no_odirect, opted_out) {
        (true, false) => Outcome::fail(
            format!(
                "{accounts} is on {} ({}), --no-accounts-db-snapshots-direct-io absent",
                best.target, best.fstype
            ),
            EXPECTED,
        )
        .why(WHY)
        .fix(edit_steps(
            ctx,
            vec![FixStep::cmd("add --no-accounts-db-snapshots-direct-io")],
        )),
        (false, true) => Outcome::pass(
            format!(
                "opted out of direct I/O on {} ({}), which does support O_DIRECT",
                best.target, best.fstype
            ),
            EXPECTED,
        )
        .why(WHY),
        _ => Outcome::pass(
            format!("{} is {}, setting is consistent", best.target, best.fstype),
            EXPECTED,
        )
        .why(WHY),
    }
}

/// PF-ARG-0011. Confirmed against a real 4.3.0-beta.0 binary, which lists
/// --limit-blockstore-size in its help. The changelog said so; the binary
/// settles it.
pub fn limit_ledger_size(ctx: &Ctx) -> Outcome {
    const WHY: &str = "The two flags count different things. BlockstoreCleanupStrategy has one \
        variant per flag: CountDataShreds for --limit-ledger-size, CountDataAndCodingShreds for \
        --limit-blockstore-size. Turbine erasure-codes every block, so the store holds coding \
        shreds alongside the data ones and both take real disk. The old flag counted only the \
        data half, so the store held roughly twice whatever you set, and the multiple moved with \
        the cluster's erasure ratio. The new one counts everything that is actually there, which \
        is why disk use may read higher at steady state and yet stay steadier when the cluster \
        gets strange.";
    const EXPECTED: &str = "--limit-blockstore-size";

    let inv = gate!(ctx, 4, 3, WHY);
    if !inv.has("--limit-ledger-size") {
        return Outcome::pass("--limit-ledger-size not in use", EXPECTED).why(WHY);
    }
    // The doubling guidance only applies to a value someone chose. On the
    // default the swap is a rename, and telling an operator to double a number
    // they never set would have them invent one.
    match inv.value("--limit-ledger-size") {
        Some(n) => {
            let doubled = n
                .parse::<u64>()
                .map(|v| (v * 2).to_string())
                .unwrap_or_else(|_| "twice that".into());
            Outcome::fail(format!("--limit-ledger-size {n}"), EXPECTED)
                .why(format!(
                    "{WHY} You set {n}, which counted data shreds only, so about {doubled} keeps \
                     the same history under the flag that counts both."
                ))
                .fix(edit_steps(
                    ctx,
                    vec![FixStep::rename(
                        "--limit-ledger-size",
                        "--limit-blockstore-size",
                        Some(&n),
                        ValueCarry::DifferentSemantics(
                            "the new flag counts coding shreds too, so roughly double it; a \
                             starting point, not a conversion",
                        ),
                    )],
                ))
        }
        None => Outcome::fail("--limit-ledger-size, with no value", EXPECTED)
            .why(format!(
                "{WHY} You are on the default. The defaults already sit in that ratio: \
                 LEGACY_DEFAULT_MAX_LEDGER_SHREDS is 200,000,000 data shreds and \
                 DEFAULT_MAX_BLOCKSTORE_SHREDS is 400,000,000 data and coding shreds, so the \
                 rename keeps the same retention with nothing to convert."
            ))
            .fix(edit_steps(
                ctx,
                vec![FixStep::rename(
                    "--limit-ledger-size",
                    "--limit-blockstore-size",
                    None,
                    ValueCarry::NoValueSet,
                )],
            )),
    }
}

/// PF-ARG-0012. Still provisional, and `--help` cannot settle it: a deprecated
/// no-op is hidden from help while still being accepted, so its absence there
/// proves nothing either way.
pub fn disable_banking_trace(ctx: &Ctx) -> Outcome {
    const WHY: &str = "Banking trace is disabled by default and this flag is a no-op, accepted \
        only for compatibility. Nothing breaks, but the flag no longer expresses anything, and \
        an operator reading their own command line would conclude they had turned something off \
        that was already off.";
    const EXPECTED: &str = "the argument removed";

    let inv = gate!(ctx, 4, 3, WHY);
    if !inv.has("--disable-banking-trace") {
        return Outcome::pass("--disable-banking-trace not in use", EXPECTED).why(WHY);
    }
    Outcome::fail("--disable-banking-trace present", EXPECTED)
        .why(WHY)
        .fix(edit_steps(
            ctx,
            vec![FixStep::noted(
                "remove --disable-banking-trace",
                "to enable tracing instead, pass --enable-banking-trace <max bytes>",
            )],
        ))
}

/// PF-ARG-0013. The flag is gone from a real 4.3.0-beta.0 binary's help, which
/// matches the changelog calling it removed. A removed flag stops the validator
/// starting, so this stays fatal.
pub fn tpu_connection_pool_size(ctx: &Ctx) -> Outcome {
    const WHY: &str = "Removed. The connection pool size is fixed at 1, the previous default. \
        Like every removal this is a startup failure rather than a behaviour change.";
    const EXPECTED: &str = "the argument removed";

    let inv = gate!(ctx, 4, 3, WHY);
    if !inv.has("--tpu-connection-pool-size") {
        return Outcome::pass("--tpu-connection-pool-size not in use", EXPECTED).why(WHY);
    }
    Outcome::fail("--tpu-connection-pool-size present", EXPECTED)
        .why(WHY)
        .fix(edit_steps(
            ctx,
            vec![FixStep::cmd("remove --tpu-connection-pool-size")],
        ))
}
