//! Every check, against hosts described as data below.
//!
//! Fixtures are built into target/fixtures on demand rather than committed as
//! directory trees, so a machine is readable in one place and a variant costs
//! one line.

use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{Mutex, OnceLock},
};

pub struct Host {
    pub name: &'static str,
    pub cpu_model: &'static str,
    pub cores: usize,
    pub threads: usize,
    pub mhz: &'static str,
    pub flags: &'static str,
    pub mem_kb: u64,
    /// name, size in GB, spinning
    pub disks: &'static [(&'static str, u64, bool)],
    pub mounts: &'static str,
    /// path under /proc/sys, value
    pub sysctl: &'static [(&'static str, &'static str)],
    /// interface name, driver, as a symlink target under sysfs
    pub nic: Option<(&'static str, &'static str)>,
    pub kernel: &'static str,
    pub os_release: &'static str,
    /// any other file, given as an absolute path
    pub files: &'static [(&'static str, &'static str)],
}

const AVX2: &str = "fpu vme de pse tsc msr avx avx2 avx512f sse4_2 aes";

/// A stock Ubuntu box with untouched kernel values and one small disk.
pub const FRESH_UBUNTU: Host = Host {
    name: "fresh-ubuntu",
    cpu_model: "AMD EPYC 9354P 32-Core Processor",
    cores: 32,
    threads: 64,
    mhz: "3800.000",
    flags: AVX2,
    mem_kb: 528_482_304,
    nic: Some(("eth0", "mlx5_core")),
    kernel: "6.8.0-31-generic",
    os_release: "PRETTY_NAME=\"Ubuntu 24.04.1 LTS\"\nNAME=\"Ubuntu\"\nID=ubuntu\nVERSION_ID=\"24.04\"\n",
    disks: &[("sda", 500, false)],
    mounts: "/dev/sda1 / ext4 rw,relatime 0 0\n\
             /dev/sda2 /mnt/accounts ext4 rw,noatime 0 0\n\
             /dev/sda3 /mnt/ledger ext4 rw,noatime 0 0\n\
             /dev/loop0 /snap/core20/2599 squashfs ro,nodev 0 0\n\
             /dev/sda1 /boot/efi vfat rw,relatime 0 0\n",
    sysctl: &[
        ("net/core/rmem_max", "212992"),
        ("net/core/wmem_max", "212992"),
        ("vm/max_map_count", "65530"),
        ("fs/nr_open", "1048576"),
    ],
    files: &[],
};

/// Anza's documented layout: a unit whose ExecStart points at a wrapper script,
/// carrying a command line written before v4.1.
pub const WRAPPER_SCRIPT_UNIT: Host = Host {
    name: "wrapper-script-unit",
    sysctl: &[
        ("net/core/rmem_max", "134217728"),
        ("net/core/wmem_max", "134217728"),
        ("vm/max_map_count", "1000000"),
        ("fs/nr_open", "1048576"),
    ],
    disks: &[
        ("nvme0n1", 2000, false),
        ("nvme1n1", 2000, false),
        ("nvme2n1", 2000, false),
    ],
    mounts: "/dev/nvme0n1p2 / ext4 rw,relatime 0 0\n\
             /dev/nvme1n1 /mnt/accounts ext4 rw,noatime 0 0\n\
             /dev/nvme2n1 /mnt/ledger ext4 rw,noatime 0 0\n",
    files: &[
        (
            "/etc/systemd/system/sol.service",
            "[Unit]\nDescription=Solana Validator\n\n\
             [Service]\nType=exec\nUser=sol\n\
             LimitNOFILE=1000000\nLimitMEMLOCK=2000000000\n\
             CapabilityBoundingSet=CAP_NET_RAW CAP_NET_ADMIN CAP_BPF CAP_PERFMON\n\
             ExecStart=/home/sol/bin/validator.sh\nRestart=always\n",
        ),
        (
            "/etc/sysctl.d/21-agave-validator.conf",
            "net.core.rmem_max = 134217728\nnet.core.wmem_max = 134217728\nvm.max_map_count = 1000000\n",
        ),
        (
            "/home/sol/bin/validator.sh",
            "#!/usr/bin/env bash\nset -e\nexec agave-validator \\\n\
             --identity /home/sol/validator-keypair.json \\\n\
             --vote-account /home/sol/vote-account-keypair.json \\\n\
             --entrypoint entrypoint.testnet.solana.com:8001 \\\n\
             --ledger /mnt/ledger \\\n\
             --accounts /mnt/accounts \\\n\
             --dynamic-port-range 8000-8020 \\\n\
             --limit-ledger-size 50000000 \\\n\
             --block-production-method central-scheduler \\\n\
             --accounts-index-limit minimal \\\n\
             --experimental-retransmit-xdp-interface eth0 \\\n\
             --experimental-retransmit-xdp-cpu-cores 1 \\\n\
             --experimental-poh-pinned-cpu-core 10 \\\n\
             --account-shrink-path /mnt/accounts/shrink \\\n\
             --tpu-disable-quic\n",
        ),
    ],
    ..FRESH_UBUNTU
};

/// The same host configured correctly: capabilities in a drop-in, wide port range.
pub const XDP_AMBIENT_OK: Host = Host {
    name: "xdp-ambient-ok",
    files: &[
        (
            "/etc/systemd/system/sol.service",
            "[Service]\nUser=sol\n\
             CapabilityBoundingSet=CAP_NET_RAW CAP_NET_ADMIN CAP_BPF CAP_PERFMON\n\
             ExecStart=/home/sol/bin/validator.sh\n",
        ),
        (
            "/etc/sysctl.d/21-agave-validator.conf",
            "net.core.rmem_max = 134217728\nnet.core.wmem_max = 134217728\nvm.max_map_count = 1000000\n",
        ),
        (
            "/etc/systemd/system/sol.service.d/20-xdp-caps.conf",
            "[Service]\nAmbientCapabilities=CAP_NET_RAW CAP_NET_ADMIN CAP_BPF CAP_PERFMON\n",
        ),
        (
            "/home/sol/bin/validator.sh",
            "#!/usr/bin/env bash\nexec agave-validator \\\n\
             --identity /home/sol/id.json \\\n\
             --vote-account /home/sol/vote.json \\\n\
             --entrypoint entrypoint.testnet.solana.com:8001 \\\n\
             --ledger /mnt/ledger \\\n\
             --accounts /mnt/accounts \\\n\
             --dynamic-port-range 8000-8030 \\\n\
             --xdp-interface eth0 \\\n\
             --xdp-zero-copy\n",
        ),
    ],
    ..WRAPPER_SCRIPT_UNIT
};

/// Everything on one spinning zfs volume, which fails every storage check.
pub const SHARED_DISK: Host = Host {
    name: "shared-disk",
    disks: &[("sda", 1000, true)],
    mounts: "/dev/sda1 / ext4 rw,relatime 0 0\n\
             /dev/sda2 /mnt/shared zfs rw,relatime 0 0\n",
    files: &[],
    ..FRESH_UBUNTU
};

/// A command line copied from Anza's XDP blog post, deprecated flags and all.
pub const STALE_BLOG_INVOCATION: &str = "exec agave-validator \\\n\
     --identity /home/sol/validator-keypair.json \\\n\
     --ledger /mnt/ledger \\\n\
     --accounts /mnt/accounts \\\n\
     --dynamic-port-range 11000-11020 \\\n\
     --experimental-retransmit-xdp-interface enp196s0f0np0 \\\n\
     --experimental-retransmit-xdp-cpu-cores 1 \\\n\
     --experimental-retransmit-xdp-zero-copy \\\n\
     --experimental-poh-pinned-cpu-core 10 \\\n\
     --allow-private-addr\n";

fn write(root: &Path, path: &str, body: &str) {
    let full = root.join(path.trim_start_matches('/'));
    fs::create_dir_all(full.parent().unwrap()).unwrap();
    fs::write(full, body).unwrap();
}

/// Materialise a host under target/fixtures and return its root. Tests run in
/// parallel, so each host is written exactly once per run.
pub fn build(h: &Host) -> PathBuf {
    static BUILT: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target/fixtures")
        .join(h.name);

    let mut built = BUILT
        .get_or_init(|| Mutex::new(HashSet::new()))
        .lock()
        .unwrap();
    if !built.insert(h.name) {
        return root;
    }
    let _ = fs::remove_dir_all(&root);

    write(&root, "/etc/os-release", h.os_release);
    write(
        &root,
        "/proc/sys/kernel/osrelease",
        &format!("{}\n", h.kernel),
    );
    write(
        &root,
        "/proc/meminfo",
        &format!(
            "MemTotal:       {} kB\nSwapTotal:             0 kB\n",
            h.mem_kb
        ),
    );
    write(&root, "/proc/mounts", h.mounts);

    let cpu: String = (0..h.threads)
        .map(|i| {
            format!(
                "processor\t: {i}\nmodel name\t: {}\ncpu MHz\t\t: {}\ncpu cores\t: {}\nflags\t\t: {}\n\n",
                h.cpu_model, h.mhz, h.cores, h.flags
            )
        })
        .collect();
    write(&root, "/proc/cpuinfo", &cpu);

    for (name, gb, rotational) in h.disks {
        write(
            &root,
            &format!("/sys/block/{name}/size"),
            &format!("{}\n", gb * 1_000_000_000 / 512),
        );
        write(
            &root,
            &format!("/sys/block/{name}/queue/rotational"),
            if *rotational { "1\n" } else { "0\n" },
        );
    }
    if let Some((iface, driver)) = h.nic {
        write(
            &root,
            "/proc/net/route",
            &format!(
                "Iface\tDestination\tGateway\tFlags\tRefCnt\tUse\tMetric\tMask\n\
                 {iface}\t00000000\t0100A8C0\t0003\t0\t0\t100\t00000000\n"
            ),
        );
        let dir = root.join(format!("sys/class/net/{iface}/device"));
        fs::create_dir_all(&dir).unwrap();
        let target = root.join(format!("sys/bus/pci/drivers/{driver}"));
        fs::create_dir_all(&target).unwrap();
        let _ = std::os::unix::fs::symlink(&target, dir.join("driver"));
    }
    for (key, value) in h.sysctl {
        write(&root, &format!("/proc/sys/{key}"), &format!("{value}\n"));
    }
    for (path, body) in h.files {
        write(&root, path, body);
    }
    root
}

/// Write a command line to a file and return its path, for `--invocation`.
pub fn invocation(name: &str, body: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target/fixtures")
        .join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, body).unwrap();
    path
}

/// Report text with wrapping collapsed, so an assertion can quote a sentence
/// without caring where the renderer broke the line.
fn flat(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Just one finding's block, ending where the next one starts.
fn block_for<'a>(output: &'a str, id: &str) -> &'a str {
    let after = match output.split_once(id) {
        Some((_, rest)) => rest,
        None => return "",
    };
    match after.find("\n  PF-") {
        Some(end) => &after[..end],
        None => after,
    }
}

fn host(h: &Host) -> String {
    build(h).display().to_string()
}

fn run(args: &[&str]) -> (String, i32) {
    let out = Command::new(env!("CARGO_BIN_EXE_preflight"))
        .args(args)
        .arg("--no-color")
        .output()
        .expect("run preflight");
    (
        String::from_utf8_lossy(&out.stdout).to_string(),
        out.status.code().unwrap_or(-1),
    )
}

#[test]
fn wrapper_script_resolves_to_full_flag_set() {
    let (o, _) = run(&[
        "--root",
        &host(&WRAPPER_SCRIPT_UNIT),
        "--client",
        "agave-validator@4.2.1",
    ]);
    assert!(o.contains("/home/sol/bin/validator.sh"), "{o}");
    assert!(o.contains("PF-ARG-0001"), "{o}");
}

#[test]
fn stable_channel_is_not_warned_about_later_releases() {
    let (o, _) = run(&[
        "--root",
        &host(&WRAPPER_SCRIPT_UNIT),
        "--client",
        "agave-validator@4.0.5",
    ]);
    assert!(
        !o.contains("PF-ARG-0005"),
        "4.1 check must not fire on 4.0: {o}"
    );
    assert!(o.contains("PF-ARG-0003"), "{o}");
}

#[test]
fn stale_blog_invocation_lights_up_xdp_and_ports() {
    let (o, _) = run(&[
        "--invocation",
        invocation("stale-blog.txt", STALE_BLOG_INVOCATION)
            .to_str()
            .unwrap(),
        "--client",
        "agave-validator@4.2.1",
        "--profile",
        "testnet",
    ]);
    for id in ["PF-ARG-0001", "PF-ARG-0002", "PF-ARG-0006", "PF-ARG-0007"] {
        assert!(o.contains(id), "expected {id} in:\n{o}");
    }
}

/// A validator preflight cannot read is Unknown, and an Unknown-only run exits
/// 4 so a declined or unreadable probe never looks like a clean bill of health.
#[test]
fn unresolved_invocation_is_unknown_not_empty() {
    let (o, code) = run(&[
        "--root",
        &host(&FRESH_UBUNTU),
        "--client",
        "agave-validator@4.2.1",
        "--profile",
        "testnet",
        "--only",
        "ARG",
    ]);
    assert!(o.contains("UNKNOWN"), "{o}");
    assert!(
        o.contains("could not resolve a validator invocation"),
        "{o}"
    );
    assert_eq!(code, 4, "unknown without failure is an incomplete run");
}

#[test]
fn xdp_bounding_set_without_ambient_fails() {
    let (o, _) = run(&[
        "--root",
        &host(&WRAPPER_SCRIPT_UNIT),
        "--client",
        "agave-validator@4.2.1",
    ]);
    assert!(o.contains("PF-XDP-0001"), "{o}");
    assert!(o.contains("with no AmbientCapabilities"), "{o}");
}

#[test]
fn xdp_capabilities_are_invocation_aware() {
    let (o, _) = run(&[
        "--root",
        &host(&WRAPPER_SCRIPT_UNIT),
        "--client",
        "agave-validator@4.2.1",
    ]);
    assert!(
        o.contains("CAP_NET_ADMIN and CAP_NET_RAW in the permitted set"),
        "{o}"
    );
    assert!(
        !o.contains("CAP_BPF and CAP_PERFMON in the permitted set"),
        "no zero-copy here: {o}"
    );

    // and zero copy pulls in the other two
    let (zc, _) = run(&[
        "--root",
        &host(&XDP_AMBIENT_OK),
        "--client",
        "agave-validator@4.2.1",
        "-v",
    ]);
    assert!(zc.contains("--xdp-zero-copy is in use"), "{zc}");
}

#[test]
fn xdp_persistence_is_unknown_not_ephemeral_without_a_process() {
    let (o, _) = run(&[
        "--root",
        &host(&WRAPPER_SCRIPT_UNIT),
        "--client",
        "agave-validator@4.2.1",
    ]);
    let block = o.split("PF-XDP-0007").nth(1).unwrap_or("");
    assert!(
        block.contains("UNKNOWN"),
        "cannot tell setcap from no grant here: {block}"
    );
}

#[test]
fn why_text_is_present_even_when_passing() {
    let (o, _) = run(&[
        "--root",
        &host(&XDP_AMBIENT_OK),
        "--client",
        "agave-validator@4.2.1",
        "-v",
    ]);
    let block = o.split("PF-ARG-0001").nth(1).unwrap_or("");
    assert!(
        block.contains("why"),
        "a passing check still explains itself: {block}"
    );
}

/// Spec §11 rule 3, as an invariant rather than an argument. Whatever an
/// unreleased changelog says today, a check sourced to it may not reach a
/// released client. If the 4.3 text changes before the tag exists, nobody gets
/// bad advice either way.
#[test]
fn provisional_checks_cannot_reach_a_released_client() {
    let ids = ["PF-ARG-0011", "PF-ARG-0012", "PF-ARG-0013"];
    for client in [
        "agave-validator@4.0.5",
        "agave-validator@4.1.0",
        "agave-validator@4.2.1",
    ] {
        let (o, _) = run(&[
            "--root",
            &host(&WRAPPER_SCRIPT_UNIT),
            "--client",
            client,
            "-v",
        ]);
        for id in ids {
            let block = o.split(id).nth(1).unwrap_or_default();
            let verdict = block.lines().next().unwrap_or_default();
            assert!(
                verdict.contains("SKIPPED"),
                "{id} is provisional and must not fire on {client}, got: {verdict}"
            );
        }
    }
}

#[test]
fn port_range_false_pass_at_the_boundary_is_impossible() {
    // 11000-11025 is 25 under agave's half-open arithmetic. An implementation
    // using end - start + 1 would call it 26 and pass.
    let dir = std::env::temp_dir().join("pf-boundary");
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("cmdline.txt");
    std::fs::write(
        &f,
        "exec agave-validator --dynamic-port-range 11000-11025 --ledger /l --accounts /a\n",
    )
    .unwrap();
    let (o, _) = run(&[
        "--invocation",
        f.to_str().unwrap(),
        "--client",
        "agave-validator@4.2.1",
        "--profile",
        "testnet",
    ]);
    let block = o.split("PF-ARG-0001").nth(1).unwrap_or_default();
    assert!(block.contains("FAIL"), "25 wide must fail: {block}");
    assert!(block.contains("(25 wide)"), "{block}");

    // and the plain case, so both ends of the arithmetic are pinned here
    let (o, code) = run(&[
        "--root",
        &host(&WRAPPER_SCRIPT_UNIT),
        "--client",
        "agave-validator@4.2.1",
    ]);
    assert!(
        o.contains("8000-8020 (20 wide)"),
        "end-start, not end-start+1: {o}"
    );
    assert_eq!(code, 1);
}

#[test]
fn every_failing_fix_names_the_file_to_edit() {
    let (o, _) = run(&[
        "--root",
        &host(&WRAPPER_SCRIPT_UNIT),
        "--client",
        "agave-validator@4.2.1",
    ]);
    let arg_blocks: Vec<&str> = o
        .split("  PF-")
        .filter(|b| b.starts_with("ARG-") && b.contains("FAIL"))
        .collect();
    assert!(!arg_blocks.is_empty());
    for b in arg_blocks {
        let id = &b[..11];
        assert!(
            b.contains("edit /home/sol/bin/validator.sh"),
            "{id} must name the resolved file, not generic advice:\n{b}"
        );
        assert!(
            b.contains("sudo systemctl restart sol.service"),
            "{id} must name the real unit:\n{b}"
        );
    }
}

#[test]
fn verify_commands_never_mutate() {
    let (o, _) = run(&[
        "--root",
        &host(&WRAPPER_SCRIPT_UNIT),
        "--client",
        "agave-validator@4.2.1",
        "-v",
    ]);
    for line in o.lines().filter(|l| l.trim_start().starts_with("verify")) {
        for bad in [
            "systemctl restart",
            "systemctl start",
            "systemctl stop",
            "| tee",
            "sudo tee",
            "sysctl -w",
        ] {
            assert!(
                !line.contains(bad),
                "verify must be read-only, found {bad:?} in: {line}"
            );
        }
    }
}

#[test]
fn run_ends_with_what_to_do_first() {
    let (o, _) = run(&[
        "--root",
        &host(&WRAPPER_SCRIPT_UNIT),
        "--client",
        "agave-validator@4.2.1",
    ]);
    assert!(
        o.contains("3 findings stop the validator from starting"),
        "{o}"
    );
    assert!(o.contains("PF-ARG-0001, PF-ARG-0003, PF-XDP-0001"), "{o}");
    assert!(flat(&o).contains("leave the node short"), "{o}");
}

#[test]
fn elevated_read_count_reflects_what_actually_runs() {
    let (arg_only, _) = run(&[
        "--root",
        &host(&WRAPPER_SCRIPT_UNIT),
        "--client",
        "agave-validator@4.2.1",
        "--only",
        "ARG",
    ]);
    assert!(
        !arg_only.contains("elevated reads"),
        "no ARG check needs root, so the line should not appear:\n{arg_only}"
    );
    let (all, _) = run(&[
        "--root",
        &host(&WRAPPER_SCRIPT_UNIT),
        "--client",
        "agave-validator@4.2.1",
    ]);
    assert!(all.contains("1 check needs elevated reads"), "{all}");
    assert!(all.contains("XDP networking"), "{all}");
}

#[test]
fn non_linux_host_says_so_once() {
    let (o, _) = run(&[
        "--invocation",
        invocation("stale-blog.txt", STALE_BLOG_INVOCATION)
            .to_str()
            .unwrap(),
        "--client",
        "agave-validator@4.2.1",
        "--profile",
        "testnet",
    ]);
    assert!(o.contains("and a Solana validator runs on Linux"), "{o}");
    // said once, in the verdict, not repeated per check
    assert_eq!(o.matches("runs on Linux").count(), 1, "{o}");
}

/// A drop-in CapabilityBoundingSet= replaces the unit's value rather than
/// adding to it. When the unit already permits what is needed, emitting one
/// would silently narrow it.
#[test]
fn xdp_drop_in_grants_without_narrowing() {
    let (o, _) = run(&[
        "--root",
        &host(&WRAPPER_SCRIPT_UNIT),
        "--client",
        "agave-validator@4.2.1",
        "--only",
        "PF-XDP-0001",
    ]);
    assert!(
        o.contains("AmbientCapabilities=CAP_NET_ADMIN CAP_NET_RAW"),
        "{o}"
    );
    assert!(
        !o.contains("CapabilityBoundingSet=CAP_NET_ADMIN CAP_NET_RAW\\n'"),
        "unit already permits all four; the fix must not rewrite the bounding set:\n{o}"
    );
    assert!(
        o.contains("already permits these, so it is left alone"),
        "{o}"
    );

    // and a unit that already grants them passes
    let (ok, _) = run(&[
        "--root",
        &host(&XDP_AMBIENT_OK),
        "--client",
        "agave-validator@4.2.1",
        "-v",
    ]);
    assert!(block_for(&ok, "PF-XDP-0001").contains("PASS"), "{ok}");
}

#[test]
fn deprecation_why_only_cites_flags_that_are_present() {
    let (o, _) = run(&[
        "--root",
        &host(&WRAPPER_SCRIPT_UNIT),
        "--client",
        "agave-validator@4.2.1",
        "--only",
        "PF-ARG-0008",
    ]);
    assert!(o.contains("--account-shrink-path"), "{o}");
    assert!(
        !o.contains("--accounts-db-access-storages-method in particular"),
        "why must not describe a flag the operator does not have:\n{o}"
    );
}

/// A version floor stays true forever: a flag removed in v4.0 is still removed
/// in v5. The registry must keep working on releases that did not exist when
/// it was written.
#[test]
fn checks_still_apply_on_a_far_future_release() {
    let (o, _) = run(&[
        "--root",
        &host(&WRAPPER_SCRIPT_UNIT),
        "--client",
        "agave-validator@9.9.9",
    ]);
    assert!(
        o.contains("PF-ARG-0003"),
        "v4.0 removals still apply on v9: {o}"
    );
    assert!(o.contains("PF-ARG-0001"), "{o}");
    assert!(
        o.contains("coverage may be incomplete"),
        "a client newer than the registry must say so:\n{o}"
    );
}

#[test]
fn no_release_channel_is_asserted_anywhere() {
    let (o, _) = run(&[
        "--root",
        &host(&WRAPPER_SCRIPT_UNIT),
        "--client",
        "agave-validator@4.2.1",
        "-v",
    ]);
    // whole words only: "edge" lives inside "/mnt/ledger"
    let sys: String = o.lines().take(14).collect::<Vec<_>>().join("\n");
    for word in ["alpha", "beta", "stable", "edge"] {
        assert!(
            !sys.split(|c: char| !c.is_alphanumeric()).any(|t| t == word),
            "channel labels rot; the report must not claim one: {sys}"
        );
    }
}

/// But a host that does have a validator preflight could not read is still
/// Unknown, and still exits 4.
#[test]
fn unreadable_validator_is_still_unknown() {
    let (o, code) = run(&[
        "--root",
        &host(&FRESH_UBUNTU),
        "--client",
        "agave-validator@4.2.1",
        "--profile",
        "testnet",
    ]);
    assert!(o.contains("resolution trail"), "{o}");
    assert_eq!(code, 1, "a fresh box has real kernel failures");
}

fn fake_validator(version: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("pf-fake-{}", version.replace('.', "_")));
    std::fs::create_dir_all(&dir).unwrap();
    let bin = dir.join("agave-validator");
    std::fs::write(
        &bin,
        format!("#!/bin/sh\necho \"agave-validator {version} (src:0; feat:1)\"\n"),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    std::fs::write(
        dir.join("cmdline.txt"),
        "exec agave-validator --ledger /l --accounts /a --dynamic-port-range 8000-8020\n",
    )
    .unwrap();
    dir
}

fn run_with_path(dir: &std::path::Path, args: &[&str]) -> String {
    let path = format!(
        "{}:{}",
        dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_preflight"))
        .args(args)
        .arg("--no-color")
        .env("PATH", path)
        .output()
        .expect("run preflight");
    String::from_utf8_lossy(&out.stdout).to_string()
}

/// The primary path: an operator runs `preflight` on their box with no flags.
/// Without version detection every check returns UNKNOWN and the tool is
/// useless on exactly the host it was built for.
#[test]
fn version_is_detected_without_any_flags() {
    let dir = fake_validator("4.2.1");
    let o = run_with_path(
        &dir,
        &[
            "--invocation",
            dir.join("cmdline.txt").to_str().unwrap(),
            "--profile",
            "testnet",
        ],
    );
    assert!(o.contains("agave-validator 4.2.1"), "{o}");
    assert!(o.contains("PF-ARG-0001"), "checks must actually run: {o}");
    assert!(!o.contains("client version not detected"), "{o}");
    assert!(
        o.contains("version read by running: agave-validator --version"),
        "executing anything must be disclosed:\n{o}"
    );
}

#[test]
fn no_exec_runs_nothing_and_says_why() {
    let dir = fake_validator("4.2.1");
    let o = run_with_path(
        &dir,
        &[
            "--invocation",
            dir.join("cmdline.txt").to_str().unwrap(),
            "--profile",
            "testnet",
            "--no-exec",
        ],
    );
    assert!(
        o.contains("version not detected: --no-exec was passed"),
        "{o}"
    );
    assert!(
        o.contains("preflight --client agave-validator@<version>"),
        "must say what to do: {o}"
    );
}

/// --root means preflight is reading a captured tree, so executing this host's
/// binary would report the wrong machine's version.
#[test]
fn root_mode_never_executes_the_host_binary() {
    let dir = fake_validator("4.2.1");
    let o = run_with_path(&dir, &["--root", &host(&WRAPPER_SCRIPT_UNIT)]);
    assert!(o.contains("--root is set"), "{o}");
    assert!(
        !o.contains("version from:"),
        "must not exec in root mode: {o}"
    );
}

/// The founding question: a bare box with no validator must still be told
/// whether it could run one. Host layers do not depend on a validator existing.
#[test]
fn a_bare_host_is_told_what_it_needs_and_how_to_ask() {
    let (o, _) = run(&["--root", &host(&FRESH_UBUNTU), "--profile", "testnet", "-v"]);
    for id in ["PF-HW-0001", "PF-HW-0002", "PF-KRN-0001", "PF-KRN-0004"] {
        assert!(
            o.contains(id),
            "expected {id} on a validator-less host:\n{o}"
        );
    }
    assert!(
        o.contains("CAN THIS MACHINE RUN A TESTNET VALIDATOR?"),
        "{o}"
    );
}

/// An unmeetable hardware requirement is Unsupported, not Fail: no command
/// fixes a CPU architecture, and offering one would be a lie.
#[test]
fn wrong_architecture_is_unsupported_with_no_fix() {
    let (o, _) = run(&["--profile", "testnet"]);
    if o.contains("PF-HW-0001") {
        let block = o.split("PF-HW-0001").nth(1).unwrap_or_default();
        let head: String = block.lines().take(12).collect::<Vec<_>>().join("\n");
        if head.contains("UNSUPPORTED") {
            assert!(
                !head.contains("\n  fix"),
                "Unsupported must carry no fix:\n{head}"
            );
        }
    }
}

/// "19 checks skipped" tells an operator nothing. Group them by reason so the
/// count is answerable rather than mysterious.
#[test]
fn skipped_checks_say_why_they_were_skipped() {
    let (o, _) = run(&["--root", &host(&FRESH_UBUNTU), "--profile", "testnet"]);
    assert!(o.contains("not checked"), "{o}");
    assert!(o.contains("need a validator installed on this host"), "{o}");
    assert!(
        !o.contains("skipped for this profile, client or version"),
        "old opaque wording: {o}"
    );
    assert!(o.contains("preflight -v  lists them individually"), "{o}");

    // version gated skips group separately from the rest
    let (v, _) = run(&[
        "--root",
        &host(&WRAPPER_SCRIPT_UNIT),
        "--client",
        "agave-validator@4.2.1",
    ]);
    assert!(
        v.contains("apply to a newer release than this client"),
        "{v}"
    );
}

/// A kernel default that is already adequate survives a reboot, so it is a
/// PASS. Calling it EPHEMERAL would be a false alarm and would erode what that
/// state means everywhere else.
#[test]
fn an_adequate_kernel_default_is_not_ephemeral() {
    let (o, _) = run(&["--root", &host(&FRESH_UBUNTU), "--profile", "testnet", "-v"]);
    let block = o.split("PF-KRN-0004").nth(1).unwrap_or_default();
    assert!(
        block.contains("PASS"),
        "fs.nr_open at its default is fine: {block}"
    );
    assert!(block.contains("kernel default"), "{block}");
}

/// FS is the layer that answers "can this machine run a validator" before
/// anything is installed. A single 500 GB disk is not enough for the ~2.5 TB
/// Anza specifies, and preflight must say so with no validator present.
#[test]
fn a_bare_box_is_told_its_storage_is_too_small() {
    let (o, _) = run(&["--root", &host(&FRESH_UBUNTU), "--profile", "mainnet"]);
    let block = block_for(&o, "PF-FS-0001");
    assert!(block.contains("FAIL"), "{block}");
    assert!(block.contains("500 GB across 1 solid-state"), "{block}");
    assert!(
        block.contains("accounts 1000 GB, ledger 1000 GB, snapshots 500 GB"),
        "{block}"
    );
}

#[test]
fn shared_spinning_zfs_storage_is_caught_on_every_axis() {
    let inv = std::env::temp_dir().join("pf-shared.txt");
    std::fs::write(
        &inv,
        "exec agave-validator --ledger /mnt/shared/ledger --accounts /mnt/shared/accounts\n",
    )
    .unwrap();
    let (o, _) = run(&[
        "--root",
        &host(&SHARED_DISK),
        "--invocation",
        inv.to_str().unwrap(),
        "--client",
        "agave-validator@4.2.1",
        "--profile",
        "testnet",
    ]);
    assert!(o.contains("accounts and ledger both on sda"), "{o}");
    assert!(flat(&o).contains("on spinning disk sda"), "{o}");
    assert!(flat(&o).contains("has no noatime"), "{o}");
    assert!(flat(&o).contains("/mnt/shared/accounts on zfs"), "{o}");
    assert!(flat(&o).contains("does not support O_DIRECT"), "{o}");
}

#[test]
fn three_separate_nvme_devices_pass() {
    let (o, _) = run(&[
        "--root",
        &host(&WRAPPER_SCRIPT_UNIT),
        "--client",
        "agave-validator@4.2.1",
        "-v",
    ]);
    for id in ["PF-FS-0002", "PF-FS-0003", "PF-FS-0005"] {
        let block = o.split(id).nth(1).unwrap_or_default();
        let head: String = block.lines().take(2).collect::<Vec<_>>().join(" ");
        assert!(
            head.contains("PASS"),
            "{id} should pass on three NVMe: {head}"
        );
    }
}

/// The machine question is answered first and on its own, because a box that
/// cannot run a validator makes every finding about a validator's own
/// configuration beside the point.
#[test]
fn the_machine_question_comes_before_the_validator_question() {
    let (o, _) = run(&[
        "--root",
        &host(&WRAPPER_SCRIPT_UNIT),
        "--client",
        "agave-validator@4.2.1",
    ]);
    let machine = o.find("CAN THIS MACHINE RUN").expect("machine question");
    let validator = o
        .find("IS THE VALIDATOR CONFIGURED")
        .expect("validator question");
    assert!(
        machine < validator,
        "machine question must come first:\n{o}"
    );

    // and every finding sits under the question it belongs to
    let (first, second) = o.split_at(validator);
    assert!(
        first.contains("PF-KRN") || first.contains("PF-FS") || first.contains("PF-HW"),
        "{first}"
    );
    assert!(
        second.contains("PF-ARG") || second.contains("PF-XDP"),
        "{second}"
    );
    assert!(
        !second.contains("PF-KRN"),
        "kernel findings belong to the machine:\n{second}"
    );

    // and each half carries its own verdict
    assert!(
        o.contains("requirements not met") || o.contains("worth fixing"),
        "{o}"
    );
    let (bare, _) = run(&["--root", &host(&FRESH_UBUNTU), "--profile", "testnet"]);
    assert!(
        bare.contains("no validator installed, nothing to check"),
        "{bare}"
    );
}

/// Cores and memory have no published minimum, so they report rather than
/// judge. Their Unknown is by design and must not stop a verdict.
#[test]
fn report_only_checks_do_not_block_a_verdict() {
    let (o, _) = run(&[
        "--root",
        &host(&XDP_AMBIENT_OK),
        "--client",
        "agave-validator@4.2.1",
        "-v",
    ]);
    assert!(o.contains("PF-HW-0004"), "{o}");
    let machine = o.split("IS THE VALIDATOR").next().unwrap_or_default();
    assert!(
        machine.contains("  yes"),
        "report-only Unknowns must not veto:\n{machine}"
    );
}

/// The commands the report suggests have to actually work. A space separated
/// cmdline, which is what the suggested ssh pipeline produces, must parse.
#[test]
fn the_suggested_cmdline_capture_parses() {
    let f = invocation(
        "space-separated.txt",
        "agave-validator --identity /i.json --ledger /l --accounts /a \
         --dynamic-port-range 8000-8020 --tpu-disable-quic ",
    );
    let (o, _) = run(&[
        "--invocation",
        f.to_str().unwrap(),
        "--client",
        "agave-validator@4.2.1",
        "--profile",
        "testnet",
    ]);
    assert!(o.contains("PF-ARG-0001"), "{o}");
    assert!(o.contains("PF-ARG-0003"), "{o}");
}

/// preflight builds for the host it is compiled on, so telling a macOS user to
/// copy their binary to a Linux server would hand them something that cannot
/// execute there.
#[test]
fn non_linux_advice_never_suggests_copying_the_binary() {
    let (o, _) = run(&["--profile", "testnet"]);
    if o.contains("runs on Linux") {
        assert!(
            !o.contains("scp target/release"),
            "a host binary will not run there:\n{o}"
        );
        assert!(o.contains("cargo install"), "{o}");
        assert!(
            o.contains("cmdline.txt"),
            "must say where the file comes from:\n{o}"
        );
    }
}

/// The install URL printed to users comes from Cargo.toml, so there is one
/// place to change it and no chance of the two disagreeing.
#[test]
fn install_url_matches_the_manifest() {
    let (o, _) = run(&["--profile", "testnet"]);
    if o.contains("cargo install") {
        assert!(o.contains(env!("CARGO_PKG_REPOSITORY")), "{o}");
    }
}

/// Promise 2 says preflight runs nothing you have not seen. Nothing in the
/// codebase may invoke sudo, since the prompt flow that would make that
/// honest is not built.
#[test]
fn nothing_is_ever_run_with_sudo() {
    let src = std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/src")).unwrap();
    let mut files: Vec<PathBuf> = Vec::new();
    for e in src.flatten() {
        match e.path().is_dir() {
            true => files.extend(
                std::fs::read_dir(e.path())
                    .unwrap()
                    .flatten()
                    .map(|x| x.path()),
            ),
            false => files.push(e.path()),
        }
    }
    for f in files
        .iter()
        .filter(|f| f.extension().is_some_and(|e| e == "rs"))
    {
        let body = fs::read_to_string(f).unwrap();
        for (n, line) in body.lines().enumerate() {
            let invokes = line.contains("Command::new") && line.contains("sudo");
            assert!(!invokes, "{}:{} runs sudo: {line}", f.display(), n + 1);
        }
    }
}

/// A unit that merely mentions a validator is not the unit running one.
/// Matching on the word alone pointed every fix at the wrong file and told the
/// operator to restart the wrong service.
#[test]
fn a_unit_that_only_mentions_a_validator_is_not_chosen() {
    let decoy = Host {
        name: "decoy-unit",
        files: &[
            (
                "/etc/systemd/system/collector.service",
                "[Unit]\nDescription=vyralabs validator metrics collector\n\n\
                 [Service]\nExecStart=/home/sol/collector/target/release/collector\n",
            ),
            (
                "/etc/systemd/system/sol.service",
                "[Service]\nUser=sol\nExecStart=/home/sol/bin/validator.sh\n",
            ),
            (
                "/home/sol/bin/validator.sh",
                "#!/usr/bin/env bash\nexec agave-validator --ledger /l --accounts /a \
                 --dynamic-port-range 8000-8030\n",
            ),
        ],
        ..WRAPPER_SCRIPT_UNIT
    };
    let (o, _) = run(&["--root", &host(&decoy), "--client", "agave-validator@4.2.1"]);
    assert!(
        o.contains("/home/sol/bin/validator.sh"),
        "must find the real one:\n{o}"
    );
    assert!(
        !o.contains("collector"),
        "must not pick a unit that only mentions one:\n{o}"
    );
}

/// A real host carries a dozen snap loopbacks and a boot partition. None are
/// validator storage, and listing them buries the disks that are.
#[test]
fn snap_and_boot_mounts_stay_out_of_the_report() {
    let (o, _) = run(&["--root", &host(&FRESH_UBUNTU), "--profile", "testnet"]);
    assert!(o.contains("/mnt/accounts"), "{o}");
    assert!(
        !o.contains("squashfs"),
        "snap loopbacks are not storage:\n{o}"
    );
    assert!(!o.contains("/boot/efi"), "{o}");
}

/// Anza publishes no memory minimum, so this reports and does not judge. An
/// invented 128 GB threshold failed a working 125 GB validator.
#[test]
fn memory_is_reported_not_failed() {
    let small = Host {
        name: "small-memory",
        mem_kb: 131_500_000,
        ..FRESH_UBUNTU
    };
    let (o, _) = run(&["--root", &host(&small), "--profile", "testnet", "-v"]);
    let block = o.split("PF-HW-0005").nth(1).unwrap_or_default();
    let head: String = block.lines().take(2).collect::<Vec<_>>().join(" ");
    assert!(
        !head.contains("FAIL"),
        "no published minimum means no failure:\n{head}"
    );
}

/// Anza cautions about accounts and ledger sharing a disk. It says nothing
/// about snapshots, which operators deliberately keep beside the ledger.
#[test]
fn snapshots_beside_the_ledger_is_not_a_finding() {
    let shared_snapshots = Host {
        name: "snapshots-with-ledger",
        disks: &[("nvme0n1", 2000, false), ("nvme1n1", 2000, false)],
        mounts: "/dev/nvme0n1p1 / ext4 rw,noatime 0 0\n\
                 /dev/nvme1n1 /mnt/accounts ext4 rw,noatime 0 0\n",
        files: &[],
        ..WRAPPER_SCRIPT_UNIT
    };
    let inv = invocation(
        "shared-snapshots.txt",
        "exec agave-validator --accounts /mnt/accounts --ledger /ledger \
         --snapshots /ledger/snapshot-store --dynamic-port-range 8000-8030\n",
    );
    let (o, _) = run(&[
        "--root",
        &host(&shared_snapshots),
        "--invocation",
        inv.to_str().unwrap(),
        "--client",
        "agave-validator@4.2.1",
        "--profile",
        "testnet",
    ]);
    let block = o.split("PF-FS-0002").nth(1).unwrap_or_default();
    let head: String = block.lines().take(2).collect::<Vec<_>>().join(" ");
    assert!(
        !head.contains("FAIL"),
        "snapshots beside the ledger is normal:\n{head}"
    );
}

/// noatime is operator practice. Anza's requirements page does not mention it,
/// so citing that page for it would be inventing a source.
#[test]
fn noatime_is_not_cited_to_anza() {
    let inv = invocation(
        "noatime-check.txt",
        "exec agave-validator --accounts /mnt/shared/a --ledger /mnt/shared/l\n",
    );
    let (o, _) = run(&[
        "--root",
        &host(&SHARED_DISK),
        "--invocation",
        inv.to_str().unwrap(),
        "--client",
        "agave-validator@4.2.1",
        "--profile",
        "testnet",
        "-v",
    ]);
    let block = o.split("PF-FS-0004").nth(1).unwrap_or_default();
    let cited = block.split("source").nth(1).unwrap_or_default();
    assert!(
        !cited.contains("docs.anza.xyz"),
        "Anza does not publish noatime:\n{cited}"
    );
    assert!(block.contains("Anza does not publish this one"), "{block}");
}

/// Core count is not the metric. The community list carries 16 core parts that
/// out-hash 32 core parts, so the check must cite that rather than imply more
/// cores is better.
#[test]
fn core_count_check_cites_the_community_list() {
    let (o, _) = run(&["--root", &host(&FRESH_UBUNTU), "--profile", "testnet", "-v"]);
    let block = block_for(&o, "PF-HW-0004");
    assert!(block.contains("solanahcl.org"), "{block}");
    assert!(block.contains("16 core"), "{block}");
    assert!(
        !block.contains("FAIL"),
        "no published minimum means no failure:\n{block}"
    );
}

/// Nobody publishes a memory minimum, Anza or the community list, so preflight
/// must not imply the 512 GB board suggestion is a testnet requirement.
#[test]
fn memory_check_does_not_present_512gb_as_required() {
    let (o, _) = run(&["--root", &host(&FRESH_UBUNTU), "--profile", "testnet", "-v"]);
    let block = block_for(&o, "PF-HW-0005");
    assert!(block.contains("no published minimum"), "{block}");
    assert!(
        block.contains("REPORTED"),
        "a measured value with no threshold is not Unknown:\n{block}"
    );
    // the expected line must not name one cluster while another is active
    let (mainnet, _) = run(&["--root", &host(&FRESH_UBUNTU), "--profile", "mainnet", "-v"]);
    assert!(
        !block_for(&mainnet, "PF-HW-0005").contains("testnet runs on far less"),
        "{mainnet}"
    );
}

/// The profile has to change what a check demands, not just which checks run.
/// Anza's figures describe a production node, so they apply to mainnet. Nobody
/// publishes testnet figures, so testnet is judged on headroom instead.
#[test]
fn storage_thresholds_follow_the_profile() {
    let small = Host {
        name: "one-small-disk",
        disks: &[("sda", 500, false)],
        ..FRESH_UBUNTU
    };
    let root = host(&small);

    let (mainnet, _) = run(&["--root", &root, "--profile", "mainnet"]);
    assert!(
        mainnet.contains("PF-FS-0001"),
        "500 GB is short for mainnet:\n{mainnet}"
    );
    assert!(
        flat(block_for(&mainnet, "PF-FS-0001")).contains("preflight applies them to mainnet"),
        "{mainnet}"
    );

    let (testnet, _) = run(&["--root", &root, "--profile", "testnet", "-v"]);
    let block = block_for(&testnet, "PF-FS-0001");
    assert!(
        block.contains("PASS"),
        "the same box must not fail on testnet:\n{block}"
    );
    assert!(
        flat(block).contains("does not judge you against a size"),
        "{block}"
    );
}

#[test]
fn base_clock_is_not_demanded_of_a_local_validator() {
    let (o, _) = run(&["--root", &host(&FRESH_UBUNTU), "--profile", "local", "-v"]);
    let block = block_for(&o, "PF-HW-0003");
    assert!(
        block.is_empty() || block.contains("SKIPPED"),
        "a test validator has no clock requirement:\n{block}"
    );
}

/// The verdict names a cluster, so the report must say where that came from.
/// Inferring silently leaves an operator wondering which figures were applied.
#[test]
fn the_report_says_which_profile_it_inferred_and_why() {
    let (o, _) = run(&[
        "--root",
        &host(&WRAPPER_SCRIPT_UNIT),
        "--client",
        "agave-validator@4.2.1",
    ]);
    assert!(o.contains("profile     testnet"), "{o}");
    assert!(
        o.contains("entrypoint entrypoint.testnet.solana.com"),
        "{o}"
    );

    let (forced, _) = run(&[
        "--root",
        &host(&WRAPPER_SCRIPT_UNIT),
        "--client",
        "agave-validator@4.2.1",
        "--profile",
        "mainnet",
    ]);
    assert!(forced.contains("set with --profile"), "{forced}");
}

/// The driver decides whether XDP works at all. Anza publishes no list, so
/// this comes from the community one, which records what operators got running.
#[test]
fn an_unsupported_nic_driver_is_reported() {
    let realtek = Host {
        name: "realtek-nic",
        nic: Some(("eth0", "r8169")),
        ..FRESH_UBUNTU
    };
    let (o, _) = run(&["--root", &host(&realtek), "--profile", "testnet"]);
    let block = block_for(&o, "PF-NET-0001");
    assert!(block.contains("FAIL"), "{block}");
    assert!(flat(block).contains("Realtek"), "{block}");
    assert!(flat(block).contains("No native XDP"), "{block}");

    // and the highest confidence family passes
    let (ok, _) = run(&["--root", &host(&FRESH_UBUNTU), "--profile", "testnet", "-v"]);
    assert!(block_for(&ok, "PF-NET-0001").contains("PASS"), "{ok}");
}

/// bnxt_en carries XDP but never accepts zero copy, so passing the flag is a
/// finding rather than something that silently does nothing.
#[test]
fn zero_copy_on_a_driver_that_refuses_it_is_reported() {
    let broadcom = Host {
        name: "broadcom-nic",
        nic: Some(("eth0", "bnxt_en")),
        ..FRESH_UBUNTU
    };
    let inv = invocation(
        "zero-copy.txt",
        "exec agave-validator --ledger /l --accounts /a --xdp-interface eth0 --xdp-zero-copy\n",
    );
    let (o, _) = run(&[
        "--root",
        &host(&broadcom),
        "--invocation",
        inv.to_str().unwrap(),
        "--client",
        "agave-validator@4.2.1",
        "--profile",
        "testnet",
    ]);
    let block = block_for(&o, "PF-NET-0001");
    assert!(block.contains("FAIL"), "{block}");
    assert!(flat(block).contains("remove --xdp-zero-copy"), "{block}");
}

/// Absence from the community list is not failure on testnet, where hardware
/// varies widely. On mainnet it is worth knowing before taking stake.
#[test]
fn an_unlisted_cpu_is_reported_on_testnet_and_flagged_on_mainnet() {
    let unlisted = Host {
        name: "unlisted-cpu",
        cpu_model: "AMD EPYC 7313P 16-Core Processor",
        ..FRESH_UBUNTU
    };
    let root = host(&unlisted);

    let (testnet, _) = run(&["--root", &root, "--profile", "testnet", "-v"]);
    let block = block_for(&testnet, "PF-HW-0006");
    assert!(
        block.contains("REPORTED"),
        "measured but unjudgeable is not a failed probe:\n{block}"
    );

    let (mainnet, _) = run(&["--root", &root, "--profile", "mainnet"]);
    let block = block_for(&mainnet, "PF-HW-0006");
    assert!(
        block.contains("FAIL"),
        "worth flagging on mainnet:\n{block}"
    );
    assert!(flat(block).contains("measure your PoH rate"), "{block}");

    // a listed part reports the numbers operators saw
    let (listed, _) = run(&["--root", &host(&FRESH_UBUNTU), "--profile", "mainnet", "-v"]);
    let block = block_for(&listed, "PF-HW-0006");
    assert!(block.contains("PASS"), "{block}");
    assert!(flat(block).contains("reported PoH"), "{block}");
}

/// The prompt is for a person watching. Piping, redirecting or asking for JSON
/// must never block waiting for input, or preflight cannot run in CI or as an
/// ExecStartPre.
#[test]
fn the_profile_prompt_never_blocks_a_pipe() {
    let (o, _) = run(&["--root", &host(&FRESH_UBUNTU)]);
    assert!(!o.contains("Which are you asking about?"), "{o}");
    assert!(o.contains("SYSTEM"), "the run must complete: {o}");

    let (json, _) = run(&["--root", &host(&FRESH_UBUNTU), "--format", "json"]);
    assert!(json.starts_with('{'), "{json}");
}

/// A count of passing checks tells an operator nothing. They cannot tell
/// whether the thing they were worried about was even looked at.
#[test]
fn passing_checks_are_named_not_just_counted() {
    let (o, _) = run(&[
        "--root",
        &host(&WRAPPER_SCRIPT_UNIT),
        "--client",
        "agave-validator@4.2.1",
    ]);
    assert!(o.contains("checked and fine"), "{o}");
    assert!(o.contains("PF-KRN-0001  net.core.rmem_max"), "{o}");

    // under -v the full block is already printed, so the list would repeat it
    let (verbose, _) = run(&[
        "--root",
        &host(&WRAPPER_SCRIPT_UNIT),
        "--client",
        "agave-validator@4.2.1",
        "-v",
    ]);
    assert!(!verbose.contains("checked and fine"), "{verbose}");
}

/// XDP is on by default since v4.2 and Anza gives a kernel floor for it. A box
/// three minor versions below that floor was reported as entirely fine.
#[test]
fn an_old_kernel_under_xdp_is_reported() {
    let old = Host {
        name: "old-kernel",
        kernel: "5.15.0-139-generic",
        nic: Some(("eth0", "bnxt_en")),
        ..WRAPPER_SCRIPT_UNIT
    };
    let (o, _) = run(&["--root", &host(&old), "--client", "agave-validator@4.3.0"]);
    let block = block_for(&o, "PF-KRN-0005");
    assert!(block.contains("FAIL"), "{block}");
    assert!(flat(block).contains("kernel 5.15"), "{block}");
    assert!(flat(block).contains("kernel 6.8 or newer"), "{block}");
}

/// igb needs a newer kernel than everything else, so the floor reads the card.
#[test]
fn the_kernel_floor_follows_the_driver() {
    let igb = Host {
        name: "igb-nic",
        kernel: "6.10.0-generic",
        nic: Some(("eth0", "igb")),
        ..WRAPPER_SCRIPT_UNIT
    };
    let (o, _) = run(&["--root", &host(&igb), "--client", "agave-validator@4.3.0"]);
    let block = block_for(&o, "PF-KRN-0005");
    assert!(
        block.contains("FAIL"),
        "6.10 clears 6.8 but not igb's 6.14:\n{block}"
    );
    assert!(flat(block).contains("because the driver is igb"), "{block}");
}

#[test]
fn a_current_kernel_passes_the_xdp_floor() {
    let (o, _) = run(&[
        "--root",
        &host(&WRAPPER_SCRIPT_UNIT),
        "--client",
        "agave-validator@4.2.1",
        "-v",
    ]);
    assert!(block_for(&o, "PF-KRN-0005").contains("PASS"), "{o}");
}

/// A forced profile that disagrees with the box has to say so. Every fix below
/// quotes the real paths and services, and this report gets screenshotted.
#[test]
fn a_forced_profile_that_contradicts_the_box_says_so() {
    let (o, _) = run(&[
        "--root",
        &host(&WRAPPER_SCRIPT_UNIT),
        "--client",
        "agave-validator@4.2.1",
        "--profile",
        "mainnet",
    ]);
    assert!(flat(&o).contains("this box looks like testnet"), "{o}");
    assert!(flat(&o).contains("judge it as mainnet"), "{o}");

    // and it stays quiet when they agree
    let (agree, _) = run(&[
        "--root",
        &host(&WRAPPER_SCRIPT_UNIT),
        "--client",
        "agave-validator@4.2.1",
        "--profile",
        "testnet",
    ]);
    assert!(!agree.contains("this box looks like"), "{agree}");
}

/// The header carries the version as reported, the command that produced the
/// report and when, because this output is meant to be pasted at someone.
#[test]
fn the_header_is_enough_to_read_a_pasted_report() {
    let (o, _) = run(&[
        "--root",
        &host(&WRAPPER_SCRIPT_UNIT),
        "--client",
        "agave-validator@4.3.0-beta.0",
    ]);
    assert!(
        o.contains("4.3.0-beta.0"),
        "prerelease must not read as 4.3.0:\n{o}"
    );
    assert!(o.contains("run         preflight --root"), "{o}");
    assert!(o.contains("UTC"), "{o}");
}

/// A box running testnet is a fair thing to judge against mainnet, so the
/// report says how to ask rather than leaving the flag undiscoverable.
#[test]
fn the_report_says_how_to_ask_about_another_cluster() {
    let (o, _) = run(&[
        "--root",
        &host(&WRAPPER_SCRIPT_UNIT),
        "--client",
        "agave-validator@4.2.1",
    ]);
    assert!(o.contains("preflight --profile mainnet"), "{o}");

    let (mainnet, _) = run(&[
        "--root",
        &host(&WRAPPER_SCRIPT_UNIT),
        "--client",
        "agave-validator@4.2.1",
        "--profile",
        "mainnet",
    ]);
    assert!(mainnet.contains("preflight --profile testnet"), "{mainnet}");
    assert!(
        !mainnet.contains("--profile mainnet |"),
        "never offer the current one:\n{mainnet}"
    );
}

#[test]
fn the_run_row_reads_cleanly_with_no_arguments() {
    let dir = fake_validator("4.2.1");
    let o = run_with_path(&dir, &["--profile", "testnet"]);
    assert!(!o.contains("preflight  ·"), "no double space:\n{o}");
}

/// A release past standard support is usually why the kernel is old, and why
/// catching up is a release upgrade rather than an apt command.
#[test]
fn a_release_past_standard_support_is_reported() {
    let old = Host {
        name: "focal",
        os_release: "PRETTY_NAME=\"Ubuntu 20.04.6 LTS\"\nID=ubuntu\nVERSION_ID=\"20.04\"\n",
        kernel: "5.15.0-139-generic",
        ..WRAPPER_SCRIPT_UNIT
    };
    let (o, _) = run(&["--root", &host(&old), "--client", "agave-validator@4.3.0"]);
    let block = block_for(&o, "PF-HW-0007");
    assert!(block.contains("FAIL"), "{block}");
    assert!(
        flat(block).contains("standard support ended 2025-05"),
        "{block}"
    );
    assert!(flat(block).contains("plan a release upgrade"), "{block}");
}

#[test]
fn a_supported_release_passes() {
    let (o, _) = run(&[
        "--root",
        &host(&WRAPPER_SCRIPT_UNIT),
        "--client",
        "agave-validator@4.2.1",
        "-v",
    ]);
    assert!(block_for(&o, "PF-HW-0007").contains("PASS"), "{o}");
}

/// The kernel floor is a property of the machine, so it belongs to the machine
/// question. Under the configuration question it let a box that cannot carry
/// the default transmit path answer "yes".
#[test]
fn the_kernel_floor_belongs_to_the_machine_question() {
    let old = Host {
        name: "old-kernel-placement",
        kernel: "5.15.0-139-generic",
        ..WRAPPER_SCRIPT_UNIT
    };
    let (o, _) = run(&["--root", &host(&old), "--client", "agave-validator@4.3.0"]);
    let machine = o.split("IS THE VALIDATOR").next().unwrap_or_default();
    assert!(machine.contains("PF-KRN-0005"), "{machine}");
    assert!(
        machine.contains("requirement"),
        "the verdict must say no:\n{machine}"
    );
}

/// Default-on XDP below the floor with no --no-xdp is not a tuning shortfall,
/// and the fix leads with the action that helps today.
#[test]
fn an_unguarded_xdp_path_leads_with_the_fallback() {
    let old = Host {
        name: "unguarded-xdp",
        kernel: "5.15.0-139-generic",
        ..WRAPPER_SCRIPT_UNIT
    };
    let (o, _) = run(&["--root", &host(&old), "--client", "agave-validator@4.3.0"]);
    let block = block_for(&o, "PF-KRN-0005");
    assert!(flat(block).contains("live with no fallback"), "{block}");
    assert!(
        flat(block).contains("fix --no-xdp"),
        "the fallback comes first:\n{block}"
    );
}

/// A machine that fails mainnet requirements must not be told "the validator
/// will start" as its closing line. Under a verdict that just said no, that
/// reads as permission.
#[test]
fn the_closing_line_leads_with_the_machine() {
    // Shaped like a working testnet box that is not a mainnet box: valid
    // configuration, a CPU nobody has reported mainnet numbers for.
    let small = Host {
        name: "mainnet-unsuitable",
        cpu_model: "AMD EPYC 7313P 16-Core Processor",
        ..XDP_AMBIENT_OK
    };
    let (o, _) = run(&[
        "--root",
        &host(&small),
        "--client",
        "agave-validator@4.2.1",
        "--profile",
        "mainnet",
    ]);
    assert!(
        o.contains("CAN THIS MACHINE RUN A MAINNET VALIDATOR?"),
        "{o}"
    );
    assert!(
        !flat(&o).contains("next the validator will start"),
        "must not open with reassurance under a no:\n{o}"
    );
    assert!(
        flat(&o).contains("this machine does not meet") || flat(&o).contains("the machine misses"),
        "{o}"
    );
}

/// Severity is what a finding costs when it fails. REPORTED cannot fail, so
/// printing one next to it reads as a verdict that was never reached.
#[test]
fn reported_findings_carry_no_severity() {
    let (o, _) = run(&[
        "--root",
        &host(&WRAPPER_SCRIPT_UNIT),
        "--client",
        "agave-validator@4.2.1",
        "-v",
    ]);
    for line in o.lines().filter(|l| l.contains("REPORTED")) {
        for sev in ["fatal", "degraded", "advisory"] {
            assert!(!line.contains(sev), "REPORTED needs no severity: {line}");
        }
        assert_eq!(line, line.trim_end(), "no trailing space: {line:?}");
    }
}

/// The headroom figure is preflight's own. Citing Anza for it would be the
/// same invention the storage check refuses to make about sizes.
#[test]
fn the_headroom_figure_is_not_cited_to_anza() {
    let (o, _) = run(&[
        "--root",
        &host(&WRAPPER_SCRIPT_UNIT),
        "--client",
        "agave-validator@4.2.1",
        "-v",
    ]);
    let block = block_for(&o, "PF-FS-0001");
    assert!(flat(block).contains("no published figure"), "{block}");
    assert!(
        flat(block).contains("preflight's own line, not anybody's published requirement"),
        "{block}"
    );
    assert!(
        flat(block).contains("headroom figure is preflight's own"),
        "the source must say so too:\n{block}"
    );
}
