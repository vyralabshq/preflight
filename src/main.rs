//! Command line entry point.
//!
//! Parses flags, probes the host once, runs every check the profile and client
//! allow, then renders the report and returns an exit code that says whether
//! anything is wrong.

mod argv;
mod checks;
mod ctx;
mod host;
mod menu;
mod model;
mod privilege;
mod registry;
mod render;

use clap::{Parser, Subcommand, ValueEnum};
use ctx::{Ctx, CtxOptions};
use model::{Check, Finding, Layer, Profile};
use render::Style;
use std::{io::IsTerminal, path::PathBuf};

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
pub enum ProfileArg {
    Local,
    Testnet,
    Mainnet,
}

impl From<ProfileArg> for model::Profile {
    fn from(p: ProfileArg) -> Self {
        match p {
            ProfileArg::Local => model::Profile::Local,
            ProfileArg::Testnet => model::Profile::Testnet,
            ProfileArg::Mainnet => model::Profile::Mainnet,
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
pub enum Format {
    Text,
    Json,
    Markdown,
}

#[derive(Parser)]
#[command(
    name = "preflight",
    version,
    about = "Read-only preflight checks for Solana validator hosts",
    long_about = "preflight never writes to your system, never runs a command you have not seen, and never guesses.\nIt reports what is wrong and prints the fix; it does not apply it."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// What the machine is being judged against. Detected when not given
    #[arg(long, value_enum, global = true)]
    pub profile: Option<ProfileArg>,

    #[arg(
        long,
        global = true,
        value_delimiter = ',',
        help = "Run only these check ids or layers"
    )]
    pub only: Vec<String>,

    #[arg(
        long,
        global = true,
        value_delimiter = ',',
        help = "Skip these check ids or layers"
    )]
    pub skip: Vec<String>,

    /// Report format. json suits CI, markdown suits pasting into a thread
    #[arg(long, value_enum, default_value = "text", global = true)]
    pub format: Format,

    #[arg(
        long,
        global = true,
        help = "Write the report here; the only file preflight writes"
    )]
    pub out: Option<PathBuf>,

    /// Plain output with no colour codes
    #[arg(long, global = true)]
    pub no_color: bool,

    #[arg(long, global = true, help = "Probe under a prefixed root")]
    pub root: Option<PathBuf>,

    #[arg(
        long,
        global = true,
        help = "Read the validator command line from a file"
    )]
    pub invocation: Option<PathBuf>,

    #[arg(
        long,
        global = true,
        help = "Override client detection, e.g. agave-validator@4.2.1"
    )]
    pub client: Option<String>,

    #[arg(
        long,
        global = true,
        help = "Never execute anything, not even <validator> --version"
    )]
    pub no_exec: bool,

    #[arg(
        long,
        global = true,
        help = "Emit the full check table as markdown and exit"
    )]
    pub dump_registry: bool,

    #[arg(
        short,
        long,
        global = true,
        help = "Show passing and skipped checks in full"
    )]
    pub verbose: bool,
}

#[derive(Subcommand)]
pub enum Command {
    /// Print one check's documentation and sources, run nothing
    Explain {
        /// A check id, for example PF-KRN-0001
        id: String,
    },
}

fn selected(c: &Check, only: &[String], skip: &[String]) -> bool {
    let matches = |pats: &[String]| {
        pats.iter()
            .any(|p| c.id.eq_ignore_ascii_case(p) || Layer::parse(p).is_some_and(|l| l == c.layer))
    };
    if !only.is_empty() && !matches(only) {
        return false;
    }
    !matches(skip)
}

/// Ask which cluster to judge against, starting on whatever was inferred.
///
/// Only when a person is watching: never when piped, never in CI, never when
/// --profile already said.
fn ask_profile(inferred: Profile, reason: &str, st: &Style) -> Profile {
    let options = [
        (Profile::Testnet, "a voting validator on testnet"),
        (Profile::Mainnet, "a voting validator on mainnet"),
        (Profile::Local, "a test validator, not joining a cluster"),
    ];
    let rows: Vec<(&str, &str)> = options.iter().map(|(p, d)| (p.label(), *d)).collect();
    let start = options
        .iter()
        .position(|(p, _)| *p == inferred)
        .unwrap_or(0);
    let chosen = menu::select(
        &st.bold("Which are you asking about?"),
        &rows,
        start,
        &format!("inferred: {reason}"),
    );
    options[chosen].0
}

fn explain(id: &str, st: &Style, ctx: &Ctx) -> i32 {
    let Some(c) = registry::find(id) else {
        eprintln!("no such check: {id}");
        return 3;
    };
    // Metadata alone is the least useful view of a finding. Run it and print
    // what it actually says about this machine.
    let outcome = match c.applies_to(ctx.profile, ctx.client) {
        true => (c.run)(ctx),
        false => model::Outcome::skipped(format!(
            "not applicable to profile {} with client {}",
            ctx.profile.label(),
            ctx.client.label()
        )),
    };
    let f = Finding {
        id: c.id,
        phase: c.layer.phase(),
        layer: c.layer.label(),
        section: c.layer.human(),
        needs_linux: c.needs_a_linux_host(),
        severity: c.severity.label(),
        title: c.title,
        provisional: c.provisional(),
        outcome,
        source: c.source,
    };
    print!("{}", render::finding_block(&f, st));
    println!();
    println!("  layer      {}", c.layer.label());
    println!("  severity   {}", c.severity.label());
    let p: Vec<&str> = c.profiles.iter().map(|x| x.label()).collect();
    let cl: Vec<&str> = c.clients.iter().map(|x| x.label()).collect();
    println!("  profiles   {}", p.join(", "));
    println!("  clients    {}", cl.join(", "));
    println!(
        "  root       {}",
        if c.needs_root {
            "elevated reads required"
        } else {
            "no"
        }
    );
    if c.needs_root {
        for e in privilege::ALLOWLIST
            .iter()
            .filter(|e| e.used_by.contains(c.id))
        {
            println!("             $ sudo {}", e.command);
            println!("               {}", e.looking_for);
        }
    }
    if c.provisional() {
        println!("  status     provisional: sourced to an unreleased channel, cannot fire yet");
    }
    println!();
    for s in c.source {
        println!("  source     {} [{}]", s.locator, s.verified_against);
    }
    0
}

fn main() {
    let cli = Cli::parse();
    let color = !cli.no_color && std::io::stdout().is_terminal();
    let st = Style { color };

    if cli.dump_registry {
        println!("{}", render::markdown::dump_registry());
        return;
    }

    let ctx = Ctx::probe(CtxOptions {
        root: cli.root.clone(),
        profile: cli.profile.map(Into::into),
        invocation_file: cli.invocation.clone(),
        client_override: cli.client.clone(),
        no_exec: cli.no_exec,
    });

    // A person watching, with nothing pinned on the command line, gets asked.
    // Only ask when the box did not say. An entrypoint naming a cluster is an
    // answer, and asking anyway is friction for nothing.
    let interactive = cli.profile.is_none()
        && !ctx.profile_confident
        && cli.format == Format::Text
        && std::io::stdin().is_terminal()
        && std::io::stdout().is_terminal();
    let ctx = match interactive {
        true => {
            let chosen = ask_profile(ctx.profile, &ctx.profile_reason, &st);
            match chosen == ctx.profile {
                true => ctx,
                false => Ctx::probe(CtxOptions {
                    root: cli.root.clone(),
                    profile: Some(chosen),
                    invocation_file: cli.invocation.clone(),
                    client_override: cli.client.clone(),
                    no_exec: cli.no_exec,
                }),
            }
        }
        false => ctx,
    };

    if let Some(Command::Explain { id }) = &cli.command {
        std::process::exit(explain(id, &st, &ctx));
    }

    let mut findings = Vec::new();
    for c in registry::CHECKS {
        if !selected(c, &cli.only, &cli.skip) {
            continue;
        }
        let outcome = if c.needs_a_validator() && !ctx.validator_present {
            model::Outcome::skipped("no validator installed; this check reads one")
        } else if c.applies_to(ctx.profile, ctx.client) {
            (c.run)(&ctx)
        } else {
            model::Outcome::skipped(format!(
                "not applicable to profile {} with client {}",
                ctx.profile.label(),
                ctx.client.label()
            ))
        };
        findings.push(Finding {
            id: c.id,
            phase: c.layer.phase(),
            layer: c.layer.label(),
            section: c.layer.human(),
            needs_linux: c.needs_a_linux_host(),
            severity: c.severity.label(),
            title: c.title,
            provisional: c.provisional(),
            outcome,
            source: c.source,
        });
    }

    let needs_root = registry::CHECKS
        .iter()
        .filter(|c| {
            c.needs_root
                && selected(c, &cli.only, &cli.skip)
                && c.applies_to(ctx.profile, ctx.client)
        })
        .count();
    let body = match cli.format {
        Format::Json => render::json::render(&ctx, &findings),
        Format::Markdown => render::markdown::render(&ctx, &findings),
        Format::Text => {
            let hide_host = !ctx.is_linux() && !cli.verbose;
            render::report(&ctx, &findings, &st, cli.verbose, hide_host, needs_root)
        }
    };

    match &cli.out {
        Some(p) => {
            if let Err(e) = std::fs::write(p, &body) {
                eprintln!("could not write {}: {e}", p.display());
                std::process::exit(3);
            }
            eprintln!("report written to {}", p.display());
        }
        None => print!("{body}"),
    }

    std::process::exit(model::exit_code(&findings));
}
