//! What preflight knows before any check runs.
//!
//! Assembles the host facts, the validator invocation, the client and its
//! version, and the profile being judged against. Every check reads from this
//! rather than touching the system itself.

use crate::{
    argv::{self, Invocation},
    host::ClientVersion,
    host::Rootfs,
    model::{ClientKind, Profile},
};
use std::{path::PathBuf, process::Command};

/// How the client version was obtained, so the header can say it out loud.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionSource {
    Flag,
    Executed(String),
    Undetected(&'static str),
}

pub struct Ctx {
    pub fs: Rootfs,
    pub profile: Profile,
    pub profile_reason: String,
    /// What the box itself points to, kept even when a flag says otherwise, so
    /// a forced profile that contradicts the machine can say so.
    pub inferred_profile: Option<(Profile, String)>,
    /// Whether the box itself named a cluster, or preflight had to guess.
    pub profile_confident: bool,
    pub client: ClientKind,
    pub version: Option<ClientVersion>,
    pub version_source: VersionSource,
    pub invocation: Option<Invocation>,
    pub invocation_trail: Vec<String>,
    pub validator_pid: Option<String>,
    /// Whether this host has a validator at all: a binary, a unit, or a running
    /// process. Absent one, checks are Skipped rather than Unknown — there is
    /// nothing to probe, which is different from failing to probe it.
    pub validator_present: bool,
    pub os: Option<String>,
    pub kernel: Option<String>,
    pub arch: Option<String>,
    pub virt: Option<String>,
    pub uid: u32,
    pub facts: crate::host::Facts,
}

fn os_release(fs: &Rootfs) -> Option<String> {
    let text = fs.read("/etc/os-release").ok()?;
    let get = |k: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(k))
            .map(|v| v.trim_matches('"').to_string())
    };
    get("PRETTY_NAME=").or_else(|| get("NAME="))
}

fn kernel(fs: &Rootfs) -> Option<String> {
    fs.read_trim("/proc/sys/kernel/osrelease")
}

fn arch(fs: &Rootfs) -> Option<String> {
    if fs.is_prefixed() {
        fs.read("/proc/cpuinfo").ok().and_then(|c| {
            c.lines()
                .find(|l| l.starts_with("flags") || l.starts_with("Features"))
                .map(|l| {
                    if l.starts_with("flags") {
                        "x86_64".to_string()
                    } else {
                        "aarch64".to_string()
                    }
                })
        })
    } else {
        Some(std::env::consts::ARCH.to_string())
    }
}

fn virt(fs: &Rootfs) -> Option<String> {
    if fs.exists("/etc/lima-version") || fs.exists("/mnt/lima-cidata") {
        return Some("Lima VM (qemu)".into());
    }
    let name = fs.read_trim("/sys/class/dmi/id/product_name")?;
    if name.is_empty() { None } else { Some(name) }
}

fn detect_client(inv: Option<&Invocation>, fs: &Rootfs) -> ClientKind {
    let base = |p: &str| p.rsplit('/').next().unwrap_or(p).to_string();
    if let Some(i) = inv {
        return match base(&i.program).as_str() {
            "agave-validator" | "solana-validator" => ClientKind::AgaveValidator,
            "solana-test-validator" => ClientKind::TestValidator,
            "fdctl" => ClientKind::Firedancer,
            _ => ClientKind::Unknown,
        };
    }
    for d in [
        "/usr/local/bin",
        "/usr/bin",
        "/root/.local/share/solana/install/active_release/bin",
    ] {
        for p in fs.list(d) {
            match p.file_name().and_then(|f| f.to_str()) {
                Some("agave-validator") => return ClientKind::AgaveValidator,
                Some("solana-test-validator") => return ClientKind::TestValidator,
                _ => {}
            }
        }
    }
    ClientKind::Unknown
}

/// The inferred profile, why, and whether the box actually said so.
///
/// A bare machine could be headed anywhere, and a voting validator whose
/// entrypoint names no cluster is a guess. Those are worth asking about. An
/// entrypoint that says testnet is not.
fn infer_profile(inv: Option<&Invocation>, client: ClientKind) -> (Profile, String, bool) {
    let Some(i) = inv else {
        return (
            Profile::Local,
            "no validator invocation resolved".into(),
            false,
        );
    };
    if client == ClientKind::TestValidator {
        return (
            Profile::Local,
            "client is solana-test-validator".into(),
            true,
        );
    }
    if !i.has("--vote-account") {
        return (
            Profile::Local,
            "invocation has no --vote-account".into(),
            true,
        );
    }
    let entry = i.value("--entrypoint").unwrap_or_default();
    match (entry.contains("testnet"), entry.contains("mainnet")) {
        (true, _) => (Profile::Testnet, format!("entrypoint {entry}"), true),
        (_, true) => (Profile::Mainnet, format!("entrypoint {entry}"), true),
        _ => (
            Profile::Testnet,
            "voting validator, cluster not identified from entrypoint".into(),
            false,
        ),
    }
}

pub struct CtxOptions {
    pub root: Option<PathBuf>,
    pub profile: Option<Profile>,
    pub invocation_file: Option<PathBuf>,
    pub client_override: Option<String>,
    pub no_exec: bool,
}

/// Resolve the validator binary, then ask it for its version.
///
/// This is the one place preflight executes anything. It runs the same binary
/// the operator already runs, with `--version`, unprivileged, and prints the
/// command in the header so it is never a surprise. `--no-exec` turns it off.
fn detect_version(
    ctx_fs: &Rootfs,
    inv: Option<&Invocation>,
    no_exec: bool,
) -> (Option<ClientVersion>, VersionSource) {
    if no_exec {
        return (None, VersionSource::Undetected("--no-exec was passed"));
    }
    if ctx_fs.is_prefixed() {
        return (
            None,
            VersionSource::Undetected(
                "--root is set, so the host's binary is not this host's validator",
            ),
        );
    }
    let Some(inv) = inv else {
        return (
            None,
            VersionSource::Undetected("no validator invocation resolved"),
        );
    };

    // argv[0] is whatever the caller passed to execve, so a process can name
    // itself agave-validator and be anything. Executing that as root would
    // hand it uid 0, so the version stays undetected instead.
    if unsafe { libc_getuid() } == 0 {
        return (
            None,
            VersionSource::Undetected(
                "running as root, and preflight will not execute a binary it found by name. \
                 Pass --client <name>@<version>, or run this unprivileged",
            ),
        );
    }

    // A resolved pid is the better answer, and its exe is a kernel-maintained
    // link rather than a PATH lookup. Take it or nothing: falling through to
    // argv[0] would search PATH for a name the process chose for itself.
    let candidate = match &inv.pid {
        Some(pid) => match std::fs::read_link(format!("/proc/{pid}/exe")) {
            Ok(exe) => {
                let name = exe
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                match crate::argv::is_validator_bin(&name) {
                    true => exe.to_string_lossy().to_string(),
                    false => {
                        return (
                            None,
                            VersionSource::Undetected(
                                "the running process does not point at a known validator binary",
                            ),
                        );
                    }
                }
            }
            Err(_) => {
                return (
                    None,
                    VersionSource::Undetected("cannot read the process exe"),
                );
            }
        },
        None => inv.program.clone(),
    };

    match run_version(&candidate) {
        Some(text) => match ClientVersion::parse(&text) {
            Some(v) => (
                Some(v),
                VersionSource::Executed(format!("{candidate} --version")),
            ),
            None => (
                None,
                VersionSource::Undetected("the binary did not report a version preflight reads"),
            ),
        },
        None => (
            None,
            VersionSource::Undetected("could not run the validator binary with --version"),
        ),
    }
}

/// A hung binary must not hang a read-only tool, so this waits a few seconds
/// and gives up rather than blocking on output.
fn run_version(bin: &str) -> Option<String> {
    use std::process::Stdio;
    let mut child = Command::new(bin)
        .arg("--version")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Ok(None) => {
                let _ = child.kill();
                return None;
            }
            Err(_) => return None,
        }
    }
    let out = child.wait_with_output().ok()?;
    Some(String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr))
}

impl Ctx {
    pub fn probe(opts: CtxOptions) -> Ctx {
        let fs = Rootfs::new(opts.root);

        let (invocation, invocation_trail) = match &opts.invocation_file {
            Some(p) => match std::fs::read_to_string(p) {
                Ok(t) => match argv::from_text(&t) {
                    Ok(i) => {
                        let t = i.trail.clone();
                        (Some(i), t)
                    }
                    Err(t) => (None, t),
                },
                Err(e) => (None, vec![format!("cannot read {}: {e}", p.display())]),
            },
            None => match argv::resolve(&fs) {
                Ok(i) => {
                    let t = i.trail.clone();
                    (Some(i), t)
                }
                Err(t) => (None, t),
            },
        };

        let mut client = detect_client(invocation.as_ref(), &fs);
        let mut version = None;

        if let Some(o) = &opts.client_override {
            let (name, ver) = o.split_once('@').unwrap_or((o.as_str(), ""));
            client = match name {
                "agave-validator" | "agave" => ClientKind::AgaveValidator,
                "solana-test-validator" | "test-validator" => ClientKind::TestValidator,
                "firedancer" | "fdctl" => ClientKind::Firedancer,
                _ => ClientKind::Unknown,
            };
            version = ClientVersion::parse(ver);
        }

        let version_source;
        if version.is_none() {
            let (v, src) = detect_version(&fs, invocation.as_ref(), opts.no_exec);
            version = v;
            version_source = src;
        } else {
            version_source = VersionSource::Flag;
        }

        let validator_pid = invocation.as_ref().and_then(|i| i.pid.clone());
        let validator_present =
            invocation.is_some() || client != ClientKind::Unknown || opts.invocation_file.is_some();
        let (guess, guess_reason, confident) = infer_profile(invocation.as_ref(), client);
        let inferred = (guess, guess_reason);
        let (profile, profile_reason) = match opts.profile {
            Some(p) => (p, "set with --profile".to_string()),
            None => inferred.clone(),
        };
        let inferred_profile = match opts.profile {
            Some(p) if p != inferred.0 && validator_present => Some(inferred),
            _ => None,
        };

        Ctx {
            os: os_release(&fs),
            kernel: kernel(&fs),
            arch: arch(&fs),
            virt: virt(&fs),
            uid: unsafe { libc_getuid() },
            facts: crate::host::gather(&fs),
            fs,
            profile,
            profile_reason,
            inferred_profile,
            profile_confident: confident,
            client,
            version,
            invocation,
            invocation_trail,
            version_source,
            validator_pid,
            validator_present,
        }
    }

    /// A validator runs on Linux. Without /proc there is nothing for the host
    /// layers to read, and saying that once beats repeating it per check.
    pub fn is_linux(&self) -> bool {
        self.fs.exists("/proc/sys/kernel/osrelease")
    }

    pub fn inv(&self) -> Option<&Invocation> {
        self.invocation.as_ref()
    }

    /// True when the detected client is at or above the given release. False
    /// when it predates it. Callers treat a missing version as "cannot tell"
    /// and report Unknown rather than assuming the newest release.
    pub fn at_least(&self, major: u64, minor: u64) -> bool {
        self.version
            .as_ref()
            .is_some_and(|v| v.at_least(major, minor))
    }
}

unsafe fn libc_getuid() -> u32 {
    unsafe extern "C" {
        fn getuid() -> u32;
    }
    unsafe { getuid() }
}
