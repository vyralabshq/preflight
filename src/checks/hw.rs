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
        sequential hash chain, so it is bound by single-core speed rather than core count.";
    const EXPECTED: &str = "2.8 GHz or faster";

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
    let mhz = info
        .lines()
        .find(|l| l.starts_with("cpu MHz"))
        .and_then(|l| l.split(':').nth(1))
        .and_then(|v| v.trim().parse::<f64>().ok());
    // Anza states 2.8 GHz; below that is a finding, not a preference.
    match mhz {
        None => Outcome::unknown(format!("{model}: no cpu MHz reported")).why(WHY),
        Some(m) if m >= want => {
            Outcome::pass(format!("{model} at {:.0} MHz", m), EXPECTED).why(WHY)
        }
        Some(m) => Outcome::fail(format!("{model} at {:.0} MHz", m),
            EXPECTED,
        )
        .why(WHY)
        .fix(vec![FixStep::noted(
            "check the BIOS for a power or efficiency profile capping the clock",
            "a reported clock below base often means a governor or firmware cap, not the silicon",
        )]),
    }
}

/// PF-HW-0004. Cores. Reported, never failed: Anza publishes no minimum.
pub fn cores(ctx: &Ctx) -> Outcome {
    const WHY: &str = "Core count is not the metric, and the community hardware list shows why: \
        a 16 core Ryzen 9950X reaches about 23M PoH hashes per second, while a 32 core EPYC 9354P \
        reaches 14M to 16M. Both are on the recommended list. Proof of History is a sequential \
        chain, so single core speed decides whether you keep up. Anza publishes no minimum and \
        preflight will not invent one.";
    const EXPECTED: &str = "no published minimum; 16 core parts are on the recommended list";

    if let Some(o) = needs_linux(ctx, WHY) {
        return o;
    }
    let Some(info) = cpuinfo(ctx) else {
        return Outcome::unknown("cannot read /proc/cpuinfo").why(WHY);
    };
    let threads = info.lines().filter(|l| l.starts_with("processor")).count();
    let physical = info
        .lines()
        .find(|l| l.starts_with("cpu cores"))
        .and_then(|l| l.split(':').nth(1))
        .and_then(|v| v.trim().parse::<usize>().ok());
    let observed = match physical {
        Some(p) => format!("{p} physical cores, {threads} threads"),
        None => format!("{threads} threads, physical core count not reported"),
    };
    Outcome::reported(observed, EXPECTED).why(WHY)
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
