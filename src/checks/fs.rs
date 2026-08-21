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
pub const S_DIO: &[Source] = &[Source {
    kind: AgaveChangelog,
    locator: "v4.0 Validator/Changes",
    verified_against: "v4.2.1",
    provisional: false,
}];

/// Anza's stated sizes: accounts 1 TB, ledger 1 TB, snapshots 500 GB.
const NEED_ACCOUNTS_GB: f64 = 1000.0;
const NEED_LEDGER_GB: f64 = 1000.0;
const NEED_SNAPSHOTS_GB: f64 = 500.0;
const NEED_TOTAL_GB: f64 = NEED_ACCOUNTS_GB + NEED_LEDGER_GB + NEED_SNAPSHOTS_GB;

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
        .filter_map(|(label, path)| mount_for(&all, path).map(|m| test(m, label)))
        .partition(Result::is_ok);

    match bad.is_empty() {
        true => Outcome::pass(unwrap_all(good).join(", "), expected).why(why),
        false => Outcome::fail(unwrap_all(bad).join("; "), expected).why(why),
    }
}

fn unwrap_all(v: Vec<Result<String, String>>) -> Vec<String> {
    v.into_iter().map(|r| r.unwrap_or_else(|e| e)).collect()
}

/// PF-FS-0001. Enough storage, and where it has to go.
pub fn capacity(ctx: &Ctx) -> Outcome {
    const WHY: &str = "Anza specifies three NVMe devices: accounts at 1 TB or larger, ledger at \
        1 TB or larger, and snapshots at 500 GB or larger, all with high write endurance. That is \
        about 2.5 TB in total. Running short does not fail at startup; it fails partway through a \
        snapshot download or weeks later when the ledger grows into the space you did not have.";
    let expected = format!("about {NEED_TOTAL_GB:.0} GB across the validator's paths");

    if let Some(o) = needs_linux(ctx, WHY) {
        return o;
    }
    let all = mounts(ctx);
    let paths = validator_paths(ctx);

    if paths.is_empty() {
        // No validator yet: judge the machine on the storage it has.
        let usable: f64 = ctx
            .facts
            .disks
            .iter()
            .filter(|d| !d.rotational)
            .map(|d| d.size_gb)
            .sum();
        let spinning: f64 = ctx
            .facts
            .disks
            .iter()
            .filter(|d| d.rotational)
            .map(|d| d.size_gb)
            .sum();
        if ctx.facts.disks.is_empty() {
            return Outcome::unknown("no block devices detected")
                .expected(expected)
                .why(WHY);
        }
        let observed = format!(
            "{:.0} GB across {} solid-state device(s){}",
            usable,
            ctx.facts.disks.iter().filter(|d| !d.rotational).count(),
            if spinning > 0.0 {
                format!(", plus {spinning:.0} GB spinning")
            } else {
                String::new()
            }
        );
        if usable >= NEED_TOTAL_GB {
            return Outcome::pass(observed, expected).why(WHY);
        }
        return Outcome::fail(observed, expected).why(WHY).fix(vec![FixStep::noted(
            format!(
                "add storage: accounts {NEED_ACCOUNTS_GB:.0} GB, ledger {NEED_LEDGER_GB:.0} GB, snapshots {NEED_SNAPSHOTS_GB:.0} GB"
            ),
            "high write endurance matters as much as size; validators write constantly",
        )]);
    }

    let mut short = Vec::new();
    let mut seen = Vec::new();
    for (label, path) in &paths {
        let need = match *label {
            "accounts" => NEED_ACCOUNTS_GB,
            "ledger" => NEED_LEDGER_GB,
            _ => NEED_SNAPSHOTS_GB,
        };
        let Some(m) = mount_for(&all, path) else {
            short.push(format!("{label} {path}: no mount found"));
            continue;
        };
        let free = ctx
            .facts
            .mounts
            .iter()
            .find(|x| x.target == m.target)
            .and_then(|x| x.free_gb);
        match free {
            Some(g) if g < need => short.push(format!(
                "{label} {path}: {g:.0} GB free, wants {need:.0} GB"
            )),
            Some(g) => seen.push(format!("{label} {g:.0} GB free")),
            None => seen.push(format!("{label} free space not measured")),
        }
    }
    if short.is_empty() {
        return Outcome::pass(seen.join(", "), expected).why(WHY);
    }
    Outcome::fail(short.join("; "), expected).why(WHY)
}

/// PF-FS-0002. Separate devices. Degraded, never Fatal: Anza permits sharing.
pub fn separate_devices(ctx: &Ctx) -> Outcome {
    const WHY: &str = "Anza's requirements say accounts and ledger can live on the same disk but \
        that it is not recommended, because both are IOPS-heavy and they contend. This is a \
        recommendation, not a rule: a node with shared storage runs, it just has less headroom \
        when the cluster gets busy.";
    const EXPECTED: &str = "accounts, ledger and snapshots on separate block devices";

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
    let mut shared = Vec::new();
    for i in 0..devices.len() {
        for j in i + 1..devices.len() {
            if devices[i].1 == devices[j].1 {
                shared.push(format!(
                    "{} and {} both on {}",
                    devices[i].0, devices[j].0, devices[i].1
                ));
            }
        }
    }
    let listing: Vec<String> = devices.iter().map(|(l, d)| format!("{l} on {d}")).collect();
    if shared.is_empty() {
        return Outcome::pass(listing.join(", "), EXPECTED).why(WHY);
    }
    Outcome::fail(shared.join("; "), EXPECTED)
        .why(WHY)
        .fix(vec![FixStep::noted(
            "move one of them to its own device",
            "if that is not possible, the node still runs; expect less headroom under load",
        )])
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
    const WHY: &str = "Without noatime the kernel writes an access timestamp every time the \
        validator reads a file, which on an accounts database means a metadata write behind a \
        large share of reads. It costs write endurance for information nothing uses.";
    const EXPECTED: &str = "noatime on the validator's filesystems";

    if let Some(o) = needs_linux(ctx, WHY) {
        return o;
    }
    per_path(ctx, EXPECTED, WHY, |m, label| {
        match m.options.split(',').any(|o| o == "noatime") {
            true => Ok(format!("{label} at {}", m.target)),
            false => Err(format!("{label} at {} has no noatime", m.target)),
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
