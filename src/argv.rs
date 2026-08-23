//! Finding the validator command line.
//!
//! Looks at a running process first, then a systemd unit, then the wrapper
//! script that unit points at, since that is the layout Anza's setup guide
//! produces. Failure is reported with the full trail, never as an empty result.

use crate::host::Rootfs;
use serde::Serialize;
use std::collections::BTreeMap;

const VALIDATOR_BINS: &[&str] = &[
    "agave-validator",
    "solana-validator",
    "solana-test-validator",
    "fdctl",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Origin {
    RunningProcess,
    UnitExecStart,
    WrapperScript,
    File,
}

impl Origin {
    pub fn label(self) -> &'static str {
        match self {
            Origin::RunningProcess => "running process",
            Origin::UnitExecStart => "unit ExecStart",
            Origin::WrapperScript => "unit ExecStart -> wrapper script",
            Origin::File => "--invocation file",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Invocation {
    pub origin: Origin,
    pub pid: Option<String>,
    /// The file an operator actually edits to change this command line.
    pub edit_target: Option<String>,
    pub unit_path: Option<String>,
    pub unit_name: Option<String>,
    pub program: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub unresolved: Vec<String>,
    pub trail: Vec<String>,
}

impl Invocation {
    pub fn has(&self, flag: &str) -> bool {
        self.args
            .iter()
            .any(|a| a == flag || a.starts_with(&format!("{flag}=")))
    }

    pub fn value(&self, flag: &str) -> Option<String> {
        let eq = format!("{flag}=");
        for (i, a) in self.args.iter().enumerate() {
            if let Some(v) = a.strip_prefix(&eq) {
                return Some(v.to_string());
            }
            if a == flag {
                return self
                    .args
                    .get(i + 1)
                    .filter(|n| !n.starts_with("--"))
                    .cloned();
            }
        }
        None
    }

    pub fn present_from(&self, list: &[&str]) -> Vec<String> {
        list.iter()
            .filter(|f| self.has(f))
            .map(|f| f.to_string())
            .collect()
    }
}

pub fn split_words(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut started = false;
    let mut chars = line.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(n) = chars.next()
                    && n != '\n'
                {
                    cur.push(n);
                    started = true;
                }
            }
            '\'' | '"' => match quote {
                Some(q) if q == c => quote = None,
                Some(_) => {
                    cur.push(c);
                    started = true;
                }
                None => {
                    quote = Some(c);
                    started = true;
                }
            },
            c if c.is_whitespace() && quote.is_none() => {
                if started {
                    out.push(std::mem::take(&mut cur));
                    started = false;
                }
            }
            c => {
                cur.push(c);
                started = true;
            }
        }
    }
    if started {
        out.push(cur);
    }
    out
}

fn join_continuations(text: &str) -> String {
    text.replace("\\\n", " ")
}

pub fn is_validator_bin(tok: &str) -> bool {
    let base = tok.rsplit('/').next().unwrap_or(tok);
    VALIDATOR_BINS.contains(&base)
}

fn split_env_and_args(words: Vec<String>) -> (BTreeMap<String, String>, Vec<String>, Vec<String>) {
    let mut env = BTreeMap::new();
    let mut unresolved = Vec::new();
    let mut rest = Vec::new();
    let mut seen_program = false;

    for w in words {
        if !seen_program && !w.starts_with('-') && w.contains('=') && !w.contains('/') {
            let (k, v) = w.split_once('=').unwrap();
            env.insert(k.to_string(), v.to_string());
            continue;
        }
        if w.contains('$') {
            unresolved.push(w.clone());
        }
        if !seen_program && is_validator_bin(&w) {
            seen_program = true;
        }
        rest.push(w);
    }
    (env, rest, unresolved)
}

fn build(
    origin: Origin,
    words: Vec<String>,
    mut trail: Vec<String>,
    mut env: BTreeMap<String, String>,
) -> Option<Invocation> {
    let start = words.iter().position(|w| is_validator_bin(w))?;
    let (inline_env, rest, unresolved) = split_env_and_args(words[start..].to_vec());
    env.extend(inline_env);
    let mut it = rest.into_iter();
    let program = it.next()?;
    if !unresolved.is_empty() {
        trail.push(format!(
            "{} unexpanded token(s) left literal",
            unresolved.len()
        ));
    }
    Some(Invocation {
        origin,
        pid: None,
        edit_target: None,
        unit_path: None,
        unit_name: None,
        program,
        args: it.collect(),
        env,
        unresolved,
        trail,
    })
}

fn find_pid(fs: &Rootfs) -> Option<(String, Vec<String>)> {
    for p in fs.list("/proc") {
        let Some(name) = p.file_name().map(|n| n.to_string_lossy().to_string()) else {
            continue;
        };
        if !name.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        // One pid preflight cannot read is not the end of the search. It used
        // to be: the ? here returned None for the whole function.
        let Ok(raw) = std::fs::read_to_string(p.join("cmdline")) else {
            continue;
        };
        let words: Vec<String> = raw
            .split('\0')
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        if words.first().is_some_and(|w| is_validator_bin(w)) {
            return Some((name, words));
        }
    }
    None
}

fn proc_environ(fs: &Rootfs, pid: &str) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    if let Ok(raw) = fs.read(format!("/proc/{pid}/environ")) {
        for kv in raw.split('\0').filter(|s| !s.is_empty()) {
            if let Some((k, v)) = kv.split_once('=') {
                env.insert(k.to_string(), v.to_string());
            }
        }
    }
    env
}

/// The unit that owns a running process, from systemd's own bookkeeping.
///
/// /proc/<pid>/cgroup names the unit exactly. Guessing by scanning unit files
/// for the word "validator" matches anything that merely mentions one, and then
/// every fix points at the wrong file and restarts the wrong service.
fn unit_of_pid(fs: &Rootfs, pid: &str) -> Option<String> {
    let text = fs.read(format!("/proc/{pid}/cgroup")).ok()?;
    text.lines()
        .filter_map(|l| l.rsplit('/').next())
        .find(|seg| seg.ends_with(".service"))
        .map(str::to_string)
}

/// Details for a unit named by systemd, or found by scanning when no process is
/// running. The scan only accepts a unit whose ExecStart actually reaches a
/// validator binary, directly or through a wrapper script.
fn unit_details(fs: &Rootfs, name: &str) -> Option<(String, String, String)> {
    let path = unit_files(fs)
        .into_iter()
        .find(|p| p.file_name().is_some_and(|f| f == name))?;
    let text = std::fs::read_to_string(&path).ok()?;
    let (exec, _) = parse_unit(&text)?;
    let abs = format!("/etc/systemd/system/{name}");
    let first = split_words(&exec).first().cloned().unwrap_or_default();
    let edit = match is_validator_bin(&first) {
        true => abs.clone(),
        false => first,
    };
    Some((abs, name.to_string(), edit))
}

fn owning_unit(fs: &Rootfs) -> Option<(String, String, String)> {
    for unit in unit_files(fs) {
        let Ok(text) = std::fs::read_to_string(&unit) else {
            continue;
        };
        if !launches_a_validator(fs, &text) {
            continue;
        }
        let Some(name) = unit.file_name().map(|n| n.to_string_lossy().to_string()) else {
            continue;
        };
        let abs = format!("/etc/systemd/system/{name}");
        let Some((exec, _)) = parse_unit(&text) else {
            continue;
        };
        let first = split_words(&exec).first().cloned().unwrap_or_default();
        let edit = if is_validator_bin(&first) {
            abs.clone()
        } else {
            first
        };
        return Some((abs, name, edit));
    }
    None
}

/// True when this unit's ExecStart reaches a validator binary, either directly
/// or through the wrapper script it points at.
fn launches_a_validator(fs: &Rootfs, text: &str) -> bool {
    let Some((exec, _)) = parse_unit(text) else {
        return false;
    };
    let words = split_words(&exec);
    if words.first().is_some_and(|w| is_validator_bin(w)) {
        return true;
    }
    let Some(script) = words.first() else {
        return false;
    };
    fs.read(script)
        .ok()
        .and_then(|body| exec_line_from_script(&body))
        .is_some()
}

fn unit_files(fs: &Rootfs) -> Vec<std::path::PathBuf> {
    let mut v = fs.list("/etc/systemd/system");
    v.extend(fs.list("/lib/systemd/system"));
    v.extend(fs.list("/usr/lib/systemd/system"));
    v.retain(|p| p.extension().is_some_and(|e| e == "service"));
    v
}

fn parse_unit(text: &str) -> Option<(String, BTreeMap<String, String>)> {
    let joined = join_continuations(text);
    let mut exec: Option<String> = None;
    let mut env = BTreeMap::new();
    for line in joined.lines() {
        let l = line.trim();
        if let Some(v) = l.strip_prefix("ExecStart=") {
            exec = Some(
                v.trim_start_matches(['-', '@', '+', '!'])
                    .trim()
                    .to_string(),
            );
        } else if let Some(v) = l.strip_prefix("Environment=") {
            for w in split_words(v) {
                if let Some((k, val)) = w.split_once('=') {
                    env.insert(k.to_string(), val.to_string());
                }
            }
        }
    }
    Some((exec?, env))
}

fn exec_line_from_script(text: &str) -> Option<Vec<String>> {
    let joined = join_continuations(text);
    for line in joined.lines() {
        let l = line.trim();
        if l.starts_with('#') {
            continue;
        }
        let words = split_words(l);
        let mut idx = 0;
        if words.first().map(String::as_str) == Some("exec") {
            idx = 1;
        }
        if words.get(idx).is_some_and(|w| is_validator_bin(w)) {
            return Some(words[idx..].to_vec());
        }
    }
    None
}

pub fn resolve(fs: &Rootfs) -> Result<Invocation, Vec<String>> {
    let mut trail = Vec::new();

    if let Some((pid, words)) = find_pid(fs) {
        trail.push(format!("found running validator, pid {pid}"));
        let env = proc_environ(fs, &pid);
        if let Some(mut inv) = build(Origin::RunningProcess, words, trail.clone(), env) {
            inv.pid = Some(pid);
            let owner = unit_of_pid(fs, inv.pid.as_deref().unwrap_or_default())
                .and_then(|name| unit_details(fs, &name))
                .or_else(|| owning_unit(fs));
            if let Some((path, name, edit)) = owner {
                inv.unit_path = Some(path);
                inv.unit_name = Some(name);
                inv.edit_target = Some(edit);
            }
            return Ok(inv);
        }
    }
    trail.push("no running validator process found".into());

    for unit in unit_files(fs) {
        let Ok(text) = std::fs::read_to_string(&unit) else {
            continue;
        };
        if !VALIDATOR_BINS.iter().any(|b| text.contains(b)) && !text.contains("validator") {
            continue;
        }
        let name = unit
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let abs = format!("/etc/systemd/system/{name}");
        let Some((exec, env)) = parse_unit(&text) else {
            continue;
        };
        trail.push(format!("unit {name}: ExecStart={exec}"));

        let words = split_words(&exec);
        if words.first().is_some_and(|w| is_validator_bin(w))
            && let Some(mut inv) = build(
                Origin::UnitExecStart,
                words.clone(),
                trail.clone(),
                env.clone(),
            )
        {
            inv.unit_path = Some(abs.clone());
            inv.unit_name = Some(name.clone());
            inv.edit_target = Some(abs.clone());
            return Ok(inv);
        }

        let script = words.first().cloned().unwrap_or_default();
        match fs.read(&script) {
            Ok(body) => {
                trail.push(format!(
                    "ExecStart is not a validator binary, reading {script}"
                ));
                match exec_line_from_script(&body) {
                    Some(w) => {
                        trail.push("found exec line in wrapper script".into());
                        if let Some(mut inv) =
                            build(Origin::WrapperScript, w, trail.clone(), env.clone())
                        {
                            inv.unit_path = Some(abs.clone());
                            inv.unit_name = Some(name.clone());
                            inv.edit_target = Some(script.clone());
                            return Ok(inv);
                        }
                    }
                    None => trail.push(format!("no 'exec <validator>' line found in {script}")),
                }
            }
            Err(e) => trail.push(format!("cannot read {script}: {e}")),
        }
    }

    trail.push("no validator invocation could be resolved".into());
    Err(trail)
}

pub fn from_text(text: &str) -> Result<Invocation, Vec<String>> {
    let trail = vec!["read from --invocation file".to_string()];
    let joined = join_continuations(text);
    if let Some(w) = exec_line_from_script(&joined) {
        return build(Origin::File, w, trail, BTreeMap::new())
            .ok_or_else(|| vec!["file contained no validator invocation".to_string()]);
    }
    let words = split_words(&joined);
    build(Origin::File, words, trail, BTreeMap::new())
        .ok_or_else(|| vec!["file contained no recognised validator binary".to_string()])
}
