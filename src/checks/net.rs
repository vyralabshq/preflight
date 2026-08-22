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
        note: "Supports native XDP and zero copy. XDP is blocked for frames larger than 3KB.",
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
