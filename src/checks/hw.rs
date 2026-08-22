//! HW layer. Can this machine physically run a validator?
//!
//! Nothing here reads a validator. These checks answer the question a bare box
//! asks: is this hardware capable at all, and if not, is that fixable or is it
//! a wall. Thresholds come from Anza's requirements page; where Anza publishes
//! no minimum, the check reports Unknown and says so rather than inventing one.

use crate::{
    checks::needs_linux,
    ctx::Ctx,
    model::{FixStep, Outcome, Profile, Source, SourceKind::*},
};

pub const S_REQ: &[Source] = &[Source {
    kind: AnzaDocs,
    locator: "docs.anza.xyz/operations/requirements",
    verified_against: "2026-08",
    provisional: false,
}];

/// The community hardware list, which records what operators actually run and
/// what PoH rate each part reaches. Anza publishes no core or memory minimum,
/// so this is the closest thing to evidence.
/// Figures nobody publishes, carried as what they are: an operator floor from
/// running these clusters, never dressed up as Anza's.
pub const S_FLOOR: &[Source] = &[
    Source {
        kind: Operator,
        locator: "operator floor, not published by Anza",
        verified_against: "2026-08",
        provisional: false,
    },
    Source {
        kind: Operator,
        locator: "solanahcl.org, agave CPU list",
        verified_against: "2026-08",
        provisional: false,
    },
];

pub const S_HCL: &[Source] = &[Source {
    kind: Operator,
    locator: "solanahcl.org, agave CPU list",
    verified_against: "2026-08",
    provisional: false,
}];

fn cpuinfo(ctx: &Ctx) -> Option<String> {
    ctx.fs.read("/proc/cpuinfo").ok()
}

/// PF-HW-0001. Architecture. Unsupported rather than Fail: no command fixes it.
pub fn architecture(ctx: &Ctx) -> Outcome {
    const WHY: &str = "Anza publishes validator binaries for x86-64 only, and the client requires \
        instructions that other architectures do not have. This is not a configuration problem \
        and no package installs a fix; it is a different machine or nothing.";
    const EXPECTED: &str = "x86-64";

    let arch = match &ctx.arch {
        Some(a) => a.clone(),
        None => return Outcome::unknown("architecture not determined").why(WHY),
    };
    if arch == "x86_64" {
        return Outcome::pass(arch, EXPECTED).why(WHY);
    }
    Outcome::unsupported(
        format!("{arch}, and the host is {}", host_os(ctx)),
        EXPECTED,
    )
    .why(WHY)
}

fn host_os(ctx: &Ctx) -> String {
    match (&ctx.os, ctx.is_linux()) {
        (Some(os), _) => os.clone(),
        (None, true) => "an unidentified Linux".to_string(),
        (None, false) => format!("the host runs {}, not Linux", std::env::consts::OS),
    }
}

/// PF-HW-0002. AVX2. Also Unsupported when absent: it is a property of the die.
pub fn avx2(ctx: &Ctx) -> Outcome {
    const WHY: &str = "The official validator binaries are built assuming AVX2. Without it the \
        process will not run, and there is no build flag an operator can pass to change that on \
        a released binary.";
    const EXPECTED: &str = "avx2 in the CPU flags";

    if let Some(o) = needs_linux(ctx, WHY) {
        return o;
    }
    let Some(info) = cpuinfo(ctx) else {
        return Outcome::unknown("cannot read /proc/cpuinfo").why(WHY);
    };
    let flags = info
        .lines()
        .find(|l| l.starts_with("flags") || l.starts_with("Features"))
        .unwrap_or("");
    if flags.split_whitespace().any(|f| f == "avx2") {
        return Outcome::pass("avx2 present", EXPECTED).why(WHY);
    }
    Outcome::unsupported("avx2 absent from the CPU flags", EXPECTED).why(WHY)
}

/// PF-HW-0003. Base clock. Anza states 2.8 GHz, and prefers clock over cores.
pub fn base_clock(ctx: &Ctx) -> Outcome {
    const WHY: &str = "Anza's stated requirement is a 2.8 GHz base clock or faster, and the docs \
        say plainly that higher clock speed is preferable to more cores. Proof of History is a \
        sequential hash chain, so it is bound by single-core speed rather than core count. \
        /proc/cpuinfo reports what the governor is doing right now, not the base clock, so it \
        reads high on a busy core and low on an idle one. Neither is the number Anza means.";
    const EXPECTED: &str = "2.8 GHz base clock or faster";

    if let Some(o) = needs_linux(ctx, WHY) {
        return o;
    }
    let Some(info) = cpuinfo(ctx) else {
        return Outcome::unknown("cannot read /proc/cpuinfo").why(WHY);
    };
    let model = info
        .lines()
        .find(|l| l.starts_with("model name"))
        .and_then(|l| l.split(':').nth(1))
        .map(str::trim)
        .unwrap_or("unknown CPU");
    let Some(want) = ctx.profile.thresholds().base_clock_mhz else {
        return Outcome::skipped("no clock requirement for this profile");
    };

    // cpufreq publishes the real thing. Fall back to the governor's current
    // reading only to say so, never to fail a machine on it.
    let khz = |p: &str| {
        ctx.fs
            .read(p)
            .ok()
            .and_then(|v| v.trim().parse::<f64>().ok())
            .map(|k| k / 1000.0)
    };
    let base = khz("/sys/devices/system/cpu/cpu0/cpufreq/base_frequency")
        .or_else(|| khz("/sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq"));
    if let Some(m) = base {
        let observed = format!("{model} at {m:.0} MHz base");
        return match m >= want {
            true => Outcome::pass(observed, EXPECTED).why(WHY),
            false => Outcome::fail(observed, EXPECTED)
                .why(WHY)
                .fix(vec![FixStep::noted(
                    "check the BIOS for a power or efficiency profile capping the clock",
                    "a base below Anza's figure is usually firmware, not the silicon",
                )]),
        };
    }

    let current = info
        .lines()
        .find(|l| l.starts_with("cpu MHz"))
        .and_then(|l| l.split(':').nth(1))
        .and_then(|v| v.trim().parse::<f64>().ok());
    match current {
        None => Outcome::unknown(format!("{model}: no clock reported")).why(WHY),
        // Above the bar on a governor reading still clears the bar.
        Some(m) if m >= want => Outcome::pass(
            format!("{model} at {m:.0} MHz current, base not published by this kernel"),
            EXPECTED,
        )
        .why(WHY),
        // Below it proves nothing: an idle core throttles well under base.
        Some(m) => Outcome::unknown(format!(
            "{model} at {m:.0} MHz current, which is the governor's reading and not the base clock"
        ))
        .expected(EXPECTED)
        .why(WHY)
        .verify("lscpu | grep -i 'model name\\|CPU max MHz'"),
    }
}

/// PF-HW-0004. Cores. Reported, never failed: Anza publishes no minimum.
pub fn cores(ctx: &Ctx) -> Outcome {
    const WHY: &str = "Anza publishes no core count for validators; the 12 cores and 24 threads \
        in its table are the RPC column. Clock still dominates, and the community list shows why: \
        a 16 core Ryzen 9950X reaches about 23M PoH hashes per second while a 32 core EPYC 9354P \
        reaches 14M to 16M. But cores decide how much else fits beside the PoH thread, so the \
        figures below are an operator floor from running these clusters rather than anything Anza \
        states: 16 cores carries testnet, and mainnet wants 24.";

    if let Some(o) = needs_linux(ctx, WHY) {
        return o;
    }
    let Some(info) = cpuinfo(ctx) else {
        return Outcome::unknown("cannot read /proc/cpuinfo").why(WHY);
    };
    let threads = info.lines().filter(|l| l.starts_with("processor")).count();
    let physical = physical_cores(&info);
    let observed = match physical {
        Some(p) => format!("{p} physical cores, {threads} threads"),
        None => format!("{threads} threads, physical core count not reported"),
    };
    let Some(want) = ctx.profile.thresholds().cores else {
        return Outcome::reported(observed, "no floor on this profile").why(WHY);
    };
    let expected = format!("{want} physical cores for {}", ctx.profile.label());
    match physical {
        None => Outcome::reported(observed, expected).why(WHY),
        Some(p) if p >= want => Outcome::pass(observed, expected).why(WHY),
        Some(_) => Outcome::fail(observed, expected)
            .why(WHY)
            .fix(vec![FixStep::noted(
                format!("run this one on a box with {want} cores or more"),
                "nothing on this machine changes its core count, so this is a hardware decision",
            )]),
    }
}

/// cpuinfo's "cpu cores" is per socket, so a dual socket box under-counts.
/// Unique (physical id, core id) pairs are the real number.
fn physical_cores(info: &str) -> Option<usize> {
    let mut seen = std::collections::BTreeSet::new();
    let (mut socket, mut core) = (None, None);
    for line in info.lines() {
        let value = || {
            line.split(':')
                .nth(1)
                .and_then(|v| v.trim().parse::<u32>().ok())
        };
        match line {
            _ if line.starts_with("physical id") => socket = value(),
            _ if line.starts_with("core id") => core = value(),
            _ => continue,
        }
        if let (Some(s), Some(c)) = (socket, core) {
            seen.insert((s, c));
            (socket, core) = (None, None);
        }
    }
    match seen.is_empty() {
        false => Some(seen.len()),
        // Single socket kernels may omit the topology lines entirely.
        true => info
            .lines()
            .find(|l| l.starts_with("cpu cores"))
            .and_then(|l| l.split(':').nth(1))
            .and_then(|v| v.trim().parse().ok()),
    }
}

/// PF-HW-0005. RAM. Anza suggests a 512 GB-capable board, which is guidance
/// rather than a floor, so this reports and warns instead of failing.
pub fn memory(ctx: &Ctx) -> Outcome {
    const WHY: &str = "Anza suggests a board with 512 GB capacity and ECC memory, without saying \
        which cluster that is for, and publishes no minimum. Neither does the community hardware \
        list. Operators run testnet on far less. Accounts and index live in memory, so running \
        short shows up as an OOM kill hours into a run rather than at startup, which is why the \
        figure is worth seeing even though nobody will tell you what it should be.";
    const EXPECTED: &str = "no published minimum for any cluster";

    if let Some(o) = needs_linux(ctx, WHY) {
        return o;
    }
    let Ok(info) = ctx.fs.read("/proc/meminfo") else {
        return Outcome::unknown("cannot read /proc/meminfo").why(WHY);
    };
    let kb = info
        .lines()
        .find(|l| l.starts_with("MemTotal:"))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|v| v.parse::<u64>().ok());
    let Some(kb) = kb else {
        return Outcome::unknown("MemTotal not found in /proc/meminfo").why(WHY);
    };
    let gb = kb as f64 / 1024.0 / 1024.0;
    let observed = format!("{gb:.1} GB installed");

    // Anza publishes no minimum, so this reports and does not judge. An earlier
    // version failed anything under 128 GB, a number nobody published, and it
    // failed a working validator.
    match ctx.profile {
        Profile::Local => Outcome::pass(observed, "enough for a test validator").why(WHY),
        _ => Outcome::reported(observed, EXPECTED).why(WHY),
    }
}

/// CPUs the community list marks recommended, with base clock and the PoH rate
/// operators have reported. Clock and PoH are what decide whether you keep up,
/// which is why a 16 core part sits alongside a 64 core one here.
const RECOMMENDED: &[(&str, &str, &str)] = &[
    ("Ryzen Threadripper PRO 7965WX", "4.20 GHz", "22.2M, 20.4M"),
    ("Ryzen Threadripper PRO 7975WX", "4.00 GHz", "not reported"),
    ("Ryzen Threadripper PRO 7985WX", "3.20 GHz", "not reported"),
    ("Ryzen Threadripper 7960X", "4.20 GHz", "20.6M, 19.9M"),
    ("Ryzen 9 7950X", "4.50 GHz", "22.4M"),
    ("Ryzen 9 9950X", "4.30 GHz", "23M"),
    ("EPYC 9274F", "4.05 GHz", "18.1M"),
    ("EPYC 9275F", "4.10 GHz", "19.3M"),
    ("EPYC 9374F", "3.85 GHz", "18.2M"),
    ("EPYC 9375F", "3.80 GHz", "18.9M-19.3M"),
    ("EPYC 9254", "2.90 GHz", "17.5M"),
    ("EPYC 9354P", "3.25 GHz", "16.1M, 14.4M"),
];

/// PF-HW-0006. Whether anyone has reported this CPU keeping up.
///
/// Advisory on testnet, where operators run far more varied hardware. On
/// mainnet a part nobody has reported on is worth knowing about before you take
/// stake, since the cost of finding out is missed blocks.
pub fn on_recommended_list(ctx: &Ctx) -> Outcome {
    const WHY: &str = "The community list records CPUs operators have run and the PoH hash rate \
        each reached. Being absent is not a failure: nobody has reported on it, which is a \
        different thing from it being unsuitable. It does mean you are the first to find out, and \
        on mainnet that is discovered as missed blocks.";
    const EXPECTED: &str = "a CPU somebody has reported PoH numbers for";

    if let Some(o) = needs_linux(ctx, WHY) {
        return o;
    }
    let Some(model) = ctx.facts.cpu_model.clone() else {
        return Outcome::unknown("CPU model not reported")
            .expected(EXPECTED)
            .why(WHY);
    };

    // Model strings carry suffixes like "16-Core Processor" that the list omits.
    let listed = RECOMMENDED.iter().find(|(name, _, _)| model.contains(name));

    match (listed, ctx.profile) {
        (Some((name, clock, poh)), _) => Outcome::pass(
            format!("{name} is on the list, base {clock}, reported PoH {poh}"),
            EXPECTED,
        )
        .why(WHY),
        (None, Profile::Mainnet) => Outcome::fail(format!("{model} is not on the list"), EXPECTED)
            .why(WHY)
            .fix(vec![FixStep::noted(
                "measure your PoH rate before taking stake, and compare against the list",
                "the listed parts report roughly 14M to 23M hashes per second",
            )]),
        (None, _) => Outcome::reported(format!("{model} is not on the list"), EXPECTED).why(WHY),
    }
}

/// Ubuntu LTS standard support end dates, as published on the release cycle
/// page. Only the releases a validator is plausibly running.
const UBUNTU_EOL: &[(&str, u32, u32)] = &[
    ("18.04", 2023, 5),
    ("20.04", 2025, 5),
    ("22.04", 2027, 6),
    ("24.04", 2029, 6),
];

pub const S_RELEASE: &[Source] = &[Source {
    kind: Operator,
    locator: "ubuntu.com/about/release-cycle",
    verified_against: "2026-08",
    provisional: false,
}];

/// PF-HW-0007. Whether the distribution still gets kernels.
///
/// Sits here rather than with the kernel values because it explains them: an
/// old release is usually why a kernel is old, and why moving off it is a
/// bigger job than an upgrade command.
pub fn os_support(ctx: &Ctx) -> Outcome {
    const WHY: &str = "A release past standard support stops getting kernel updates, which is \
        usually why a validator host is several kernel versions behind and why catching up means \
        a release upgrade rather than apt. Worth knowing before you plan the work, not a reason \
        the node will stop.";
    const EXPECTED: &str = "a release still in standard support";

    if let Some(o) = needs_linux(ctx, WHY) {
        return o;
    }
    let Ok(text) = ctx.fs.read("/etc/os-release") else {
        return Outcome::unknown("cannot read /etc/os-release").why(WHY);
    };
    let field = |k: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(k))
            .map(|v| v.trim_matches('"').to_string())
    };
    let (Some(id), Some(version)) = (field("ID="), field("VERSION_ID=")) else {
        return Outcome::unknown("no release id in /etc/os-release").why(WHY);
    };
    let pretty = ctx.os.clone().unwrap_or_else(|| format!("{id} {version}"));

    if id != "ubuntu" {
        return Outcome::reported(
            format!("{pretty}, no support dates recorded here"),
            EXPECTED,
        )
        .why(WHY);
    }
    let Some((_, year, month)) = UBUNTU_EOL.iter().find(|(v, _, _)| *v == version) else {
        return Outcome::reported(format!("{pretty}, not a release listed here"), EXPECTED)
            .why(WHY);
    };

    let now = crate::host::now_utc();
    let (this_year, this_month) = (
        now[..4].parse::<u32>().unwrap_or(*year),
        now[5..7].parse::<u32>().unwrap_or(*month),
    );
    match (this_year, this_month) < (*year, *month) {
        true => Outcome::pass(
            format!("{pretty}, supported until {year}-{month:02}"),
            EXPECTED,
        )
        .why(WHY),
        false => Outcome::fail(
            format!("{pretty}, standard support ended {year}-{month:02}"),
            EXPECTED,
        )
        .why(WHY)
        .fix(vec![FixStep::noted(
            "plan a release upgrade, or move to a host on a supported release",
            "extended maintenance may still deliver security fixes, but not new kernels",
        )]),
    }
}
