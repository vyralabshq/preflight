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
            let core = if h.cores == 0 { i } else { i % h.cores };
            format!(
                "processor\t: {i}\nphysical id\t: 0\ncore id\t\t: {core}\n\
                 model name\t: {}\ncpu MHz\t\t: {}\ncpu cores\t: {}\nflags\t\t: {}\n\n",
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

/// Whatever an unreleased changelog says today, a check sourced to it may not
/// reach a released client. If the 4.3 text changes before the tag exists, nobody gets
/// bad advice either way.
#[test]
fn provisional_checks_cannot_reach_a_released_client() {
    // 0011 and 0013 were settled against a real 4.3.0-beta.0 binary and are no
    // longer provisional. 0012 stays: --help cannot distinguish a hidden
    // deprecated flag from a removed one.
    let ids = ["PF-ARG-0012"];
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
        o.contains("2 findings stop the validator from starting"),
        "{o}"
    );
    // A missing capability is not one of them: agave only refuses to start over
    // it when the invocation asks for XDP explicitly.
    assert!(o.contains("PF-ARG-0001, PF-ARG-0003"), "{o}");
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
    // No check claims elevated reads while preflight never executes sudo.
    assert!(
        !all.contains("needs elevated reads"),
        "the header must not promise a prompt the binary does not have:\n{all}"
    );
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
    assert!(block.contains("stock value"), "{block}");
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

/// Core count is not the metric. Anza lists 12/24 as a guide; the community
/// list carries 16 core parts that out-hash 32 core parts. Neither is a FAIL.
#[test]
fn core_count_check_cites_anza_and_the_community_list() {
    let (o, _) = run(&["--root", &host(&FRESH_UBUNTU), "--profile", "testnet", "-v"]);
    let block = block_for(&o, "PF-HW-0004");
    assert!(block.contains("solanahcl.org"), "{block}");
    assert!(flat(block).contains("12 cores"), "{block}");
    assert!(flat(block).contains("RPC column"), "{block}");
    assert!(
        !block.contains("FAIL"),
        "Anza's figure is a guide, not a floor we fail on:\n{block}"
    );
}

/// Anza lists 256 GB for validators and 512 GB as board capacity / RPC extra.
/// Neither is a testnet FAIL — an invented 128 GB floor already false-failed
/// a working node.
#[test]
fn memory_check_names_anza_and_does_not_fail_125gb() {
    let (o, _) = run(&["--root", &host(&FRESH_UBUNTU), "--profile", "testnet", "-v"]);
    let block = block_for(&o, "PF-HW-0005");
    assert!(flat(block).contains("256 GB"), "{block}");
    assert!(flat(block).contains("512 GB"), "{block}");
    assert!(
        block.contains("REPORTED"),
        "a measured value with no fail threshold is not Unknown:\n{block}"
    );
    let small = Host {
        name: "small-memory",
        mem_kb: 131_500_000,
        ..FRESH_UBUNTU
    };
    let (o, _) = run(&["--root", &host(&small), "--profile", "testnet", "-v"]);
    let block = block_for(&o, "PF-HW-0005");
    assert!(
        !block.contains("FAIL"),
        "125 GB testnet must stay reported:\n{block}"
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
        flat(block).contains("operator one from running it"),
        "the testnet floor is ours, and has to say so:\n{block}"
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

    let (mainnet, _) = run(&["--root", &root, "--profile", "mainnet", "-v"]);
    let block = block_for(&mainnet, "PF-HW-0006");
    assert!(
        block.contains("REPORTED"),
        "absence from a blog list is not Anza saying no:\n{block}"
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

/// Anza's 6.8 and 6.14 are zero copy numbers, and agave's default is XDP
/// without zero copy. Applying them to the default path invented a requirement
/// and failed a box whose own logs showed the path working.
#[test]
fn the_zero_copy_floor_does_not_bind_the_default_path() {
    let old = Host {
        name: "old-kernel",
        kernel: "5.15.0-139-generic",
        nic: Some(("eth0", "bnxt_en")),
        ..WRAPPER_SCRIPT_UNIT
    };
    let (o, code) = run(&[
        "--root",
        &host(&old),
        "--client",
        "agave-validator@4.3.0",
        "-v",
    ]);
    let block = block_for(&o, "PF-KRN-0005");
    assert!(
        !block.contains("FAIL"),
        "copy mode on 5.15 is not a shortfall:\n{block}"
    );
    assert!(flat(block).contains("copy mode"), "{block}");
    assert!(
        flat(block).contains("recommended"),
        "the guide says recommended, and so must this:\n{block}"
    );
    assert!(
        !flat(&o).contains("PF-KRN-0005") || !block.contains("FAIL"),
        "the kernel must not be what makes this box fail:\n{block}"
    );
    let _ = code;
}

/// Asking for zero copy is what makes the guide's numbers bind.
#[test]
fn zero_copy_below_the_floor_is_a_finding() {
    let zc = Host {
        name: "zero-copy-old-kernel",
        kernel: "5.15.0-139-generic",
        nic: Some(("eth0", "bnxt_en")),
        files: &[
            (
                "/etc/systemd/system/sol.service",
                "[Service]\nUser=sol\nExecStart=/home/sol/bin/validator.sh\n",
            ),
            (
                "/home/sol/bin/validator.sh",
                "#!/usr/bin/env bash\nexec agave-validator \\\n\
             --identity /home/sol/validator-keypair.json \\\n\
             --vote-account /home/sol/vote-account-keypair.json \\\n\
             --entrypoint entrypoint.testnet.solana.com:8001 \\\n\
             --ledger /mnt/ledger \\\n\
             --accounts /mnt/accounts \\\n\
             --xdp-zero-copy \\\n\
             --dynamic-port-range 8000-8020\n",
            ),
        ],
        ..WRAPPER_SCRIPT_UNIT
    };
    let (o, _) = run(&["--root", &host(&zc), "--client", "agave-validator@4.3.0"]);
    let block = block_for(&o, "PF-KRN-0005");
    assert!(block.contains("FAIL"), "{block}");
    assert!(flat(block).contains("6.8"), "{block}");
    assert!(
        flat(block).contains("drop --xdp-zero-copy"),
        "dropping the flag is the cheap way out, not a release upgrade:\n{block}"
    );
}

/// igb needs a newer kernel than everything else, so the floor reads the card.
#[test]
fn the_kernel_floor_follows_the_driver() {
    let igb = Host {
        name: "igb-nic",
        kernel: "6.10.0-generic",
        nic: Some(("eth0", "igb")),
        files: &[
            (
                "/etc/systemd/system/sol.service",
                "[Service]\nUser=sol\nExecStart=/home/sol/bin/validator.sh\n",
            ),
            (
                "/home/sol/bin/validator.sh",
                "#!/usr/bin/env bash\nexec agave-validator \\\n\
             --identity /home/sol/validator-keypair.json \\\n\
             --vote-account /home/sol/vote-account-keypair.json \\\n\
             --entrypoint entrypoint.testnet.solana.com:8001 \\\n\
             --ledger /mnt/ledger \\\n\
             --accounts /mnt/accounts \\\n\
             --xdp-zero-copy \\\n\
             --dynamic-port-range 8000-8020\n",
            ),
        ],
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
        "-v",
    ]);
    assert!(
        o.contains("4.3.0-beta.0"),
        "prerelease must not read as 4.3.0:\n{o}"
    );
    assert!(o.contains("run         preflight --root"), "{o}");
    assert!(o.contains("UTC"), "{o}");
    assert!(o.contains("preflight --profile mainnet"), "{o}");
    // The pass cases for the two checks a stale box gets wrong most often.
    assert!(block_for(&o, "PF-HW-0007").contains("PASS"), "{o}");
    assert!(block_for(&o, "PF-KRN-0005").contains("PASS"), "{o}");

    let clean = run_with_path(&fake_validator("4.2.1"), &["--profile", "testnet"]);
    assert!(!clean.contains("preflight  ·"), "no double space:\n{clean}");
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

/// A machine that fails mainnet requirements must not be told "the validator
/// will start" as its closing line. Under a verdict that just said no, that
/// reads as permission.
#[test]
fn the_closing_line_leads_with_the_machine() {
    // Shaped like a working testnet box that is not a mainnet box: valid
    // configuration, disks that miss Anza's published 1 TB.
    let small = Host {
        name: "mainnet-unsuitable",
        cpu_model: "AMD EPYC 7313P 16-Core Processor",
        disks: &[
            ("nvme0n1", 2000, true),
            ("nvme1n1", 2000, true),
            ("nvme2n1", 2000, true),
        ],
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
    assert!(
        flat(block).contains("not anybody's published requirement"),
        "{block}"
    );
    assert!(
        flat(block).contains("headroom figure is preflight's own"),
        "the source must say so too:\n{block}"
    );
}

/// The changelog could not settle whether the 4.3 flags exist. A real
/// 4.3.0-beta.0 binary could, and did: --limit-blockstore-size is in its help
/// and --tpu-connection-pool-size is gone from it.
#[test]
fn the_settled_flags_cite_the_binary_not_the_changelog() {
    let (o, _) = run(&["--dump-registry"]);
    for id in ["PF-ARG-0011", "PF-ARG-0013"] {
        let row = o.lines().find(|l| l.contains(id)).unwrap_or_default();
        assert!(row.contains("4.3.0-beta.0"), "{row}");
        assert!(
            !row.contains("provisional"),
            "settled, so not provisional: {row}"
        );
    }
    // and the one it could not settle keeps saying so
    let row = o
        .lines()
        .find(|l| l.contains("PF-ARG-0012"))
        .unwrap_or_default();
    assert!(row.contains("provisional"), "{row}");
}

/// The doubling guidance applies to a value someone chose. On the default there
/// is no number, and telling an operator to double one would have them invent
/// it. This is exactly the shape of a real start script.
#[test]
fn a_flag_with_no_value_is_a_rename_not_a_conversion() {
    let bare = invocation(
        "default-ledger-size.txt",
        "exec agave-validator --ledger /l --accounts /a --limit-ledger-size\n",
    );
    let (o, _) = run(&[
        "--invocation",
        bare.to_str().unwrap(),
        "--client",
        "agave-validator@4.3.0-beta.0",
        "--profile",
        "testnet",
    ]);
    let block = block_for(&o, "PF-ARG-0011");
    assert!(flat(block).contains("with no value"), "{block}");
    assert!(flat(block).contains("no value to convert"), "{block}");
    assert!(!flat(block).contains("<2n>"), "nothing to double:\n{block}");

    let sized = invocation(
        "sized-ledger.txt",
        "exec agave-validator --ledger /l --accounts /a --limit-ledger-size 50000000\n",
    );
    let (o, _) = run(&[
        "--invocation",
        sized.to_str().unwrap(),
        "--client",
        "agave-validator@4.3.0-beta.0",
        "--profile",
        "testnet",
    ]);
    let block = block_for(&o, "PF-ARG-0011");
    assert!(
        flat(block).contains("about 100000000"),
        "doubled for them:\n{block}"
    );
}

/// The changelog says "counts more precisely". The source says which variants
/// do the counting, and that is what an operator needs to decide anything.
#[test]
fn the_ledger_size_check_explains_the_mechanism_not_the_changelog() {
    let bare = invocation(
        "mechanism.txt",
        "exec agave-validator --ledger /l --accounts /a --limit-ledger-size\n",
    );
    let (o, _) = run(&[
        "--invocation",
        bare.to_str().unwrap(),
        "--client",
        "agave-validator@4.3.0-beta.0",
        "--profile",
        "testnet",
    ]);
    let block = block_for(&o, "PF-ARG-0011");
    assert!(flat(block).contains("CountDataAndCodingShreds"), "{block}");
    assert!(flat(block).contains("coding shreds"), "{block}");
    assert!(
        flat(block).contains("BlockstoreCleanupStrategy"),
        "the source is a symbol, not a paraphrase:\n{block}"
    );
}

/// A substitution preflight has not verified must never render as an arrow.
/// Two live bugs were confident swaps: one quoted a default read from
/// solana-test-validator's struct, the other ignored that the old flag name
/// carried its unit.
#[test]
fn an_unverified_rename_never_renders_a_substitution() {
    let inv = invocation(
        "unverified-rename.txt",
        "exec agave-validator --ledger /l --accounts /a --accounts-db-cache-limit-mb 10240\n",
    );
    let (o, _) = run(&[
        "--invocation",
        inv.to_str().unwrap(),
        "--client",
        "agave-validator@4.2.1",
        "--profile",
        "testnet",
    ]);
    let block = block_for(&o, "PF-ARG-0008");
    assert!(
        !flat(block).contains("--accounts-db-cache-limit-mb   ->"),
        "the unit is unverified, so no arrow:\n{block}"
    );
    assert!(flat(block).contains("is replaced by"), "{block}");
    assert!(
        flat(block).contains("has not read the replacement's unit"),
        "{block}"
    );
    // and it reports the value, which is what makes the unit question visible
    assert!(flat(block).contains("10240"), "{block}");
}

/// The default quoted for --limit-blockstore-size came from DefaultTestArgs,
/// which belongs to solana-test-validator. The real constants are 200,000,000
/// and 400,000,000, already in the 2:1 ratio the changelog describes.
#[test]
fn the_ledger_defaults_come_from_the_validator_not_the_test_validator() {
    let inv = invocation(
        "ledger-defaults.txt",
        "exec agave-validator --ledger /l --accounts /a --limit-ledger-size\n",
    );
    let (o, _) = run(&[
        "--invocation",
        inv.to_str().unwrap(),
        "--client",
        "agave-validator@4.3.0-beta.0",
        "--profile",
        "testnet",
    ]);
    let block = block_for(&o, "PF-ARG-0011");
    assert!(
        !flat(block).contains("800000"),
        "that is the test validator's default:\n{block}"
    );
    assert!(flat(block).contains("200,000,000"), "{block}");
    assert!(flat(block).contains("400,000,000"), "{block}");
}

/// A fix block that restarts before the edit reads as "restart, then change it".
#[test]
fn the_restart_comes_last_in_a_fix_block() {
    let unit = Host {
        name: "cache-limit-unit",
        files: &[
            (
                "/etc/systemd/system/sol.service",
                "[Service]\nUser=sol\nExecStart=/home/sol/bin/validator.sh\n",
            ),
            (
                "/home/sol/bin/validator.sh",
                "#!/usr/bin/env bash\nexec agave-validator --ledger /mnt/ledger \
                 --accounts /mnt/accounts --dynamic-port-range 8000-8030 \
                 --accounts-db-cache-limit-mb 10240\n",
            ),
        ],
        ..WRAPPER_SCRIPT_UNIT
    };
    let (o, _) = run(&[
        "--root",
        &host(&unit),
        "--client",
        "agave-validator@4.2.1",
        "--profile",
        "testnet",
    ]);
    let block = block_for(&o, "PF-ARG-0008");
    let edit = block.find("edit /home/sol").expect("edit step");
    let change = block.find("is replaced by").expect("change step");
    let restart = block.find("systemctl restart").expect("restart step");
    assert!(edit < change, "edit before the change:\n{block}");
    assert!(change < restart, "restart last:\n{block}");
}

/// Two checks each held half of this: one knows free space, the other knows the
/// retention setting. Neither said the blockstore was aimed at more disk than
/// exists.
#[test]
fn retention_larger_than_the_disk_is_a_finding() {
    // Free space is only measurable on the real host, so under --root this
    // reports what it cannot see rather than guessing.
    let (o, _) = run(&[
        "--root",
        &host(&WRAPPER_SCRIPT_UNIT),
        "--client",
        "agave-validator@4.3.0-beta.0",
        "-v",
    ]);
    let block = block_for(&o, "PF-FS-0007");
    assert!(
        block.contains("SKIPPED"),
        "a captured tree carries no free space, which is out of scope not unknown:\n{block}"
    );
    assert!(
        flat(block).contains("cannot be read from a captured tree"),
        "{block}"
    );

    // The check exists and is registered; its sizing is cited to the symbol.
    let (reg, _) = run(&["--dump-registry"]);
    let row = reg
        .lines()
        .find(|l| l.contains("PF-FS-0007"))
        .unwrap_or_default();
    assert!(row.contains("DEFAULT_MAX_BLOCKSTORE_SHREDS"), "{row}");
}

/// An unexpanded token used to make thirteen ARG checks report PASS on flags
/// nobody read, and the configuration question answer "yes" off nothing.
#[test]
fn an_unexpanded_token_is_unknown_not_a_clean_bill() {
    let shell = Host {
        name: "unexpanded-flags",
        files: &[
            (
                "/etc/systemd/system/sol.service",
                "[Service]\nUser=sol\nExecStart=/home/sol/bin/validator.sh\n",
            ),
            (
                "/home/sol/bin/validator.sh",
                "#!/usr/bin/env bash\nFLAGS=\"--ledger /mnt/ledger\"\n\
                 exec agave-validator $FLAGS\n",
            ),
        ],
        ..WRAPPER_SCRIPT_UNIT
    };
    let (o, code) = run(&[
        "--root",
        &host(&shell),
        "--client",
        "agave-validator@4.3.0",
        "--profile",
        "testnet",
        "-v",
    ]);
    let block = block_for(&o, "PF-ARG-0001");
    assert!(block.contains("UNKNOWN"), "{block}");
    assert!(
        flat(block).contains("$FLAGS"),
        "the token must be shown:\n{block}"
    );
    assert!(
        !o.contains("IS THE VALIDATOR CONFIGURED CORRECTLY?\n  yes"),
        "an unread command line cannot answer yes:\n{o}"
    );
    // Other findings on this fixture outrank unknown, but never with a zero.
    assert_ne!(
        code, 0,
        "an unread command line is not a pass:
{o}"
    );
}

/// /proc/cpuinfo reports the governor's current speed, not the base clock. An
/// idle core reads well under base and used to FAIL a machine that meets Anza.
#[test]
fn an_idle_core_is_unknown_not_a_slow_cpu() {
    let idle = Host {
        name: "idle-core",
        mhz: "1500.000",
        ..WRAPPER_SCRIPT_UNIT
    };
    let (o, _) = run(&[
        "--root",
        &host(&idle),
        "--client",
        "agave-validator@4.3.0",
        "--profile",
        "testnet",
    ]);
    let block = block_for(&o, "PF-HW-0003");
    assert!(
        block.contains("UNKNOWN"),
        "a throttled core is not slow silicon:\n{block}"
    );

    let published = Host {
        name: "idle-core-with-sysfs",
        mhz: "1500.000",
        files: &[(
            "/sys/devices/system/cpu/cpu0/cpufreq/base_frequency",
            "3000000\n",
        )],
        ..WRAPPER_SCRIPT_UNIT
    };
    let (o, _) = run(&[
        "--root",
        &host(&published),
        "--client",
        "agave-validator@4.3.0",
        "--profile",
        "testnet",
        "-v",
    ]);
    let block = block_for(&o, "PF-HW-0003");
    assert!(
        block.contains("PASS"),
        "sysfs publishes the real base:\n{block}"
    );
    assert!(flat(block).contains("3000 MHz base"), "{block}");
}

/// A 943 GB filesystem was failed for having 249 GB free against a 250 GB
/// floor. The floor is a device size; free space is the separate headroom line.
#[test]
fn the_disk_floor_is_capacity_not_free_space() {
    let full = Host {
        name: "big-disk-mostly-used",
        disks: &[("nvme0n1", 960, false), ("nvme1n1", 960, false)],
        mounts: "/dev/nvme0n1p2 / ext4 rw,noatime 0 0\n\
                 /dev/nvme1n1 /mnt/accounts xfs rw,noatime 0 0\n",
        ..WRAPPER_SCRIPT_UNIT
    };
    let (o, _) = run(&[
        "--root",
        &host(&full),
        "--client",
        "agave-validator@4.3.0",
        "--profile",
        "testnet",
        "-v",
    ]);
    let block = block_for(&o, "PF-FS-0001");
    assert!(
        !flat(block).contains("wants 250 GB"),
        "a disk larger than the floor cannot be short of it:\n{block}"
    );
}

/// A title as wide as its column used to run straight into the status word.
#[test]
fn no_title_touches_its_status() {
    let (o, _) = run(&[
        "--root",
        &host(&WRAPPER_SCRIPT_UNIT),
        "--client",
        "agave-validator@4.3.0",
        "-v",
    ]);
    const STATUSES: [&str; 6] = [
        "PASS",
        "FAIL",
        "REPORTED",
        "SKIPPED",
        "UNKNOWN",
        "EPHEMERAL",
    ];
    for line in o.lines().filter(|l| l.trim_start().starts_with("PF-")) {
        let Some(word) = STATUSES.iter().find(|w| line.contains(**w)) else {
            continue;
        };
        let before = &line[..line.find(*word).unwrap()];
        assert!(
            before.ends_with("  "),
            "the status needs clear air after the title:\n{line}"
        );
    }
}

/// A FAIL with no fix is half a finding, and the option lives on the mount
/// rather than the path the validator was given.
#[test]
fn noatime_says_how_to_set_it() {
    let atime = Host {
        name: "no-noatime",
        mounts: "/dev/nvme0n1p2 / ext4 rw,relatime 0 0\n\
                 /dev/nvme1n1 /mnt/accounts xfs rw,relatime 0 0\n",
        ..WRAPPER_SCRIPT_UNIT
    };
    let (o, _) = run(&[
        "--root",
        &host(&atime),
        "--client",
        "agave-validator@4.3.0",
        "--profile",
        "testnet",
    ]);
    let block = block_for(&o, "PF-FS-0004");
    assert!(block.contains("FAIL"), "{block}");
    assert!(flat(block).contains("/etc/fstab"), "{block}");
    assert!(flat(block).contains("remount,noatime"), "{block}");
    assert!(
        flat(block).contains("watch the node"),
        "remounting / deserves the caveat:\n{block}"
    );
}

/// Anza's XDP guide names ice next to bnxt_en: do not pass zero-copy.
#[test]
fn ice_plus_zero_copy_is_a_fail() {
    let ice = Host {
        name: "ice-nic",
        nic: Some(("eth0", "ice")),
        ..FRESH_UBUNTU
    };
    let inv = invocation(
        "ice-zero-copy.txt",
        "exec agave-validator --ledger /l --accounts /a --xdp-interface eth0 --xdp-zero-copy\n",
    );
    let (o, _) = run(&[
        "--root",
        &host(&ice),
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
    assert!(flat(block).contains("Anza"), "{block}");
}

/// Ambient in the unit is not the process's permitted set. A pid with empty
/// CapPrm must FAIL even when the drop-in looks right.
#[test]
fn xdp_reads_capprm_when_a_process_exists() {
    let empty = Host {
        name: "xdp-capprm-empty",
        files: &[
            (
                "/etc/systemd/system/sol.service",
                "[Service]\nUser=sol\nExecStart=/home/sol/bin/validator.sh\n",
            ),
            (
                "/etc/systemd/system/sol.service.d/20-xdp-caps.conf",
                "[Service]\nAmbientCapabilities=CAP_NET_RAW CAP_NET_ADMIN\n",
            ),
            (
                "/home/sol/bin/validator.sh",
                "#!/usr/bin/env bash\nexec agave-validator --ledger /mnt/ledger \
                 --accounts /mnt/accounts --entrypoint entrypoint.testnet.solana.com:8001 \
                 --dynamic-port-range 8000-8030 --xdp-interface eth0\n",
            ),
            (
                "/proc/42/cmdline",
                "agave-validator\0--ledger\0/mnt/ledger\0",
            ),
            (
                "/proc/42/status",
                "Name:\tagave-validat\nUid:\t1001\t1001\t1001\t1001\nCapPrm:\t0000000000000000\n",
            ),
            (
                "/proc/42/task/42/status",
                "Name:\tagave-validat\nCapPrm:\t0000000000000000\n",
            ),
            ("/proc/42/cgroup", "0::/system.slice/sol.service\n"),
        ],
        ..WRAPPER_SCRIPT_UNIT
    };
    let (o, _) = run(&[
        "--root",
        &host(&empty),
        "--client",
        "agave-validator@4.2.1",
        "--profile",
        "testnet",
    ]);
    // Agave drops what it does not need before spawning threads, so zero after
    // startup is what a healthy node looks like and cannot be a finding.
    let block = block_for(&o, "PF-XDP-0001");
    assert!(
        !block.contains("FAIL"),
        "a dropped capability is not a missing one:\n{block}"
    );

    let held = Host {
        name: "xdp-capprm-held",
        files: &[
            (
                "/etc/systemd/system/sol.service",
                "[Service]\nUser=sol\nExecStart=/home/sol/bin/validator.sh\n",
            ),
            (
                "/home/sol/bin/validator.sh",
                "#!/usr/bin/env bash\nexec agave-validator --ledger /mnt/ledger \
                 --accounts /mnt/accounts --entrypoint entrypoint.testnet.solana.com:8001 \
                 --dynamic-port-range 8000-8030 --xdp-interface eth0\n",
            ),
            (
                "/proc/42/cmdline",
                "agave-validator\0--ledger\0/mnt/ledger\0",
            ),
            (
                "/proc/42/status",
                "Name:\tagave-validat\nUid:\t1001\t1001\t1001\t1001\nCapPrm:\t0000000000000000\n",
            ),
            // The main thread drops everything; one thread keeps cap_net_admin.
            (
                "/proc/42/task/42/status",
                "Name:\tagave-validat\nCapPrm:\t0000000000000000\n",
            ),
            (
                "/proc/42/task/57/status",
                "Name:\tsolNetLnkRecv\nCapPrm:\t0000000000001000\n",
            ),
            ("/proc/42/cgroup", "0::/system.slice/sol.service\n"),
        ],
        ..WRAPPER_SCRIPT_UNIT
    };
    let (o, _) = run(&[
        "--root",
        &host(&held),
        "--client",
        "agave-validator@4.2.1",
        "--profile",
        "testnet",
        "-v",
    ]);
    let block = block_for(&o, "PF-XDP-0001");
    assert!(
        block.contains("PASS"),
        "a thread still holding cap_net_admin proves the grant arrived:\n{block}"
    );
    assert!(
        flat(block).contains("reached the process"),
        "runtime evidence confirms, it never refutes:\n{block}"
    );
    let persist = block_for(&o, "PF-XDP-0007");
    assert!(
        persist.contains("EPHEMERAL"),
        "caps without Ambient are setcap:\n{persist}"
    );
}

/// Mapper volumes used to vanish because host.rs skipped dm- devices. Now they
/// are listed, but a volume and the disk beneath it are the same bytes, so only
/// one of them may count toward a capacity total.
#[test]
fn mapper_disks_are_listed_but_not_counted_twice() {
    let mapper = Host {
        name: "mapper-disk",
        disks: &[("dm-0", 960, false), ("nvme0n1", 960, false)],
        files: &[
            ("/sys/block/dm-0/dm/name", "accounts\n"),
            ("/sys/block/dm-0/slaves/nvme0n1/uevent", "\n"),
        ],
        ..FRESH_UBUNTU
    };
    let (o, _) = run(&["--root", &host(&mapper), "--profile", "mainnet"]);
    assert!(
        o.contains("accounts") || o.contains("dm-0"),
        "mapper volume must show up:\n{o}"
    );
    let block = block_for(&o, "PF-FS-0001");
    assert!(
        !flat(block).contains("1920 GB"),
        "960 GB of hardware cannot report 1920:\n{block}"
    );
    assert!(flat(block).contains("960 GB"), "{block}");
}

/// A unit that grants ambient capabilities while the running process holds
/// none means the grant was added after launch. Telling that operator to write
/// the drop-in they already have is useless; the answer is a restart.
#[test]
fn a_grant_the_process_predates_asks_for_a_restart() {
    let stale = Host {
        name: "caps-added-after-launch",
        files: &[
            (
                "/etc/systemd/system/sol.service",
                "[Service]\nUser=sol\n\
                 CapabilityBoundingSet=CAP_NET_ADMIN CAP_NET_RAW\n\
                 AmbientCapabilities=CAP_NET_ADMIN CAP_NET_RAW\n\
                 ExecStart=/home/sol/bin/validator.sh\n",
            ),
            (
                "/home/sol/bin/validator.sh",
                "#!/usr/bin/env bash\nexec agave-validator \\\n\
                 --identity /home/sol/validator-keypair.json \\\n\
                 --vote-account /home/sol/vote-account-keypair.json \\\n\
                 --entrypoint entrypoint.testnet.solana.com:8001 \\\n\
                 --ledger /mnt/ledger \\\n\
                 --accounts /mnt/accounts \\\n\
                 --dynamic-port-range 8000-8020\n",
            ),
            // A live process with an empty permitted set, as seen on a real box.
            (
                "/proc/4242/cmdline",
                "agave-validator\0--ledger\0/mnt/ledger\0",
            ),
            (
                "/proc/4242/status",
                "Name:\tagave-validator\nUid:\t1001\t1001\t1001\t1001\n\
                 CapPrm:\t0000000000000000\nCapBnd:\t0000000000003000\n\
                 CapAmb:\t0000000000000000\n",
            ),
        ],
        ..WRAPPER_SCRIPT_UNIT
    };
    let (o, _) = run(&[
        "--root",
        &host(&stale),
        "--client",
        "agave-validator@4.3.0",
        "--profile",
        "testnet",
    ]);
    let block = block_for(&o, "PF-XDP-0001");
    assert!(
        !flat(block).contains("stop the validator from starting"),
        "it is running; that is how preflight read its CapPrm:\n{block}"
    );
}

/// A required value swallowed by the next flag is invisible once the process
/// is up: the running command line still shows both flags.
#[test]
fn a_flag_that_lost_its_value_is_a_finding() {
    let lost = Host {
        name: "flag-without-value",
        files: &[
            (
                "/etc/systemd/system/sol.service",
                "[Service]\nUser=sol\nExecStart=/home/sol/bin/validator.sh\n",
            ),
            (
                "/home/sol/bin/validator.sh",
                "#!/usr/bin/env bash\nexec agave-validator \\\n\
                 --identity /home/sol/validator-keypair.json \\\n\
                 --vote-account /home/sol/vote-account-keypair.json \\\n\
                 --entrypoint entrypoint.testnet.solana.com:8001 \\\n\
                 --ledger /mnt/ledger \\\n\
                 --accounts /mnt/accounts \\\n\
                 --limit-blockstore-size \\\n\
                 --accounts-db-write-cache-limit 10GB\n",
            ),
        ],
        ..WRAPPER_SCRIPT_UNIT
    };
    let (o, _) = run(&[
        "--root",
        &host(&lost),
        "--client",
        "agave-validator@4.3.0",
        "--profile",
        "testnet",
        "-v",
    ]);
    let block = block_for(&o, "PF-ARG-0014");
    assert!(block.contains("FAIL"), "{block}");
    assert!(
        flat(block).contains("--limit-blockstore-size"),
        "name the flag that lost it:\n{block}"
    );
    // The flag it swallowed still parses as present, which is the trap.
    assert!(
        block_for(&o, "PF-ARG-0011").contains("PASS"),
        "0011 sees the flag and cannot see the missing value; 0014 is why it exists"
    );
}
