//! Turning findings into a report.
//!
//! Describes the machine first, then answers two questions in order: can this
//! box run a validator, and is the validator on it configured correctly. Also
//! holds the JSON and markdown renderers.

use crate::{
    ctx::{Ctx, VersionSource},
    host::REGISTRY_COVERS_THROUGH,
    model::{Finding, Phase, Status},
    registry::CHECKS,
};
use serde::Serialize;

pub struct Style {
    pub color: bool,
}

impl Style {
    pub fn paint(&self, s: Status, text: &str) -> String {
        if !self.color {
            return text.to_string();
        }
        let c = match s {
            Status::Pass => "32",
            Status::Ephemeral => "33",
            Status::Fail => "31",
            Status::Unsupported => "35",
            Status::Skipped => "90",
            Status::Unknown => "36",
        };
        format!("\x1b[{c}m{text}\x1b[0m")
    }

    pub fn dim(&self, text: &str) -> String {
        if self.color {
            format!("\x1b[90m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }

    pub fn bold(&self, text: &str) -> String {
        if self.color {
            format!("\x1b[1m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }
}

const W: usize = 10;

fn wrap(label: &str, body: &str, out: &mut String) {
    if body.is_empty() {
        return;
    }
    let pad = " ".repeat(W + 2);
    for (i, line) in textwrap(body, 68).into_iter().enumerate() {
        if i == 0 {
            out.push_str(&format!("  {label:<W$}{line}\n"));
        } else {
            out.push_str(&format!("{pad}{line}\n"));
        }
    }
}

fn textwrap(s: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for para in s.split('\n') {
        let mut cur = String::new();
        for word in para.split_whitespace() {
            if !cur.is_empty() && cur.len() + 1 + word.len() > width {
                lines.push(std::mem::take(&mut cur));
            }
            if !cur.is_empty() {
                cur.push(' ');
            }
            cur.push_str(word);
        }
        lines.push(cur);
    }
    lines
}

/// What this machine is, before any judgement about it. An operator wants to
/// see the box described back to them whether or not anything is wrong.
fn system_report(ctx: &Ctx, st: &Style, needs_root: usize) -> String {
    let f = &ctx.facts;
    let mut out: Vec<String> = Vec::new();
    macro_rules! row {
        ($k:expr, $v:expr) => {
            out.push(format!("  {:<12}{}\n", $k, $v))
        };
    }

    match (&f.cpu_model, f.cores, f.threads, f.mhz) {
        (Some(m), _, _, _) => {
            row!("cpu", m.clone());
            let mut d = Vec::new();
            if let Some(c) = f.cores {
                d.push(format!("{c} cores"));
            }
            if let Some(t) = f.threads {
                d.push(format!("{t} threads"));
            }
            if let Some(hz) = f.mhz {
                d.push(format!("{hz:.0} MHz"));
            }
            d.push(match f.avx2 {
                Some(true) => "AVX2 yes".to_string(),
                Some(false) => "AVX2 NO".to_string(),
                None => "AVX2 unknown".to_string(),
            });
            row!("", d.join("  ·  "));
        }
        _ => row!("cpu", ctx.arch.clone().unwrap_or_else(|| "unknown".into())),
    }

    if let Some(g) = f.mem_gb {
        let swap = match f.swap_gb {
            Some(s) if s > 0.5 => format!("{s:.0} GB swap"),
            Some(_) => "no swap".to_string(),
            None => "swap unknown".to_string(),
        };
        row!("memory", format!("{g:.0} GB  ·  {swap}"));
    }

    if f.disks.is_empty() && ctx.is_linux() {
        row!("disks", "none detected");
    } else if !f.disks.is_empty() {
        for (i, d) in f.disks.iter().enumerate() {
            let kind = if d.rotational {
                "spinning disk"
            } else {
                "SSD or NVMe"
            };
            row!(
                if i == 0 { "disks" } else { "" },
                format!("{:<12}{:>8.0} GB  {kind}", d.name, d.size_gb)
            );
        }
    }

    for (i, m) in f.mounts.iter().enumerate() {
        let free = match m.free_gb {
            Some(g) => format!("{g:.0} GB free"),
            None => "free space not measured".to_string(),
        };
        row!(
            if i == 0 { "storage" } else { "" },
            format!("{:<16}{:<10}{free}", m.target, m.fstype)
        );
    }

    match (&ctx.os, &ctx.kernel) {
        (Some(o), Some(k)) => row!("os", format!("{o}  ·  kernel {k}")),
        _ => row!("os", os_name()),
    }
    if let Some(v) = &ctx.virt {
        row!("machine", v.clone());
    }

    let mode = match (ctx.uid, needs_root) {
        (0, 0) => "read-only, running as root".to_string(),
        (u, 0) => format!("read-only, running as uid {u}"),
        (u, n) => format!(
            "read-only, running as uid {u}, {n} check{} need{} elevated reads",
            plural(n, "", "s"),
            plural(n, "s", "")
        ),
    };
    let validator = match (&ctx.version, ctx.validator_present) {
        (Some(v), _) => format!("{} {}", ctx.client.label(), v.short()),
        (None, true) => format!("{}, version not detected", ctx.client.label()),
        (None, false) => "none installed".to_string(),
    };
    row!("validator", validator);
    if ctx.validator_present
        && ctx.version.is_none()
        && let VersionSource::Undetected(reason) = &ctx.version_source
    {
        row!("", st.dim(&format!("version not detected: {reason}")));
    }
    if ctx.validator_present && ctx.inv().is_none() {
        for t in &ctx.invocation_trail {
            row!("", st.dim(&format!("resolution trail: {t}")));
        }
    }
    if let Some(VersionSource::Executed(cmd)) =
        ctx.version.as_ref().map(|_| ctx.version_source.clone())
    {
        row!("", st.dim(&format!("version read by running: {cmd}")));
    }
    if let Some(t) = ctx.inv().and_then(|i| i.edit_target.clone()) {
        row!("config", t);
    }
    if ctx
        .version
        .as_ref()
        .is_some_and(|v| v.newer_than_registry())
    {
        row!(
            "",
            st.dim(&format!(
                "checks cover releases up to v{}.{}; this client is newer, so coverage may be incomplete",
                REGISTRY_COVERS_THROUGH.0, REGISTRY_COVERS_THROUGH.1
            ))
        );
    }
    // The verdict names a cluster, so the report has to say where that came
    // from and how to ask a different question.
    row!(
        "profile",
        format!("{}  ·  {}", ctx.profile.label(), ctx.profile_reason)
    );
    row!("preflight", mode);
    format!("{}{}", st.bold("SYSTEM\n"), out.concat())
}

fn finding(f: &Finding, st: &Style, verbose: bool) -> String {
    if !verbose && matches!(f.outcome.status, Status::Pass | Status::Skipped) {
        return String::new();
    }
    let mut s = String::new();
    let tag = if f.provisional { "  [provisional]" } else { "" };
    let title = if f.title.len() > 46 {
        format!("{}...", &f.title[..43])
    } else {
        f.title.to_string()
    };
    s.push_str(&format!(
        "\n  {}  {:<48}{}  {}{}\n\n",
        st.dim(f.id),
        title,
        st.paint(f.outcome.status, f.outcome.status.label()),
        f.severity,
        st.dim(tag)
    ));
    wrap("observed", &f.outcome.observed, &mut s);
    wrap("expected", &f.outcome.expected, &mut s);
    wrap("why", &f.outcome.why, &mut s);

    if !f.outcome.fix.is_empty() {
        let pad = " ".repeat(W + 2);
        for (i, step) in f.outcome.fix.iter().enumerate() {
            let label = if i == 0 { "fix" } else { "" };
            s.push_str(&format!("  {label:<W$}{}\n", step.command));
            if let Some(n) = &step.note {
                s.push_str(&format!("{pad}{}\n", st.dim(&format!("({n})"))));
            }
        }
    }
    if let Some(v) = &f.outcome.verify {
        wrap("verify", v, &mut s);
    }
    let src: Vec<String> = f
        .source
        .iter()
        .map(|x| format!("{} [{}]", x.locator, x.verified_against))
        .collect();
    wrap("source", &src.join("  ·  "), &mut s);
    s
}

/// The whole text report: what the machine is, the verdict, then findings
/// grouped under plain-English headings rather than layer codes.
pub fn report(
    ctx: &Ctx,
    findings: &[Finding],
    st: &Style,
    verbose: bool,
    hide_host: bool,
    needs_root: usize,
) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "{} {}\n\n",
        st.bold("preflight"),
        env!("CARGO_PKG_VERSION")
    ));
    s.push_str(&system_report(ctx, st, needs_root));

    // The machine question is answered first and on its own. A box that cannot
    // run a validator makes every finding about a validator's configuration
    // beside the point.
    s.push_str(&phase_block(
        ctx,
        findings,
        st,
        verbose,
        hide_host,
        Phase::Machine,
        &format!(
            "CAN THIS MACHINE RUN A {} VALIDATOR?",
            ctx.profile.label().to_uppercase()
        ),
    ));
    s.push_str(&phase_block(
        ctx,
        findings,
        st,
        verbose,
        hide_host,
        Phase::Validator,
        "IS THE VALIDATOR CONFIGURED CORRECTLY?",
    ));

    s.push_str(&summary(ctx, findings, st));
    s
}

/// "1 check needs" but "2 checks need".
fn plural(n: usize, one: &'static str, many: &'static str) -> &'static str {
    match n {
        1 => one,
        _ => many,
    }
}

/// "macos" is how Rust spells it; people spell it macOS.
fn os_name() -> &'static str {
    match std::env::consts::OS {
        "macos" => "macOS",
        other => other,
    }
}

/// One half of the report: a verdict, then the findings behind it.
#[allow(clippy::too_many_arguments)]
fn phase_block(
    ctx: &Ctx,
    findings: &[Finding],
    st: &Style,
    verbose: bool,
    hide_host: bool,
    phase: Phase,
    question: &str,
) -> String {
    let mine: Vec<&Finding> = findings.iter().filter(|f| f.phase == phase).collect();
    if mine.is_empty() {
        return String::new();
    }
    let count = |s: Status| mine.iter().filter(|f| f.outcome.status == s).count();
    let fatal = mine
        .iter()
        .filter(|f| f.outcome.status == Status::Fail && f.severity == "fatal")
        .count();
    let unsupported = count(Status::Unsupported);
    let other = count(Status::Fail) - fatal + count(Status::Ephemeral);
    let unknown = mine
        .iter()
        .filter(|f| f.outcome.status == Status::Unknown && !f.reports_only)
        .count();
    let ran = mine.len() - count(Status::Skipped);

    // When the reason is the operating system, say that rather than a count.
    if phase == Phase::Machine && !ctx.is_linux() && !ctx.fs.is_prefixed() {
        return format!(
            "\n{}\n  {}\n{}",
            st.bold(question),
            format_args!(
                "no. this machine runs {}, and a Solana validator runs on Linux",
                os_name()
            ),
            st.dim(&format!(
                "\n  preflight has to run on the Linux machine you are asking about:\n\
                 \x20     ssh you@your-server\n\
                 \x20     cargo install --git {repo}\n\
                 \x20     preflight --profile testnet\n\n\
                 \x20 or, to check that machine's validator settings from here, copy its\n\
                 \x20 command line over first:\n\
                 \x20     ssh you@your-server 'cat /proc/$(pgrep -f agave-validator)/cmdline | tr \"\\0\" \" \"' > cmdline.txt\n\
                 \x20     ssh you@your-server 'agave-validator --version'\n\
                 \x20     preflight --invocation cmdline.txt --client agave-validator@<that version>\n",
                repo = env!("CARGO_PKG_REPOSITORY")
            ))
        );
    }

    let verdict = match (ran, unsupported, fatal, other, unknown) {
        (0, ..) if phase == Phase::Validator => "no validator installed, nothing to check".into(),
        (0, ..) => "nothing applied to this host".into(),
        (_, u, ..) if u > 0 => format!(
            "no. {u} requirement{} cannot be met on this hardware",
            plural(u, "", "s")
        ),
        (_, _, f, _, _) if f > 0 => {
            format!("no. {f} thing{} must be fixed first", plural(f, "", "s"))
        }
        (_, _, _, o, _) if o > 0 => {
            format!("yes, with {o} thing{} worth fixing", plural(o, "", "s"))
        }
        (_, _, _, _, u) if u > 0 => format!(
            "cannot say. {u} thing{} could not be read",
            plural(u, "", "s")
        ),
        _ => "yes".into(),
    };

    let mut s = format!("\n{}\n  {}\n", st.bold(question), verdict);
    if phase == Phase::Machine
        && !ctx.validator_present
        && ctx.profile == crate::model::Profile::Local
    {
        s.push_str(&st.dim(
            "  this asks what a test validator needs. for a real voting node:\n\
             \x20   preflight --profile testnet\n",
        ));
    }

    let mut sections: Vec<&'static str> = Vec::new();
    for f in &mine {
        if !sections.contains(&f.section) {
            sections.push(f.section);
        }
    }
    for section in sections {
        let shown: Vec<&&Finding> = mine
            .iter()
            .filter(|f| f.section == section)
            .filter(|f| !(hide_host && f.needs_linux))
            .filter(|f| verbose || !matches!(f.outcome.status, Status::Pass | Status::Skipped))
            .collect();
        if shown.is_empty() {
            continue;
        }
        s.push_str(&format!("\n{}\n", st.dim(section)));
        for f in shown {
            s.push_str(&finding(f, st, true));
        }
    }
    s
}

fn summary(ctx: &Ctx, findings: &[Finding], st: &Style) -> String {
    // Nothing ran, and the verdict already said why. Counts would be filler.
    if !ctx.is_linux() && !ctx.fs.is_prefixed() {
        return String::new();
    }

    let n = |s: Status| findings.iter().filter(|f| f.outcome.status == s).count();
    let mut parts = Vec::new();
    for s in [
        Status::Fail,
        Status::Unsupported,
        Status::Ephemeral,
        Status::Unknown,
        Status::Pass,
    ] {
        if n(s) > 0 {
            parts.push(st.paint(s, &format!("{} {}", n(s), s.label().to_lowercase())));
        }
    }
    let mut line = format!("\n{}\n", parts.join("  ·  "));

    // Why a check was skipped, matched on the reason it recorded.
    const SKIP_REASONS: &[(&str, &str)] = &[
        (
            "no validator installed",
            "need a validator installed on this host",
        ),
        (
            "no validator on this host",
            "need a validator installed on this host",
        ),
        (
            "requirement starts at",
            "apply to a newer release than this client",
        ),
        ("not applicable to profile", "do not apply to this profile"),
    ];

    let mut groups: Vec<(&str, usize)> = Vec::new();
    for f in findings
        .iter()
        .filter(|f| f.outcome.status == Status::Skipped)
    {
        let reason = SKIP_REASONS
            .iter()
            .find(|(needle, _)| f.outcome.observed.contains(needle))
            .map_or("do not apply to this host", |(_, label)| *label);
        match groups.iter_mut().find(|(r, _)| *r == reason) {
            Some((_, count)) => *count += 1,
            None => groups.push((reason, 1)),
        }
    }
    if !groups.is_empty() {
        line.push_str("\nnot checked\n");
        for (reason, count) in &groups {
            line.push_str(&st.dim(&format!("          {count:>2}  {reason}\n")));
        }
        line.push_str(&st.dim("          preflight -v  lists them individually\n"));
    }

    if findings.iter().all(|f| f.outcome.status == Status::Skipped) {
        return format!(
            "\nnothing applied to this host\n{}",
            st.dim(
                "          no validator found on this host: no running process, no systemd\n\
                 \x20         unit, no binary on PATH. preflight has nothing to look at.\n\n\
                 \x20         to check a command line from somewhere else:\n\
                 \x20           preflight --invocation <file> --client agave-validator@<version>\n"
            )
        );
    }

    let fatal: Vec<&str> = findings
        .iter()
        .filter(|f| f.outcome.status == Status::Fail && f.severity == "fatal")
        .map(|f| f.id)
        .collect();
    let other = findings
        .iter()
        .filter(|f| f.outcome.status == Status::Fail && f.severity != "fatal")
        .count();

    if !fatal.is_empty() {
        line.push_str(&format!(
            "\nnext      {} stop{} the validator from starting: {}\n",
            if fatal.len() == 1 {
                "1 fatal finding".into()
            } else {
                format!("{} fatal findings", fatal.len())
            },
            if fatal.len() == 1 { "s" } else { "" },
            fatal.join(", ")
        ));
        line.push_str(plural(
            fatal.len(),
            "          fix that first, then re-run\n",
            "          fix those first, then re-run\n",
        ));
        if other > 0 {
            line.push_str(&st.dim(&format!(
                "          the other {other} {} drift: the node runs, but not as configured\n",
                plural(other, "is", "are")
            )));
        }
    } else if other > 0 {
        line.push_str(&format!(
            "\nnext      nothing blocks startup; {other} finding{} mean the node runs but not as configured\n",
            plural(other, "", "s")
        ));
    }
    if findings.iter().any(|f| f.outcome.status == Status::Fail) {
        line.push_str(&st.dim("          preflight explain <id>  for one finding on its own\n"));
    }
    line
}

pub mod json {
    use super::*;

    #[derive(Serialize)]
    struct Host<'a> {
        arch: Option<&'a String>,
        kernel: Option<&'a String>,
        os: Option<&'a String>,
        virt: Option<&'a String>,
        uid: u32,
    }

    #[derive(Serialize)]
    struct Report<'a> {
        tool: &'a str,
        version: &'a str,
        profile: &'a str,
        profile_reason: &'a str,
        client: &'a str,
        client_version: Option<String>,
        invocation_origin: Option<&'a str>,
        invocation_trail: &'a [String],
        host: Host<'a>,
        findings: &'a [Finding],
        exit_code: i32,
    }

    pub fn render(ctx: &Ctx, findings: &[Finding]) -> String {
        let r = Report {
            tool: "preflight",
            version: env!("CARGO_PKG_VERSION"),
            profile: ctx.profile.label(),
            profile_reason: &ctx.profile_reason,
            client: ctx.client.label(),
            client_version: ctx.version.as_ref().map(|v| v.short()),
            invocation_origin: ctx.inv().map(|i| i.origin.label()),
            invocation_trail: &ctx.invocation_trail,
            host: Host {
                arch: ctx.arch.as_ref(),
                kernel: ctx.kernel.as_ref(),
                os: ctx.os.as_ref(),
                virt: ctx.virt.as_ref(),
                uid: ctx.uid,
            },
            findings,
            exit_code: crate::model::exit_code(findings),
        };
        serde_json::to_string_pretty(&r).unwrap_or_else(|e| format!("{{\"error\":\"{e}\"}}"))
    }
}

pub mod markdown {
    use super::*;

    pub fn render(ctx: &Ctx, findings: &[Finding]) -> String {
        let mut s = String::new();
        s.push_str("## preflight report\n\n");
        s.push_str(&format!(
            "| | |\n|---|---|\n| host | `{}` · kernel `{}` |\n| os | {} |\n| client | {} {} |\n| profile | `{}` ({}) |\n\n",
            ctx.arch.clone().unwrap_or_else(|| "not probed".into()),
            ctx.kernel.clone().unwrap_or_else(|| "not probed".into()),
            ctx.os.clone().unwrap_or_else(|| "not probed".into()),
            ctx.client.label(),
            ctx.version.as_ref().map(|v| v.short()).unwrap_or_else(|| "unknown".into()),
            ctx.profile.label(),
            ctx.profile_reason
        ));

        s.push_str("| id | status | severity | check |\n|---|---|---|---|\n");
        for f in findings {
            if f.outcome.status == Status::Skipped {
                continue;
            }
            s.push_str(&format!(
                "| `{}` | {} | {} | {} |\n",
                f.id,
                f.outcome.status.label(),
                f.severity,
                f.title
            ));
        }

        for f in findings {
            if matches!(f.outcome.status, Status::Pass | Status::Skipped) {
                continue;
            }
            s.push_str(&format!("\n### {} — {}\n\n", f.id, f.title));
            s.push_str(&format!("- **observed** {}\n", f.outcome.observed));
            if !f.outcome.expected.is_empty() {
                s.push_str(&format!("- **expected** {}\n", f.outcome.expected));
            }
            if !f.outcome.why.is_empty() {
                s.push_str(&format!("- **why** {}\n", f.outcome.why));
            }
            if !f.outcome.fix.is_empty() {
                s.push_str("\n```\n");
                for step in &f.outcome.fix {
                    s.push_str(&format!("{}\n", step.command));
                }
                s.push_str("```\n");
            }
        }
        s
    }

    pub fn dump_registry() -> String {
        let mut s = String::from(
            "| id | layer | severity | profiles | clients | check | source | verified |\n|---|---|---|---|---|---|---|---|\n",
        );
        for c in CHECKS {
            let profiles: Vec<&str> = c.profiles.iter().map(|p| p.label()).collect();
            let clients: Vec<&str> = c.clients.iter().map(|x| x.label()).collect();
            let src: Vec<String> = c.source.iter().map(|x| x.locator.to_string()).collect();
            let ver: Vec<String> = c
                .source
                .iter()
                .map(|x| x.verified_against.to_string())
                .collect();
            s.push_str(&format!(
                "| `{}` | {} | {} | {} | {} | {}{} | {} | {} |\n",
                c.id,
                c.layer.label(),
                c.severity.label(),
                profiles.join(" "),
                clients.join(" "),
                c.title,
                if c.provisional() {
                    " *(provisional)*"
                } else {
                    ""
                },
                src.join("<br>"),
                ver.join("<br>")
            ));
        }
        s
    }
}
