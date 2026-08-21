//! Everything preflight reads about a machine.
//!
//! Rootfs is the only way the codebase touches a filesystem, so every read can
//! be pointed at a captured tree instead of the live host. Facts are what the
//! machine is. ClientVersion is which validator it runs.

use serde::Serialize;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone)]
pub struct Rootfs {
    pub root: PathBuf,
}

impl Rootfs {
    pub fn new(root: Option<PathBuf>) -> Self {
        Rootfs {
            root: root.unwrap_or_else(|| PathBuf::from("/")),
        }
    }

    pub fn is_prefixed(&self) -> bool {
        self.root != Path::new("/")
    }

    pub fn at(&self, p: impl AsRef<Path>) -> PathBuf {
        let p = p.as_ref();
        let rel = p.strip_prefix("/").unwrap_or(p);
        self.root.join(rel)
    }

    pub fn read(&self, p: impl AsRef<Path>) -> io::Result<String> {
        fs::read_to_string(self.at(p))
    }

    pub fn read_trim(&self, p: impl AsRef<Path>) -> Option<String> {
        self.read(p).ok().map(|s| s.trim().to_string())
    }

    pub fn exists(&self, p: impl AsRef<Path>) -> bool {
        self.at(p).exists()
    }

    pub fn list(&self, dir: impl AsRef<Path>) -> Vec<PathBuf> {
        let mut out = Vec::new();
        if let Ok(rd) = fs::read_dir(self.at(dir)) {
            for e in rd.flatten() {
                out.push(e.path());
            }
        }
        out.sort();
        out
    }
}

#[derive(Default)]
pub struct Facts {
    pub cpu_model: Option<String>,
    pub cores: Option<usize>,
    pub threads: Option<usize>,
    pub mhz: Option<f64>,
    pub avx2: Option<bool>,
    pub mem_gb: Option<f64>,
    pub swap_gb: Option<f64>,
    pub disks: Vec<Disk>,
    pub mounts: Vec<Mount>,
}

pub struct Disk {
    pub name: String,
    pub size_gb: f64,
    pub rotational: bool,
}

pub struct Mount {
    pub target: String,
    pub fstype: String,
    pub free_gb: Option<f64>,
}

fn field<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines()
        .find(|l| l.starts_with(key))
        .and_then(|l| l.split(':').nth(1))
        .map(str::trim)
}

pub fn gather(fs: &Rootfs) -> Facts {
    let mut f = Facts::default();

    if let Ok(info) = fs.read("/proc/cpuinfo") {
        f.cpu_model = field(&info, "model name")
            .or_else(|| field(&info, "Model"))
            .map(str::to_string);
        f.threads =
            Some(info.lines().filter(|l| l.starts_with("processor")).count()).filter(|n| *n > 0);
        f.cores = field(&info, "cpu cores").and_then(|v| v.parse().ok());
        f.mhz = field(&info, "cpu MHz").and_then(|v| v.parse().ok());
        let flags = info
            .lines()
            .find(|l| l.starts_with("flags") || l.starts_with("Features"))
            .unwrap_or("");
        f.avx2 = Some(flags.split_whitespace().any(|x| x == "avx2"));
    }

    if let Ok(m) = fs.read("/proc/meminfo") {
        let kb = |k: &str| {
            m.lines()
                .find(|l| l.starts_with(k))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|v| v.parse::<f64>().ok())
        };
        f.mem_gb = kb("MemTotal:").map(|v| v / 1024.0 / 1024.0);
        f.swap_gb = kb("SwapTotal:").map(|v| v / 1024.0 / 1024.0);
    }

    for dev in fs.list("/sys/block") {
        let Some(name) = dev.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with("loop") || name.starts_with("ram") || name.starts_with("dm-") {
            continue;
        }
        let sectors: Option<f64> = std::fs::read_to_string(dev.join("size"))
            .ok()
            .and_then(|v| v.trim().parse().ok());
        let rot = std::fs::read_to_string(dev.join("queue/rotational"))
            .map(|v| v.trim() == "1")
            .unwrap_or(false);
        if let Some(s) = sectors
            && s > 0.0
        {
            f.disks.push(Disk {
                name: name.to_string(),
                size_gb: s * 512.0 / 1e9,
                rotational: rot,
            });
        }
    }
    f.disks.sort_by(|a, b| a.name.cmp(&b.name));

    if let Ok(mounts) = fs.read("/proc/mounts") {
        for line in mounts.lines() {
            let p: Vec<&str> = line.split_whitespace().collect();
            if p.len() < 3 {
                continue;
            }
            let (src, target, fstype) = (p[0], p[1], p[2]);
            // snap loopbacks and the boot partition are not validator storage,
            // and a real host has a dozen of them burying the disks that are.
            let noise = !src.starts_with("/dev/")
                || fstype == "squashfs"
                || target.starts_with("/snap/")
                || target.starts_with("/boot");
            if noise {
                continue;
            }
            f.mounts.push(Mount {
                target: target.to_string(),
                fstype: fstype.to_string(),
                free_gb: free_space_gb(fs, target),
            });
        }
    }
    f
}

/// statvfs on the real host only. Under --root the numbers would describe the
/// machine running preflight, not the machine being reported on.
fn free_space_gb(fs: &Rootfs, target: &str) -> Option<f64> {
    if fs.is_prefixed() {
        return None;
    }
    unsafe extern "C" {
        fn statvfs(path: *const i8, buf: *mut Statvfs) -> i32;
    }
    #[repr(C)]
    #[derive(Default)]
    struct Statvfs {
        f_bsize: u64,
        f_frsize: u64,
        f_blocks: u64,
        f_bfree: u64,
        f_bavail: u64,
        rest: [u64; 8],
    }
    let c = std::ffi::CString::new(target).ok()?;
    let mut s = Statvfs::default();
    let rc = unsafe { statvfs(c.as_ptr(), &mut s) };
    if rc != 0 || s.f_frsize == 0 {
        return None;
    }
    Some(s.f_bavail as f64 * s.f_frsize as f64 / 1e9)
}

/// A client version, parsed only as far as checks need it.
///
/// Checks gate on floors — "this requirement starts at v4.1" — never on a
/// release channel. A floor stays correct forever: a flag removed in v4.0 is
/// still removed in v5. A channel does not: today's alpha is next quarter's
/// stable, and a hardcoded table would go quietly wrong with nothing to signal
/// it. So preflight reports the version it found and stops there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ClientVersion {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub raw: String,
}

impl ClientVersion {
    pub fn parse(s: &str) -> Option<ClientVersion> {
        let tok = s
            .split_whitespace()
            .find(|t| t.chars().next().is_some_and(|c| c.is_ascii_digit()) && t.contains('.'))?;
        let core = tok.split(['-', '+']).next()?;
        let mut it = core.split('.');
        let major = it.next()?.parse().ok()?;
        let minor = it.next()?.parse().ok()?;
        let patch = it.next().unwrap_or("0").parse().unwrap_or(0);
        Some(ClientVersion {
            major,
            minor,
            patch,
            raw: tok.to_string(),
        })
    }

    pub fn at_least(&self, major: u64, minor: u64) -> bool {
        (self.major, self.minor) >= (major, minor)
    }

    pub fn short(&self) -> String {
        format!("{}.{}.{}", self.major, self.minor, self.patch)
    }

    /// True when the version is newer than anything preflight's registry was
    /// written against, so the report can say its coverage may be incomplete
    /// rather than implying a clean bill of health.
    pub fn newer_than_registry(&self) -> bool {
        (self.major, self.minor) > REGISTRY_COVERS_THROUGH
    }
}

/// The newest release any check in the registry cites. Bump it when checks are
/// added for a newer release; the drift job in the README watches for this
/// falling behind.
pub const REGISTRY_COVERS_THROUGH: (u64, u64) = (4, 3);
