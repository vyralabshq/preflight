//! FS layer. Disks and filesystems.
//!
//! The only layer that answers "can this machine run a validator" in full
//! before anything is installed. With a validator present it checks the paths
//! that validator actually uses; without one it asks whether this box has
//! enough suitable storage to be worth installing on at all.

use crate::{
    checks::needs_linux,
    ctx::Ctx,
    model::{FixStep, Outcome, Source, SourceKind::*},
};

pub const S_REQ: &[Source] = &[Source {
    kind: AnzaDocs,
    locator: "docs.anza.xyz/operations/requirements, Disk Storage",
    verified_against: "2026-08",
    provisional: false,
}];

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

struct MountInfo {
    target: String,
    source: String,
    fstype: String,
    options: String,
}

fn mounts(ctx: &Ctx) -> Vec<MountInfo> {
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
fn mount_for<'a>(all: &'a [MountInfo], path: &str) -> Option<&'a MountInfo> {
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
    const WHY_HEADROOM: &str = "Nobody publishes storage figures for this cluster, and operators \
        run it on far less than Anza's production numbers, so preflight does not judge you \
        against a size. What does take a node down is running out: partway through a snapshot \
        download, or weeks later as the ledger grows. So this checks headroom instead.";

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
        "ledger" => t.ledger_gb,
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
        match want(label) {
            Some(need) if free < need => short.push(format!(
                "{label} {path}: {free:.0} GB free, wants {need:.0} GB"
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
    let expected = match t.accounts_gb {
        Some(_) => format!("about {NEED_TOTAL_GB:.0} GB across the validator's paths"),
        None => format!("at least {:.0}% free on each path", t.min_free * 100.0),
    };
    if short.is_empty() {
        return Outcome::pass(seen.join(", "), expected).why(why);
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
