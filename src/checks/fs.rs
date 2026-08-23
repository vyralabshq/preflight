//! FS layer. Disks and filesystems.
//!
//! The only layer that answers "can this machine run a validator" in full
//! before anything is installed. With a validator present it checks the paths
//! that validator actually uses; without one it asks whether this box has
//! enough suitable storage to be worth installing on at all.

use crate::{
    checks::needs_linux,
    ctx::Ctx,
    host,
    model::{FixStep, Outcome, Source, SourceKind::*},
};

pub const S_REQ: &[Source] = &[Source {
    kind: AnzaDocs,
    locator: "docs.anza.xyz/operations/requirements, Disk Storage",
    verified_against: "2026-08",
    provisional: false,
}];

/// Anza's figures, plus a note that the headroom line is preflight's own.
pub const S_REQ_AND_OPERATOR: &[Source] = &[
    Source {
        kind: AnzaDocs,
        locator: "docs.anza.xyz/operations/requirements, Disk Storage",
        verified_against: "2026-08",
        provisional: false,
    },
    Source {
        kind: Operator,
        locator: "headroom figure is preflight's own, not published",
        verified_against: "2026-08",
        provisional: false,
    },
];

/// noatime is common operator practice, not something Anza publishes. Citing
/// their requirements page for it would be inventing a source.
pub const S_OPERATOR: &[Source] = &[Source {
    kind: Operator,
    locator: "operator practice, not published by Anza",
    verified_against: "2026-08",
    provisional: false,
}];
pub const S_DIO: &[Source] = &[Source {
    kind: AgaveChangelog,
    locator: "v4.0 Validator/Changes",
    verified_against: "v4.2.1",
    provisional: false,
}];

/// Anza's stated sizes, used as the mainnet baseline. See Profile::thresholds.
const NEED_TOTAL_GB: f64 = 2500.0;

const NETWORK_FS: &[&str] = &["nfs", "nfs4", "cifs", "smb3", "ceph", "glusterfs", "9p"];
const NO_ODIRECT: &[&str] = &["tmpfs", "zfs", "overlay", "nfs", "nfs4", "cifs", "9p"];

pub struct MountInfo {
    pub target: String,
    pub source: String,
    pub fstype: String,
    pub options: String,
}

pub fn mounts(ctx: &Ctx) -> Vec<MountInfo> {
    let Ok(text) = ctx.fs.read("/proc/mounts") else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|l| {
            let p: Vec<&str> = l.split_whitespace().collect();
            (p.len() >= 4).then(|| MountInfo {
                target: p[1].to_string(),
                source: p[0].to_string(),
                fstype: p[2].to_string(),
                options: p[3].to_string(),
            })
        })
        .collect()
}

/// The mount that actually holds a path: the longest matching mount point.
/// Longest mount whose target contains the path, on a slash boundary so
/// /mnt/accounts-old never matches /mnt/accounts. Shared with the ARG layer.
pub fn mount_for<'a>(all: &'a [MountInfo], path: &str) -> Option<&'a MountInfo> {
    all.iter()
        .filter(|m| {
            path == m.target
                || path.starts_with(&format!("{}/", m.target.trim_end_matches('/')))
                || m.target == "/"
        })
        .max_by_key(|m| m.target.len())
}

/// Strip the partition suffix so nvme0n1p2 and nvme0n1 read as one device.
/// Two paths on the same base device share the same queue.
fn base_device(source: &str) -> Option<String> {
    let name = source.strip_prefix("/dev/")?;
    let trimmed = if name.starts_with("nvme") {
        name.split('p').next().unwrap_or(name)
    } else {
        name.trim_end_matches(|c: char| c.is_ascii_digit())
    };
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// The paths this validator uses, when there is one.
fn validator_paths(ctx: &Ctx) -> Vec<(&'static str, String)> {
    let Some(inv) = ctx.inv() else {
        return Vec::new();
    };
    [
        ("accounts", "--accounts"),
        ("ledger", "--ledger"),
        ("snapshots", "--snapshots"),
    ]
    .iter()
    .filter_map(|(label, flag)| inv.value(flag).map(|v| (*label, v)))
    .collect()
}

/// Three checks share one shape: test each validator path, then report the
/// failures if there are any and the successes if there are not.
fn per_path<F>(ctx: &Ctx, expected: &str, why: &str, test: F) -> Outcome
where
    F: Fn(&MountInfo, &str) -> Result<String, String>,
{
    let paths = validator_paths(ctx);
    if paths.is_empty() {
        return Outcome::skipped("no validator paths to check yet");
    }
    let all = mounts(ctx);
    let (good, bad): (Vec<_>, Vec<_>) = paths
        .iter()
        .filter_map(|(label, path)| {
            // The path the operator gave, not just "ledger", since a check that
            // says "ledger at /" leaves them guessing which one it means.
            let named = format!("{label} {path}");
            mount_for(&all, path).map(|m| test(m, &named))
        })
        .partition(Result::is_ok);

    match bad.is_empty() {
        true => Outcome::pass(unwrap_all(good).join(", "), expected).why(why),
        false => Outcome::fail(unwrap_all(bad).join("; "), expected).why(why),
    }
}

fn unwrap_all(v: Vec<Result<String, String>>) -> Vec<String> {
    v.into_iter().map(|r| r.unwrap_or_else(|e| e)).collect()
}

/// PF-FS-0001. Storage, judged against the active profile.
///
/// mainnet gets Anza's figures. testnet and local have no published figures, so
/// they are judged on free headroom instead, which is the thing that actually
/// takes a node down.
pub fn capacity(ctx: &Ctx) -> Outcome {
    const WHY_SIZED: &str = "Anza gives one set of figures without saying which cluster they are \
        for: accounts 1 TB, ledger 1 TB, snapshots 500 GB, all high write endurance. They \
        describe a production node, so preflight applies them to mainnet.";
    const WHY_HEADROOM: &str = "Anza publishes no storage figures for this cluster, so the ledger \
        floor below is an operator one from running it, not anybody's published requirement: \
        250 GB carries testnet where mainnet wants Anza's 1 TB. What takes a node down is running \
        out, either partway through a snapshot download or weeks later as the ledger grows, and a \
        one-shot check sees a level rather than a rate, so treat the headroom line as a prompt to \
        look at your own trend.";

    if let Some(o) = needs_linux(ctx, WHY_HEADROOM) {
        return o;
    }
    let t = ctx.profile.thresholds();
    let all = mounts(ctx);
    let paths = validator_paths(ctx);

    if paths.is_empty() {
        return bare_box_capacity(ctx, t.accounts_gb.is_some(), WHY_SIZED, WHY_HEADROOM);
    }

    let want = |label: &str| match label {
        "accounts" => t.accounts_gb,
        // Anza's figure where there is one, the operator floor where there is not.
        "ledger" => t.ledger_gb.or(t.disk_gb),
        _ => t.snapshots_gb,
    };

    let mut short = Vec::new();
    let mut seen = Vec::new();
    for (label, path) in &paths {
        let Some(m) = mount_for(&all, path) else {
            short.push(format!("{label} {path}: no mount found"));
            continue;
        };
        let facts = ctx.facts.mounts.iter().find(|x| x.target == m.target);
        let Some(free) = facts.and_then(|x| x.free_gb) else {
            seen.push(format!("{label} free space not measured"));
            continue;
        };
        let total = facts.and_then(|x| x.total_gb).unwrap_or(0.0);
        let ratio = match total > 0.0 {
            true => free / total,
            false => 1.0,
        };
        // The floor is a device size, not a level. Comparing it against free
        // space fails a 943 GB disk for holding a ledger, which is its job.
        match want(label) {
            Some(need) if total > 0.0 && total < need => short.push(format!(
                "{label} {path}: {total:.0} GB filesystem, wants {need:.0} GB"
            )),
            _ if ratio < t.min_free => short.push(format!(
                "{label} {path}: {free:.0} GB free of {total:.0} GB, under {:.0}% headroom",
                t.min_free * 100.0
            )),
            _ => seen.push(format!("{label} {free:.0} GB free")),
        }
    }

    let why = match t.accounts_gb.is_some() {
        true => WHY_SIZED,
        false => WHY_HEADROOM,
    };
    let expected = match (t.accounts_gb, t.disk_gb) {
        (Some(_), _) => {
            format!("about {NEED_TOTAL_GB:.0} GB across the validator's paths, per Anza")
        }
        (None, Some(d)) => format!(
            "no published figure; a {d:.0} GB ledger filesystem and {:.0}% of each still free",
            t.min_free * 100.0
        ),
        (None, None) => format!(
            "no published figure; preflight looks for {:.0}% free on each path",
            t.min_free * 100.0
        ),
    };
    if short.is_empty() {
        // A pass implies a threshold was met. On a cluster nobody publishes
        // figures for there is no threshold, so this reports like cores and
        // memory do rather than claiming a bar was cleared.
        return match t.accounts_gb.is_some() {
            true => Outcome::pass(seen.join(", "), expected).why(why),
            false => Outcome::reported(seen.join(", "), expected).why(why),
        };
    }
    Outcome::fail(short.join("; "), expected).why(why)
}

/// No validator yet, so judge the devices rather than any path.
fn bare_box_capacity(ctx: &Ctx, sized: bool, why_sized: &str, why_headroom: &str) -> Outcome {
    let why = match sized {
        true => why_sized,
        false => why_headroom,
    };
    let expected = match sized {
        true => format!("about {NEED_TOTAL_GB:.0} GB, Anza's figures for a production node"),
        false => "no published figure for this cluster; reported for your judgement".to_string(),
    };
    if ctx.facts.disks.is_empty() {
        return Outcome::unknown("no block devices detected")
            .expected(expected)
            .why(why);
    }
    let usable: f64 = ctx
        .facts
        .disks
        .iter()
        .filter(|d| !d.rotational)
        .map(|d| d.size_gb)
        .sum();
    let observed = format!(
        "{usable:.0} GB across {} solid-state device(s)",
        ctx.facts.disks.iter().filter(|d| !d.rotational).count()
    );
    match sized && usable < NEED_TOTAL_GB {
        true => Outcome::fail(observed, expected)
            .why(why)
            .fix(vec![FixStep::noted(
                "accounts 1000 GB, ledger 1000 GB, snapshots 500 GB",
                "high write endurance matters as much as size; validators write constantly",
            )]),
        false => Outcome::pass(observed, expected).why(why),
    }
}

/// PF-FS-0002. Separate devices. Degraded, never Fatal: Anza permits sharing.
pub fn separate_devices(ctx: &Ctx) -> Outcome {
    const WHY: &str = "Anza says accounts and ledger can share a disk but that it is not \
        recommended, because both are IOPS-heavy and contend. It says nothing about snapshots, \
        which are written in bursts and are commonly kept alongside the ledger on purpose. \
        preflight only reports the pairing Anza actually cautions about.";
    const EXPECTED: &str = "accounts and ledger on separate block devices";

    if let Some(o) = needs_linux(ctx, WHY) {
        return o;
    }
    let paths = validator_paths(ctx);
    if paths.is_empty() {
        let ssds = ctx.facts.disks.iter().filter(|d| !d.rotational).count();
        let observed =
            format!("{ssds} solid-state device(s) available, no validator paths to place yet");
        // Severity carries "how bad"; status carries "is it wrong". Fewer than
        // three devices is a real finding, at Degraded severity in the registry.
        return match ssds {
            0 => Outcome::unknown(observed).expected(EXPECTED).why(WHY),
            1 | 2 => Outcome::fail(observed, EXPECTED)
                .why(WHY)
                .fix(vec![FixStep::noted(
                    "plan for three devices: accounts, ledger, snapshots",
                    "sharing is permitted and the node will run; it has less headroom under load",
                )]),
            _ => Outcome::pass(observed, EXPECTED).why(WHY),
        };
    }

    let all = mounts(ctx);
    let mut devices: Vec<(String, String)> = Vec::new();
    for (label, path) in &paths {
        if let Some(m) = mount_for(&all, path)
            && let Some(dev) = base_device(&m.source)
        {
            devices.push((label.to_string(), dev));
        }
    }
    if devices.len() < 2 {
        return Outcome::unknown("could not resolve the block devices behind these paths")
            .expected(EXPECTED)
            .why(WHY);
    }
    // Only the pairing Anza cautions about. Snapshots alongside the ledger is a
    // normal, deliberate choice and not something they warn against.
    let device_of = |label: &str| {
        devices
            .iter()
            .find(|(l, _)| l == label)
            .map(|(_, d)| d.clone())
    };
    let listing: Vec<String> = devices.iter().map(|(l, d)| format!("{l} on {d}")).collect();

    match (device_of("accounts"), device_of("ledger")) {
        (Some(a), Some(l)) if a == l => {
            Outcome::fail(format!("accounts and ledger both on {a}"), EXPECTED)
                .why(WHY)
                .fix(vec![FixStep::noted(
                    "move accounts or ledger to its own device",
                    "the node still runs either way; expect less headroom when the cluster is busy",
                )])
        }
        _ => Outcome::pass(listing.join(", "), EXPECTED).why(WHY),
    }
}

/// PF-FS-0003. Solid-state, and not somebody else's disk over a network.
pub fn storage_media(ctx: &Ctx) -> Outcome {
    const WHY: &str = "Anza specifies NVMe. A validator's write pattern destroys consumer drives \
        and starves spinning disks, and network-attached storage adds latency to every account \
        read on a path that has none to spare.";
    const EXPECTED: &str = "local NVMe or SSD";

    if let Some(o) = needs_linux(ctx, WHY) {
        return o;
    }
    // With no validator yet, judge the devices themselves.
    if validator_paths(ctx).is_empty() {
        if ctx.facts.disks.is_empty() {
            return Outcome::unknown("no block devices detected")
                .expected(EXPECTED)
                .why(WHY);
        }
        let spinning: Vec<&str> = ctx
            .facts
            .disks
            .iter()
            .filter(|d| d.rotational)
            .map(|d| d.name.as_str())
            .collect();
        if spinning.is_empty() {
            return Outcome::pass(
                format!("{} device(s), none spinning", ctx.facts.disks.len()),
                EXPECTED,
            )
            .why(WHY);
        }
        return Outcome::fail(
            format!("spinning disk(s): {}", spinning.join(", ")),
            EXPECTED,
        )
        .why(WHY);
    }

    per_path(ctx, EXPECTED, WHY, |m, label| {
        if NETWORK_FS.contains(&m.fstype.as_str()) {
            return Err(format!("{label} on {} ({})", m.fstype, m.source));
        }
        match base_device(&m.source).and_then(|d| ctx.facts.disks.iter().find(|x| x.name == d)) {
            Some(d) if d.rotational => Err(format!("{label} on spinning disk {}", d.name)),
            Some(d) => Ok(format!("{label} on {}", d.name)),
            None => Ok(format!("{label} on {}", m.source)),
        }
    })
}

/// PF-FS-0004. noatime.
pub fn noatime(ctx: &Ctx) -> Outcome {
    const WHY: &str = "Anza does not publish this one. It is common operator practice: without \
        noatime the kernel writes an access timestamp every time the validator reads a file, \
        which on an accounts database means a metadata write behind a large share of reads. It \
        costs write endurance for information nothing uses. Worth doing, not required.";
    const EXPECTED: &str = "noatime on the validator's filesystems";

    if let Some(o) = needs_linux(ctx, WHY) {
        return o;
    }
    per_path(ctx, EXPECTED, WHY, |m, named| {
        match m.options.split(',').any(|o| o == "noatime") {
            true => Ok(named.to_string()),
            // The mount carries the option, not the path, so name both.
            false => Err(format!("{named}, mounted at {}, has no noatime", m.target)),
        }
    })
}

/// PF-FS-0005. Filesystem type.
pub fn filesystem_type(ctx: &Ctx) -> Outcome {
    const WHY: &str = "ext4 and xfs are what validator operators run and what Anza's guidance \
        assumes. Others are not forbidden, but they are untested at this write volume and some, \
        like zfs and overlay, do not support the direct I/O agave uses to unpack snapshots.";
    const EXPECTED: &str = "ext4 or xfs";

    if let Some(o) = needs_linux(ctx, WHY) {
        return o;
    }
    per_path(ctx, EXPECTED, WHY, |m, label| match m.fstype.as_str() {
        "ext4" | "xfs" => Ok(format!("{label} {}", m.fstype)),
        other => Err(format!("{label} on {other}")),
    })
}

/// PF-FS-0006. O_DIRECT. The filesystem half of PF-ARG-0010.
pub fn direct_io_support(ctx: &Ctx) -> Outcome {
    const WHY: &str = "Since v4.0 agave unpacks snapshot archives with direct I/O to bypass the \
        page cache. A filesystem without O_DIRECT cannot serve that, and the failure appears \
        during a snapshot restore rather than at startup. The opt-out flag is checked separately \
        as PF-ARG-0010; this half checks whether you need it.";
    const EXPECTED: &str = "a filesystem supporting O_DIRECT, or the opt-out flag set";

    if let Some(o) = needs_linux(ctx, WHY) {
        return o;
    }
    let all = mounts(ctx);
    let paths = validator_paths(ctx);
    let target = paths
        .iter()
        .find(|(l, _)| *l == "accounts")
        .map(|(_, p)| p.clone());

    let Some(path) = target else {
        return Outcome::skipped("no accounts path to check yet");
    };
    let Some(m) = mount_for(&all, &path) else {
        return Outcome::unknown(format!("no mount found for {path}"))
            .expected(EXPECTED)
            .why(WHY);
    };
    let opted_out = ctx
        .inv()
        .is_some_and(|i| i.has("--no-accounts-db-snapshots-direct-io"));
    if !NO_ODIRECT.contains(&m.fstype.as_str()) {
        return Outcome::pass(
            format!("{} is {}, which supports O_DIRECT", m.target, m.fstype),
            EXPECTED,
        )
        .why(WHY);
    }
    if opted_out {
        return Outcome::pass(
            format!(
                "{} is {}, and --no-accounts-db-snapshots-direct-io is set",
                m.target, m.fstype
            ),
            EXPECTED,
        )
        .why(WHY);
    }
    Outcome::fail(
        format!(
            "{} is {}, which does not support O_DIRECT, and the opt-out is not set",
            m.target, m.fstype
        ),
        EXPECTED,
    )
    .why(WHY)
    .fix(vec![FixStep::noted(
        "--no-accounts-db-snapshots-direct-io",
        "or move the accounts path to ext4 or xfs, which is the better answer",
    )])
}

/// Agave's own sizing, from the comment block above DEFAULT_MAX_BLOCKSTORE_SHREDS.
const BYTES_PER_SHRED: f64 = 1250.0;
const DEFAULT_BLOCKSTORE_SHREDS: f64 = 400_000_000.0;
const LEGACY_DEFAULT_LEDGER_SHREDS: f64 = 200_000_000.0;

pub const S_RETENTION: &[Source] = &[Source {
    kind: AgaveSymbol,
    locator: "DEFAULT_MAX_BLOCKSTORE_SHREDS and the sizing comment above it in cleanup_service.rs",
    verified_against: "agave master",
    provisional: false,
}];

/// PF-FS-0007. Does the ledger's retention target fit the disk holding it?
///
/// Two checks each hold half of this. One knows free space, the other knows the
/// retention setting, and neither says the thing that matters: a blockstore
/// aimed at more disk than exists is a future outage with a stated mechanism.
pub fn retention_fits_disk(ctx: &Ctx) -> Outcome {
    const WHY: &str = "Agave sizes the blockstore in shreds and approximates a shred at 1250 \
        bytes, so a retention target converts to a disk footprint. What decides the question is \
        not the target but the distance left to it: a store at 475 GB of a 500 GB target needs \
        25 GB more, not 500. preflight measures the blockstore rather than asking you to, so the \
        numbers below account for the whole filesystem and sum.";
    const EXPECTED: &str = "room on the ledger's filesystem for the distance still to grow";

    if let Some(o) = needs_linux(ctx, WHY) {
        return o;
    }
    let Some(inv) = ctx.inv() else {
        return Outcome::skipped("no validator yet, so no retention target");
    };
    let Some(ledger) = inv.value("--ledger") else {
        return Outcome::skipped("no --ledger path in the invocation");
    };

    // Derive from the flag actually in use. Under the legacy flag the budget
    // counts data shreds only, so the footprint is an estimate at a 1:1 erasure
    // ratio rather than a bound, and it drifts up when the cluster gets strange.
    let (total_shreds, constant, bounded) = match (
        inv.value("--limit-blockstore-size"),
        inv.has("--limit-ledger-size"),
        inv.value("--limit-ledger-size"),
    ) {
        (Some(v), ..) => (
            v.parse::<f64>().unwrap_or(DEFAULT_BLOCKSTORE_SHREDS),
            "your --limit-blockstore-size value",
            true,
        ),
        (None, true, Some(v)) => (
            v.parse::<f64>().unwrap_or(LEGACY_DEFAULT_LEDGER_SHREDS) * 2.0,
            "your --limit-ledger-size value, doubled for coding shreds",
            false,
        ),
        (None, true, None) => (
            LEGACY_DEFAULT_LEDGER_SHREDS * 2.0,
            "LEGACY_DEFAULT_MAX_LEDGER_SHREDS, doubled for coding shreds",
            false,
        ),
        (None, false, _) => (
            DEFAULT_BLOCKSTORE_SHREDS,
            "DEFAULT_MAX_BLOCKSTORE_SHREDS",
            true,
        ),
    };
    let target_gb = total_shreds * BYTES_PER_SHRED / 1e9;
    let bound_note = match bounded {
        true => String::new(),
        false => format!(
            " That target comes from {constant}, which counts data shreds only, so it is an \
             estimate at a 1:1 erasure ratio and not a ceiling. Moving to \
             --limit-blockstore-size is what makes it one."
        ),
    };

    let all = mounts(ctx);
    let Some(m) = mount_for(&all, &ledger) else {
        return Outcome::unknown(format!("no mount found for {ledger}"))
            .expected(EXPECTED)
            .why(WHY);
    };
    let facts = ctx.facts.mounts.iter().find(|x| x.target == m.target);
    let (Some(free), Some(total)) = (
        facts.and_then(|x| x.free_gb),
        facts.and_then(|x| x.total_gb),
    ) else {
        return match ctx.fs.is_prefixed() {
            true => Outcome::skipped("free space cannot be read from a captured tree"),
            false => Outcome::unknown(format!("free space on {} not measured", m.target))
                .expected(EXPECTED)
                .why(WHY),
        };
    };

    let Some(store) = host::dir_size_gb(&ctx.fs, &format!("{ledger}/rocksdb")) else {
        return match ctx.fs.is_prefixed() {
            true => Outcome::skipped("directory sizes cannot be read from a captured tree"),
            false => Outcome::unknown(format!("{ledger}/rocksdb could not be measured"))
                .expected(EXPECTED)
                .why(format!(
                    "{WHY} Without the current size the distance to the target is unknowable, \
                     and free space alone would call a store near its cap a shortfall."
                ))
                .verify(format!("sudo du -sh {ledger}/rocksdb")),
        };
    };

    // Snapshots only enter the arithmetic when they share the filesystem, since
    // that is the only case where they compete for the same free space.
    let snaps = inv
        .value("--snapshots")
        .filter(|p| mount_for(&all, p).is_some_and(|sm| sm.target == m.target))
        .and_then(|p| host::dir_size_gb(&ctx.fs, &p));
    let other = (total - free - store - snaps.unwrap_or(0.0)).max(0.0);
    let ledger_line = format!(
        "{} holds {total:.0} GB: blockstore {store:.0}, {}other {other:.0}, free {free:.0}",
        m.target,
        match snaps {
            Some(v) => format!("snapshots {v:.0}, "),
            None => String::new(),
        }
    );
    let need = target_gb - store;
    let observed = format!("retention targets {target_gb:.0} GB from {constant}; {ledger_line}");

    match () {
        // Larger than the whole filesystem can never be met, whatever cleanup does.
        _ if target_gb > total => Outcome::fail(observed, format!("a target under {total:.0} GB"))
            .why(format!(
                "{WHY}{bound_note} This target is larger than the entire filesystem, so the node \
                 fills the disk on its way to a size it can never reach."
            ))
            .fix(retention_fix(ctx, target_gb, total, bounded))
            .verify(format!("du -sh {ledger}/rocksdb")),

        // The case one run genuinely can decide, and the one the operator was
        // previously left to work out: distance still to grow against room left.
        _ if need > free => Outcome::fail(
            format!("{observed}; {need:.0} GB still to grow, {free:.0} GB left"),
            EXPECTED,
        )
        .why(format!(
            "{WHY}{bound_note} The store has {need:.0} GB left to grow before cleanup holds it \
             flat, and only {free:.0} GB to grow into. It fills the disk first, weeks from now, \
             with nothing at the time to connect the failure to this setting."
        ))
        .fix(retention_fix(ctx, target_gb, free + store, bounded))
        .verify(format!("du -sh {ledger}/rocksdb")),

        // At or past the cap already: cleanup is what holds it, not free space.
        _ if need <= 0.0 => Outcome::pass(
            format!("{ledger_line}; already at the {target_gb:.0} GB target"),
            "cleanup holds the store flat from here, so free space stops falling to it",
        ),

        _ => Outcome::pass(
            format!("{ledger_line}; {need:.0} GB still to grow"),
            format!("{free:.0} GB free, which is more than the {need:.0} GB left to grow"),
        ),
    }
}

/// Round hard: the inputs are a 1250-byte approximation and an 80% rule of
/// thumb, and nine significant figures would imply a measurement nobody made.
fn sized_shreds(room_gb: f64) -> f64 {
    (room_gb * 0.8 * 1e9 / BYTES_PER_SHRED / 10e6).round() * 10e6
}

fn retention_fix(ctx: &Ctx, target_gb: f64, room_gb: f64, bounded: bool) -> Vec<FixStep> {
    let shreds = sized_shreds(room_gb);
    // Both this and PF-ARG-0011 edit the same line. Working the list top to
    // bottom must not leave an operator holding both flags at once.
    let lead = match bounded {
        true => "if you shrink it: ",
        false => "if you shrink it, on the flag PF-ARG-0011 renames to: ",
    };
    let mut steps = vec![FixStep::noted(
        format!("{lead}--limit-blockstore-size {shreds:.0}"),
        format!(
            "about 80% of the {room_gb:.0} GB free, against a {target_gb:.0} GB target. Subtract \
             what du reports before trusting it"
        ),
    )];

    // An idle device next door is the better answer than shrinking history.
    let idle: Vec<String> = ctx
        .facts
        .disks
        .iter()
        .filter(|d| !d.rotational)
        .filter(|d| {
            !ctx.facts
                .mounts
                .iter()
                .any(|m| m.free_gb.unwrap_or(0.0) < room_gb + 1.0 && m.target.contains(&d.name))
        })
        .map(|d| format!("{} holds {:.0} GB", d.name, d.size_gb))
        .collect();
    if !idle.is_empty() {
        steps.push(FixStep::noted(
            "or move the ledger or snapshots onto another device",
            format!(
                "this host has {}. Splitting them is what Anza's separate-device layout is for",
                idle.join(", ")
            ),
        ));
    }
    steps
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A store at 475 GB of a 500 GB target needs 25 GB more, not 500, so free
    /// space alone must never decide this. The only fail is a target larger than
    /// the whole filesystem, which no amount of cleanup can meet.
    #[test]
    fn sizing_is_rounded_to_what_the_inputs_support() {
        // 235 GB free once produced 150218434, which reads as a measurement.
        assert_eq!(sized_shreds(235.0), 150_000_000.0);
        assert_eq!(sized_shreds(905.0), 580_000_000.0);
        // The legacy flag counts data shreds only, so the footprint doubles.
        assert_eq!(
            LEGACY_DEFAULT_LEDGER_SHREDS * 2.0,
            DEFAULT_BLOCKSTORE_SHREDS
        );
    }
}
