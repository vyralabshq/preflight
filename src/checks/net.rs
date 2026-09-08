//! NET layer. The network card and its driver.
//!
//! Whether AF_XDP works, and whether zero copy works, is decided by the driver
//! more than anything else. The community hardware list records which families
//! operators have actually got working, which is the only evidence there is.

use crate::{
    checks::needs_linux,
    ctx::Ctx,
    model::{FixStep, Outcome, Source, SourceKind::*},
};

/// Vendor guides, not Anza. Anza publishes nothing on interrupt affinity.
pub const S_IRQ: &[Source] = &[
    Source {
        kind: Operator,
        locator: "AMD EPYC Linux Network Tuning Guide, IRQ affinity",
        verified_against: "2026-09",
        provisional: false,
    },
    Source {
        kind: Operator,
        locator: "Intel Ethernet 800 Series perf tuning, IRQ affinity",
        verified_against: "2026-09",
        provisional: false,
    },
];

pub const S_HCL: &[Source] = &[Source {
    kind: Operator,
    locator: "solanahcl.org, network card list",
    verified_against: "2026-08",
    provisional: false,
}];

/// How a driver behaves under AF_XDP, from the community list.
struct DriverSupport {
    driver: &'static str,
    family: &'static str,
    plain: Xdp,
    zero_copy: Xdp,
    note: &'static str,
}

#[derive(PartialEq)]
enum Xdp {
    Works,
    Caveat,
    Unstable,
    No,
}

const DRIVERS: &[DriverSupport] = &[
    DriverSupport {
        driver: "mlx5_core",
        family: "NVIDIA/Mellanox ConnectX-5 or ConnectX-6 Lx",
        plain: Xdp::Works,
        zero_copy: Xdp::Works,
        note: "The highest confidence family on the list. Works with zero copy on kernel 6.8.",
    },
    DriverSupport {
        driver: "i40e",
        family: "Intel 700 series",
        plain: Xdp::Works,
        zero_copy: Xdp::Works,
        note: "Reported working with zero copy on kernel 6.8.",
    },
    DriverSupport {
        driver: "ice",
        family: "Intel E800 series",
        plain: Xdp::Works,
        // Anza's guide names ice alongside bnxt_en as a driver not to pass
        // zero copy with, whatever the community table reports.
        zero_copy: Xdp::No,
        note: "Anza's XDP guide: do not pass --xdp-zero-copy with ice (or bnxt_en). Plain XDP \
               still works. The community table lists zero copy; the guide wins.",
    },
    DriverSupport {
        driver: "igb",
        family: "Intel I210",
        plain: Xdp::Works,
        zero_copy: Xdp::Caveat,
        note: "Zero copy needs kernel 6.14 or newer. One operator saw severe degradation and high \
               skips on 6.17 with zero copy on, and fell back to plain XDP.",
    },
    DriverSupport {
        driver: "ixgbe",
        family: "Intel X540 or X550",
        plain: Xdp::Works,
        zero_copy: Xdp::Unstable,
        note: "Zero copy is mixed and unstable here. Guidance for freezes and link flaps is to \
               start without it.",
    },
    DriverSupport {
        driver: "bnxt_en",
        family: "Broadcom",
        plain: Xdp::Works,
        zero_copy: Xdp::No,
        note: "Works with XDP but never accepts the zero copy flag. Non zero copy is still \
               reasonably fast. Prefer a different card when you can.",
    },
    DriverSupport {
        driver: "tg3",
        family: "Broadcom BCM5720",
        plain: Xdp::No,
        zero_copy: Xdp::No,
        note: "No native XDP and no zero copy. Treated as unsupported for validator work.",
    },
    DriverSupport {
        driver: "r8169",
        family: "Realtek",
        plain: Xdp::No,
        zero_copy: Xdp::No,
        note: "No native XDP and no zero copy. Treated as unsupported for validator work.",
    },
    DriverSupport {
        driver: "mlx4_en",
        family: "NVIDIA/Mellanox ConnectX-3",
        plain: Xdp::No,
        zero_copy: Xdp::No,
        note: "The driver is no longer supported and zero copy does not work. Do not use.",
    },
];

/// The default route's interface, which is the one the validator gossips over.
pub fn primary_interface(ctx: &Ctx) -> Option<String> {
    let routes = ctx.fs.read("/proc/net/route").ok()?;
    routes
        .lines()
        .skip(1)
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            (f.len() > 2 && f[1] == "00000000").then(|| f[0].to_string())
        })
        .next()
}

/// The driver name, read from the sysfs symlink rather than by running ethtool.
pub fn driver_of(ctx: &Ctx, iface: &str) -> Option<String> {
    let link = ctx.fs.at(format!("/sys/class/net/{iface}/device/driver"));
    let target = std::fs::read_link(&link).ok()?;
    Some(target.file_name()?.to_string_lossy().to_string())
}

/// PF-NET-0001. Whether this card can carry the XDP transmit path.
pub fn xdp_driver_support(ctx: &Ctx) -> Outcome {
    const WHY: &str = "Since v4.2 agave sends over XDP by default on Linux, and whether that \
        works at all, and whether zero copy works, is decided by the network driver. Anza does \
        not publish a compatibility list. The community one records what operators have actually \
        got running, including which cards silently fall back to a slow path.";
    const EXPECTED: &str = "a driver that carries AF_XDP";

    if let Some(o) = needs_linux(ctx, WHY) {
        return o;
    }
    let Some(iface) = primary_interface(ctx) else {
        return Outcome::unknown("no default route, so no interface to check")
            .expected(EXPECTED)
            .why(WHY);
    };
    let Some(driver) = driver_of(ctx, &iface) else {
        return Outcome::unknown(format!("cannot read the driver for {iface}"))
            .expected(EXPECTED)
            .why(WHY);
    };
    let Some(d) = DRIVERS.iter().find(|d| d.driver == driver) else {
        return Outcome::unknown(format!("{iface} uses {driver}, which is not on the list"))
            .expected(EXPECTED)
            .why(WHY)
            .fix(vec![FixStep::noted(
                "check solanahcl.org, or report what you find",
                "an absent driver means nobody has reported on it, not that it fails",
            )]);
    };

    let zero_copy_wanted = ctx.inv().is_some_and(|i| {
        i.has("--xdp-zero-copy") || i.has("--experimental-retransmit-xdp-zero-copy")
    });
    let observed = format!("{iface} uses {} ({})", d.driver, d.family);
    let why = format!("{WHY} {}", d.note);

    match (&d.plain, &d.zero_copy, zero_copy_wanted) {
        (Xdp::No, ..) => Outcome::fail(observed, "a driver with native XDP support")
            .why(why)
            .fix(vec![FixStep::noted(
                "use --no-xdp, or fit a card on the supported list",
                "without native XDP the node falls back to a slower path rather than failing",
            )]),
        (_, Xdp::No | Xdp::Unstable, true) => Outcome::fail(
            format!("{observed}, and --xdp-zero-copy is set"),
            "zero copy off on this driver",
        )
        .why(why)
        .fix(vec![FixStep::cmd("remove --xdp-zero-copy")]),
        (_, Xdp::Caveat, true) => Outcome::fail(
            format!("{observed}, and --xdp-zero-copy is set"),
            "kernel 6.14 or newer for zero copy on this driver",
        )
        .why(why),
        _ => Outcome::pass(observed, EXPECTED).why(why),
    }
}

/// This card's queue interrupts, as (name, cpu). Matched by name because irq
/// numbers move across reboots.
fn queue_irqs(ctx: &Ctx, iface: &str) -> Vec<(String, u32)> {
    let Ok(text) = ctx.fs.read("/proc/interrupts") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in text.lines() {
        let Some((num, rest)) = line.split_once(':') else {
            continue;
        };
        let irq = num.trim();
        if irq.is_empty() || !irq.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let label = rest.split_whitespace().last().unwrap_or_default();
        // mlx5 writes iface-0, intel iface-TxRx-0. Common part is the
        // interface name then a queue index.
        if !label.starts_with(iface) || !label.ends_with(|c: char| c.is_ascii_digit()) {
            continue;
        }
        let Some(cpu) = ctx
            .fs
            .read(format!("/proc/irq/{irq}/smp_affinity_list"))
            .ok()
            .and_then(|v| v.trim().split(&[',', '-'][..]).next()?.parse::<u32>().ok())
        else {
            continue;
        };
        out.push((label.to_string(), cpu));
    }
    out
}

/// A CPU's physical core, as its sibling list. Two SMT threads share it.
fn physical_core(ctx: &Ctx, cpu: u32) -> String {
    ctx.fs
        .read(format!(
            "/sys/devices/system/cpu/cpu{cpu}/topology/thread_siblings_list"
        ))
        .map(|v| v.trim().to_string())
        .unwrap_or_else(|_| cpu.to_string())
}

/// PF-NET-0002. Queue interrupts stacked on one core while others sit idle.
pub fn irq_affinity(ctx: &Ctx) -> Outcome {
    const WHY: &str = "The card sorts incoming packets into hardware queues, each drained by the \
        CPU it interrupts. If a queue fills faster than its CPU empties it, the card discards \
        packets, and what is discarded is shreds, votes and gossip the validator never sees. Two \
        queues landing on the two SMT threads of one physical core give those queues one core's \
        worth of capacity between them while other cores carry none. Anza publishes nothing here; \
        the vendor guides do, and they disagree with Red Hat about whether to disable irqbalance, \
        so preflight reports the layout and fails only on the stacking itself.";
    const EXPECTED: &str = "each queue interrupt on a core of its own";

    if let Some(o) = needs_linux(ctx, WHY) {
        return o;
    }
    let Some(iface) = primary_interface(ctx) else {
        return Outcome::skipped("no primary interface to read queue interrupts for");
    };
    let irqs = queue_irqs(ctx, &iface);
    if irqs.is_empty() {
        return Outcome::skipped(format!("{iface} raises no per-queue interrupts to place"));
    }

    let mut by_core: std::collections::BTreeMap<String, Vec<u32>> = Default::default();
    for (_, cpu) in &irqs {
        by_core
            .entry(physical_core(ctx, *cpu))
            .or_default()
            .push(*cpu);
    }
    let stacked: Vec<(&String, &Vec<u32>)> = by_core.iter().filter(|(_, v)| v.len() > 1).collect();
    let layout = {
        let mut cpus: Vec<String> = irqs.iter().map(|(_, c)| c.to_string()).collect();
        cpus.sort_by_key(|c| c.parse::<u32>().unwrap_or(0));
        format!("{} queues on CPUs {}", irqs.len(), cpus.join(" "))
    };

    // A good layout today is not a layout tomorrow. Enabled state is a
    // wants/ symlink, so no exec needed.
    let balancer = ctx
        .fs
        .exists("/etc/systemd/system/multi-user.target.wants/irqbalance.service");
    let drift = match balancer {
        true => " irqbalance is enabled, so whatever is set here it will move again on its own.",
        false => "",
    };

    if stacked.is_empty() {
        return Outcome::pass(format!("{layout}, one physical core each"), EXPECTED)
            .why(format!("{WHY}{drift}"));
    }
    let named: Vec<String> = stacked
        .iter()
        .map(|(core, cpus)| {
            let l: Vec<String> = cpus.iter().map(|c| c.to_string()).collect();
            format!("CPUs {} are one core ({core})", l.join(" and "))
        })
        .collect();
    Outcome::fail(format!("{layout}; {}", named.join("; ")), EXPECTED)
        .why(format!("{WHY}{drift}"))
        .fix(vec![
            FixStep::noted(
                "cat /sys/devices/system/cpu/cpu*/cache/index3/shared_cpu_list | sort -u",
                "read the real cache topology first, then spread the queues evenly across it \
                 rather than onto the first N cores",
            ),
            FixStep::noted(
                format!(
                    "echo <cpu> | sudo tee /proc/irq/<irq>/smp_affinity_list   for each {iface} queue"
                ),
                "one queue per physical core. Look the interrupts up by name, since irq numbers \
                 change across reboots",
            ),
            FixStep::noted(
                "then decide about irqbalance",
                "the vendor guides say disable it because it overrides manual affinity; Red Hat \
                 says not to unless every interrupt source is pinned by hand, or they all land on \
                 CPU 0. Pin them all, or leave it running and accept the drift",
            ),
        ])
        .verify(format!(
            "grep '{iface}-' /proc/interrupts | cut -d: -f1 | xargs -I{{}} cat /proc/irq/{{}}/smp_affinity_list"
        ))
}
