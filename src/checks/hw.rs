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
    let mhz = info
        .lines()
        .find(|l| l.starts_with("cpu MHz"))
        .and_then(|l| l.split(':').nth(1))
        .and_then(|v| v.trim().parse::<f64>().ok());
    // Anza states 2.8 GHz; below that is a finding, not a preference.
    match mhz {
        None => Outcome::unknown(format!("{model}: no cpu MHz reported")).why(WHY),
        Some(m) if m >= 2800.0 => {
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
    const WHY: &str = "Anza's requirements page gives no core-count minimum. It states only that \
        higher clock speed is preferable to more cores. preflight reports what this machine has \
        and refuses to invent a threshold, because a made-up number would either pass a box that \
        cannot keep up or fail one that can.";
    const EXPECTED: &str = "no published minimum; reported for your judgement";

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
    Outcome::unknown(observed).expected(EXPECTED).why(WHY)
}

/// PF-HW-0005. RAM. Anza suggests a 512 GB-capable board, which is guidance
/// rather than a floor, so this reports and warns instead of failing.
pub fn memory(ctx: &Ctx) -> Outcome {
    const WHY: &str = "Anza suggests a motherboard with 512 GB capacity and ECC memory. That is \
        a suggestion about the board, not a stated minimum for the process, so preflight reports \
        what is installed rather than failing a box against a number Anza did not publish. \
        Accounts and index live in memory; running short shows up as an OOM kill hours into a \
        run, not at startup.";
    const EXPECTED: &str = "512 GB board capacity suggested by Anza; no hard minimum published";

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

    // No published minimum, so report unless the number is obviously unworkable.
    match (ctx.profile, gb) {
        (Profile::Local, _) => Outcome::pass(observed, "enough for a test validator").why(WHY),
        (_, g) if g < 128.0 => {
            Outcome::fail(observed, EXPECTED)
                .why(WHY)
                .fix(vec![FixStep::noted(
                    "add memory before joining a cluster",
                    "well under any published guidance; the node will be OOM-killed under load",
                )])
        }
        _ => Outcome::unknown(observed).expected(EXPECTED).why(WHY),
    }
}
