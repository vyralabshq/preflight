//! The vocabulary every check speaks.
//!
//! A Check declares what it looks at and where its requirement comes from. An
//! Outcome is what it found: a status, what was observed, what was expected,
//! why it matters, and the fix to run by hand.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Pass,
    Ephemeral,
    Fail,
    Unsupported,
    Skipped,
    Unknown,
    /// Measured successfully, but nobody publishes a threshold to judge it
    /// against. Distinct from Unknown, which means the probe failed. A run
    /// full of these is complete, so it must not affect the exit code.
    Reported,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Status::Pass => "PASS",
            Status::Ephemeral => "EPHEMERAL",
            Status::Fail => "FAIL",
            Status::Unsupported => "UNSUPPORTED",
            Status::Skipped => "SKIPPED",
            Status::Unknown => "UNKNOWN",
            Status::Reported => "REPORTED",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Fatal,
    Degraded,
    Advisory,
}

impl Severity {
    pub fn label(self) -> &'static str {
        match self {
            Severity::Fatal => "fatal",
            Severity::Degraded => "degraded",
            Severity::Advisory => "advisory",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Layer {
    Arg,
    Xdp,
    Hw,
    Kernel,
    Limits,
    Fs,
    Net,
    Service,
    Validator,
    Security,
}

impl Layer {
    pub fn label(self) -> &'static str {
        match self {
            Layer::Arg => "ARG",
            Layer::Xdp => "XDP",
            Layer::Hw => "HW",
            Layer::Kernel => "KRN",
            Layer::Limits => "LIM",
            Layer::Fs => "FS",
            Layer::Net => "NET",
            Layer::Service => "SVC",
            Layer::Validator => "VAL",
            Layer::Security => "SEC",
        }
    }

    /// Which half of the report a layer belongs to. The machine question is
    /// answered before the validator question, because a box that cannot run a
    /// validator makes every finding about its configuration irrelevant.
    pub fn phase(self) -> Phase {
        match self {
            Layer::Hw | Layer::Kernel | Layer::Fs | Layer::Limits | Layer::Net => Phase::Machine,
            Layer::Arg | Layer::Xdp | Layer::Service | Layer::Validator | Layer::Security => {
                Phase::Validator
            }
        }
    }

    /// What this layer is, in words. The short codes are for citing a finding
    /// in a chat thread, not for reading a report.
    pub fn human(self) -> &'static str {
        match self {
            Layer::Arg => "Validator command line",
            Layer::Xdp => "XDP networking",
            Layer::Hw => "Hardware and OS",
            Layer::Kernel => "Kernel settings",
            Layer::Limits => "Process limits",
            Layer::Fs => "Disks and filesystems",
            Layer::Net => "Network",
            Layer::Service => "systemd service",
            Layer::Validator => "Validator install",
            Layer::Security => "Security",
        }
    }

    pub fn parse(s: &str) -> Option<Layer> {
        Some(match s.to_ascii_uppercase().as_str() {
            "ARG" => Layer::Arg,
            "XDP" => Layer::Xdp,
            "HW" => Layer::Hw,
            "KRN" => Layer::Kernel,
            "LIM" => Layer::Limits,
            "FS" => Layer::Fs,
            "NET" => Layer::Net,
            "SVC" => Layer::Service,
            "VAL" => Layer::Validator,
            "SEC" => Layer::Security,
            _ => return None,
        })
    }
}

/// What a profile is judged against.
///
/// Anza publishes one set of figures and never says which cluster they are for.
/// They describe a production node, so preflight treats them as the mainnet
/// baseline. Nobody publishes testnet figures at all, so testnet is judged on
/// headroom, which is the thing that actually bites, rather than on a size
/// somebody made up.
pub struct Thresholds {
    pub accounts_gb: Option<f64>,
    pub ledger_gb: Option<f64>,
    pub snapshots_gb: Option<f64>,
    /// Fraction of a filesystem that should still be free, on any cluster.
    pub min_free: f64,
    pub base_clock_mhz: Option<f64>,
}

impl Profile {
    pub fn thresholds(self) -> Thresholds {
        match self {
            Profile::Mainnet => Thresholds {
                accounts_gb: Some(1000.0),
                ledger_gb: Some(1000.0),
                snapshots_gb: Some(500.0),
                min_free: 0.15,
                base_clock_mhz: Some(2800.0),
            },
            Profile::Testnet => Thresholds {
                accounts_gb: None,
                ledger_gb: None,
                snapshots_gb: None,
                min_free: 0.10,
                base_clock_mhz: Some(2800.0),
            },
            Profile::Local => Thresholds {
                accounts_gb: None,
                ledger_gb: None,
                snapshots_gb: None,
                min_free: 0.05,
                base_clock_mhz: None,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Machine,
    Validator,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Profile {
    Local,
    Testnet,
    Mainnet,
}

impl Profile {
    pub fn label(self) -> &'static str {
        match self {
            Profile::Local => "local",
            Profile::Testnet => "testnet",
            Profile::Mainnet => "mainnet",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClientKind {
    AgaveValidator,
    TestValidator,
    Firedancer,
    Unknown,
}

impl ClientKind {
    pub fn label(self) -> &'static str {
        match self {
            ClientKind::AgaveValidator => "agave-validator",
            ClientKind::TestValidator => "solana-test-validator",
            ClientKind::Firedancer => "firedancer",
            ClientKind::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
#[allow(dead_code)]
pub enum SourceKind {
    AgaveSymbol,
    AgaveChangelog,
    AnzaDocs,
    AnzaBlog,
    Simd,
    Operator,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Source {
    pub kind: SourceKind,
    pub locator: &'static str,
    pub verified_against: &'static str,
    pub provisional: bool,
}

/// Whether a value survives a flag rename, and on whose authority.
///
/// Two live bugs were fixes that swapped a flag while saying nothing about the
/// value: one quoted a default read from the wrong struct, the other renamed a
/// flag whose name carried its unit without checking the replacement's. Making
/// the question unrepresentable-as-unanswered is cheaper than remembering.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum ValueCarry {
    /// Nothing was set, so nothing has to carry.
    NoValueSet,
    /// Same unit, same meaning. Unused today; kept so a future rename that has
    /// been verified has somewhere to say so rather than reaching for text.
    #[allow(dead_code)]
    Identical,
    /// Same unit, different scale, with the reason.
    #[allow(dead_code)]
    Converted(&'static str),
    /// Counts or measures a different thing.
    DifferentSemantics(&'static str),
    /// preflight has not verified this. Never renders a substitution.
    Unverified,
}

#[derive(Debug, Clone, Serialize)]
pub struct FixStep {
    pub command: String,
    pub note: Option<String>,
}

impl FixStep {
    pub fn cmd(c: impl Into<String>) -> Self {
        FixStep {
            command: c.into(),
            note: None,
        }
    }

    pub fn noted(c: impl Into<String>, n: impl Into<String>) -> Self {
        FixStep {
            command: c.into(),
            note: Some(n.into()),
        }
    }

    /// A flag rename, rendered according to what is known about the value.
    ///
    /// Unverified never prints an arrow: it names both flags and says to check,
    /// because a confident substitution nobody verified is how an operator ends
    /// up with the wrong number.
    pub fn rename(from: &str, to: &str, observed: Option<&str>, carry: ValueCarry) -> Self {
        match (carry, observed) {
            (ValueCarry::Unverified, Some(v)) => FixStep::noted(
                format!("{from} {v} is replaced by {to}"),
                "preflight has not read the replacement's unit from your binary, so it will not \
                 say what number goes there. Check both in agave-validator --help",
            ),
            (ValueCarry::Unverified, None) => FixStep::noted(
                format!("{from} is replaced by {to}"),
                "preflight has not verified whether a value carries across. Check both in \
                 agave-validator --help",
            ),
            (ValueCarry::NoValueSet, _) => {
                FixStep::noted(format!("{from}   ->   {to}"), "no value to convert")
            }
            (ValueCarry::Identical, Some(v)) => FixStep::cmd(format!("{from} {v}   ->   {to} {v}")),
            (ValueCarry::Identical, None) => FixStep::cmd(format!("{from}   ->   {to}")),
            (ValueCarry::Converted(note) | ValueCarry::DifferentSemantics(note), Some(v)) => {
                FixStep::noted(format!("{from} {v}   ->   {to} <see note>"), note)
            }
            (ValueCarry::Converted(note) | ValueCarry::DifferentSemantics(note), None) => {
                FixStep::noted(format!("{from}   ->   {to}"), note)
            }
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Persistence {
    pub found: Option<String>,
    pub expected: String,
}

impl Persistence {
    pub fn unit_dropin(found: Option<String>, unit: &str) -> Persistence {
        Persistence {
            found,
            expected: format!("/etc/systemd/system/{unit}.d/20-xdp-caps.conf"),
            // `unit` is whatever the host actually uses, or a placeholder when
            // preflight could not resolve one.
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Outcome {
    pub status: Status,
    pub observed: String,
    pub expected: String,
    pub why: String,
    pub fix: Vec<FixStep>,
    pub verify: Option<String>,
    pub persistence: Option<Persistence>,
}

impl Outcome {
    fn base(status: Status) -> Self {
        Outcome {
            status,
            observed: String::new(),
            expected: String::new(),
            why: String::new(),
            fix: Vec::new(),
            verify: None,
            persistence: None,
        }
    }

    fn new(status: Status, observed: impl Into<String>, expected: impl Into<String>) -> Self {
        Outcome {
            observed: observed.into(),
            expected: expected.into(),
            ..Outcome::base(status)
        }
    }

    pub fn pass(observed: impl Into<String>, expected: impl Into<String>) -> Self {
        Outcome::new(Status::Pass, observed, expected)
    }

    /// Wrong, and fixable.
    pub fn fail(observed: impl Into<String>, expected: impl Into<String>) -> Self {
        Outcome::new(Status::Fail, observed, expected)
    }

    /// Unmeetable on this hardware. Carries no fix because none exists.
    pub fn unsupported(observed: impl Into<String>, expected: impl Into<String>) -> Self {
        Outcome::new(Status::Unsupported, observed, expected)
    }

    /// Correct now, but nothing on disk restores it after a reboot.
    pub fn ephemeral(observed: impl Into<String>, expected: impl Into<String>) -> Self {
        Outcome::new(Status::Ephemeral, observed, expected)
    }

    pub fn expected(mut self, e: impl Into<String>) -> Self {
        self.expected = e.into();
        self
    }

    pub fn why(mut self, w: impl Into<String>) -> Self {
        self.why = w.into();
        self
    }

    pub fn fix(mut self, steps: Vec<FixStep>) -> Self {
        self.fix = steps;
        self
    }

    pub fn verify(mut self, v: impl Into<String>) -> Self {
        self.verify = Some(v.into());
        self
    }

    pub fn persists(mut self, p: Persistence) -> Self {
        self.persistence = Some(p);
        self
    }

    pub fn skipped(reason: impl Into<String>) -> Self {
        Outcome {
            observed: reason.into(),
            ..Outcome::base(Status::Skipped)
        }
    }

    /// Measured, with no published threshold to compare against.
    pub fn reported(observed: impl Into<String>, expected: impl Into<String>) -> Self {
        Outcome::new(Status::Reported, observed, expected)
    }

    pub fn unknown(reason: impl Into<String>) -> Self {
        Outcome {
            observed: reason.into(),
            ..Outcome::base(Status::Unknown)
        }
    }
}

pub type RunFn = fn(&crate::ctx::Ctx) -> Outcome;

pub struct Check {
    pub id: &'static str,
    pub layer: Layer,
    pub severity: Severity,
    pub title: &'static str,
    pub profiles: &'static [Profile],
    pub clients: &'static [ClientKind],
    pub needs_root: bool,
    /// True when the check exists to report a number rather than judge it,
    /// because no minimum is published. Its Unknown is by design, so it must
    pub source: &'static [Source],
    pub run: RunFn,
}

impl Check {
    pub fn provisional(&self) -> bool {
        self.source.iter().any(|s| s.provisional)
    }

    pub fn applies_to(&self, p: Profile, c: ClientKind) -> bool {
        self.profiles.contains(&p) && (c == ClientKind::Unknown || self.clients.contains(&c))
    }

    /// HW and KRN describe the machine; they answer "can this box run a
    /// validator" and must run on a host that has none yet. ARG and XDP read a
    /// validator's configuration and cannot.
    /// Layers that read the machine itself, so they need Linux.
    /// ARG reads a command line and works from text on any OS.
    pub fn needs_a_linux_host(&self) -> bool {
        matches!(
            self.layer,
            Layer::Hw | Layer::Kernel | Layer::Limits | Layer::Fs | Layer::Net | Layer::Service
        )
    }

    pub fn needs_a_validator(&self) -> bool {
        matches!(
            self.layer,
            Layer::Arg | Layer::Xdp | Layer::Service | Layer::Validator
        )
    }
}

#[derive(Serialize)]
pub struct Finding {
    pub id: &'static str,
    #[serde(skip)]
    pub phase: Phase,
    pub layer: &'static str,
    #[serde(skip)]
    pub section: &'static str,
    #[serde(skip)]
    pub needs_linux: bool,
    pub severity: &'static str,
    pub title: &'static str,
    pub provisional: bool,
    #[serde(flatten)]
    pub outcome: Outcome,
    pub source: &'static [Source],
}

pub fn exit_code(findings: &[Finding]) -> i32 {
    let s = |st: Status| findings.iter().any(|f| f.outcome.status == st);
    if s(Status::Fail) || s(Status::Unsupported) {
        1
    } else if s(Status::Ephemeral) {
        2
    } else if s(Status::Unknown) {
        4
    } else {
        0
    }
}
