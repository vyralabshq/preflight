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
    /// A view of other devices rather than hardware of its own. Listed, but
    /// never added to a capacity total: a mapper volume and the disk under it
    /// are the same bytes counted twice.
    pub mapped: bool,
}

pub struct Mount {
    pub target: String,
    pub fstype: String,
    pub free_gb: Option<f64>,
    pub total_gb: Option<f64>,
}

/// cpuinfo's "cpu cores" is per socket, so a dual socket box under-counts.
/// Unique (physical id, core id) pairs are the real number.
pub fn physical_cores(info: &str) -> Option<usize> {
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
        true => field(info, "cpu cores").and_then(|v| v.parse().ok()),
    }
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
        f.cores = physical_cores(&info);
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
        if name.starts_with("loop") || name.starts_with("ram") {
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
            let mapped = std::fs::read_dir(dev.join("slaves"))
                .map(|mut d| d.next().is_some())
                .unwrap_or(false);
            let label = std::fs::read_to_string(dev.join("dm/name"))
                .ok()
                .map(|n| n.trim().to_string())
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| name.to_string());
            f.disks.push(Disk {
                name: label,
                size_gb: s * 512.0 / 1e9,
                rotational: rot,
                mapped,
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
            let (free, total) = space_gb(fs, target);
            f.mounts.push(Mount {
                target: target.to_string(),
                fstype: fstype.to_string(),
                free_gb: free,
                total_gb: total,
            });
        }
    }
    f
}

/// du, in process. Sums allocated blocks rather than file lengths so the answer
/// matches what du reports and what the filesystem actually gave away. Real host
/// only, for the same reason as statvfs. rocksdb keeps a few thousand large SSTs
/// rather than many small files, so this is metadata reads, not a content walk.
pub fn dir_size_gb(fs: &Rootfs, path: &str) -> Option<f64> {
    use std::os::unix::fs::MetadataExt;
    if fs.is_prefixed() {
        return None;
    }
    let mut total: u64 = 0;
    let mut stack = vec![std::path::PathBuf::from(path)];
    let mut seen = 0u32;
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            // The top of the tree failing means no answer. A subdirectory the
            // caller cannot read means an undercount, which would be worse to
            // report as a figure than to withhold.
            return None;
        };
        for e in entries.flatten() {
            let Ok(md) = e.metadata() else { return None };
            seen += 1;
            // A blockstore should never hold this many entries. Bail rather
            // than stall a read-only tool on a pathological tree.
            if seen > 2_000_000 {
                return None;
            }
            match md.is_dir() {
                true => stack.push(e.path()),
                false => total += md.blocks() * 512,
            }
        }
    }
    Some(total as f64 / 1e9)
}

/// statvfs on the real host only. Under --root the numbers would describe the
/// machine running preflight, not the machine being reported on.
fn space_gb(fs: &Rootfs, target: &str) -> (Option<f64>, Option<f64>) {
    match statvfs_gb(fs, target) {
        Some((free, total)) => (Some(free), Some(total)),
        None => (None, None),
    }
}

/// Free and total gigabytes, on the real host only. Under --root the numbers
/// would describe the machine running preflight, not the one being reported on.
fn statvfs_gb(fs: &Rootfs, target: &str) -> Option<(f64, f64)> {
    if fs.is_prefixed() {
        return None;
    }
    let c = std::ffi::CString::new(target).ok()?;
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut s) } != 0 || s.f_frsize == 0 {
        return None;
    }
    let unit = s.f_frsize as f64;
    Some((
        s.f_bavail as f64 * unit / 1e9,
        s.f_blocks as f64 * unit / 1e9,
    ))
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

/// A UTC timestamp for the report header, so a pasted run says when it was
/// taken. Seconds since the epoch converted by hand rather than pulling in a
/// date library for one line.
pub fn now_utc() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (days, rem) = (secs / 86_400, secs % 86_400);
    let (h, m) = (rem / 3600, (rem % 3600) / 60);

    let mut year = 1970;
    let mut left = days as i64;
    loop {
        let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
        let len = if leap { 366 } else { 365 };
        if left < len {
            break;
        }
        left -= len;
        year += 1;
    }
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let months = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 0;
    while left >= months[month] {
        left -= months[month];
        month += 1;
    }
    format!("{year}-{:02}-{:02} {h:02}:{m:02} UTC", month + 1, left + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The check used to print du as an instruction. It runs it now, so the
    /// walk has to be right: nested directories counted, unreadable trees
    /// withheld rather than undercounted, captured trees out of scope.
    #[test]
    fn directory_sizes_are_measured_not_delegated() {
        let dir = std::env::temp_dir().join("preflight-dirsize-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("nested")).unwrap();
        std::fs::write(dir.join("a"), vec![0u8; 2_000_000]).unwrap();
        std::fs::write(dir.join("nested/b"), vec![0u8; 3_000_000]).unwrap();

        let live = Rootfs::new(None);
        let got = dir_size_gb(&live, dir.to_str().unwrap()).expect("a readable tree measures");
        // Allocated blocks, so at least the bytes written and not wildly over.
        assert!(
            (0.005..0.02).contains(&got),
            "5 MB across two directories, got {got} GB"
        );

        assert!(
            dir_size_gb(&live, "/nonexistent-preflight-path").is_none(),
            "an unreadable tree has no answer, and an undercount would be worse"
        );
        let captured = Rootfs::new(Some(PathBuf::from("/tmp")));
        assert!(
            dir_size_gb(&captured, dir.to_str().unwrap()).is_none(),
            "a captured tree carries no sizes for the machine being reported on"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
