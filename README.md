# preflight

A read-only CLI that tells you whether a Linux box can run a Solana validator,
and if it cannot, what is missing and how to fix it.

Built and maintained by [vyralabshq](https://github.com/vyralabshq).

## What it does

It answers two questions, in this order, because a machine that cannot run a
validator makes every question about a validator's configuration beside the
point.

**Can this machine run a validator?** Works on a bare box with nothing
installed. CPU, memory, disks, filesystems, free space, and the kernel values
agave refuses to start without.

**Is the validator configured correctly?** Needs one installed. Whether the
command line survived the last upgrade, and whether the Linux capabilities the
XDP transmit path needs actually reached the process.

36 checks: 7 hardware, 5 kernel, 7 filesystem, 1 network card, 14 command line,
2 XDP capability. Process limits, systemd and security are not built. Firedancer
is detected and skipped rather than checked.

## How it helps

Validator problems do not announce themselves. A sysctl below agave's floor stops
the node from starting, but only after a snapshot download. A renamed flag still
parses, so nothing looks wrong until the setting it controlled quietly stops
applying. A capability granted in the wrong systemd directive leaves the permitted
set empty and the node runs without the thing you set up.

preflight finds those before they cost you an outage, and cites where each
requirement comes from so you can check the claim rather than trust it.

It never writes to your system, never uses sudo, and runs exactly one command:
`<your validator> --version`, unprivileged and printed in every report.
`--no-exec` disables even that. When it cannot read something it says `UNKNOWN`
and why, rather than guessing.

## What you see

Every finding has the same shape: what is there, what should be, why it matters,
what to run, how to confirm it worked, and where the requirement comes from.

```
  PF-KRN-0001  net.core.rmem_max                                 FAIL  fatal

  observed  net.core.rmem_max = 212992
  expected  net.core.rmem_max at or above 134217728
  why       agave calls check_os_network_limits() before it opens the ledger and
            returns an error if this value is below its recommendation, so the
            validator refuses to start. It is not a tuning preference. The value
            preflight adds is catching it before a multi-hour snapshot download
            rather than after. This is the receive buffer for the UDP paths the
            validator ingests on.
  fix       echo 'net.core.rmem_max = 134217728' | sudo tee -a /etc/sysctl.d/21-agave-validator.conf
            sudo sysctl -p /etc/sysctl.d/21-agave-validator.conf
            (applies it now; the file is what makes it survive a reboot)
  verify    cat /proc/sys/net/core/rmem_max
  source    INTERESTING_LIMITS [v4.2.1] · check_os_network_limits() [v4.2.1]
```

A run opens with what the machine is and closes with what to do first. Passing
checks are listed by name, so you can see what was looked at.

## Install

```
cargo install --git https://github.com/vyralabshq/preflight
```

From a clone, `make install` puts it on your PATH and `make` lists the rest.

Nothing has to go on the validator itself. See `--invocation` and `--root`.

## Commands

```
preflight                       check this machine
preflight --profile mainnet     judge it against a different cluster
preflight -v                    show passing and skipped checks too
preflight explain PF-KRN-0001   one finding on its own
preflight --dump-registry       every check and its source, then exit
preflight --help                everything below
```

With no arguments it detects the client, its version, where its command line
lives, and what the machine is.

## Flags

| Flag | What it does |
|---|---|
| `--profile <local\|testnet\|mainnet>` | What the machine is judged against. Detected when not given |
| `--only <ids or layers>` | Run a subset: `--only ARG`, `--only PF-KRN-0001,FS`. `--skip` is the inverse |
| `--format <text\|json\|markdown>` | `json` for CI, `markdown` for pasting into a thread |
| `--out <path>` | Write the report to a file. The only file preflight writes |
| `--no-color` | Plain output |
| `--invocation <file>` | Read a command line from a file, so you can check someone else's node from your laptop |
| `--client <name@version>` | Override client detection. Needed with `--invocation`, since text carries no version |
| `--root <dir>` | Read a captured directory tree instead of this machine |
| `--no-exec` | Run nothing at all, then supply `--client` yourself |

## Exit codes

| Code | |
|---|---|
| 0 | everything applicable passed |
| 1 | a `FAIL`, or an `UNSUPPORTED` that no command can fix |
| 2 | an `EPHEMERAL`: correct now, gone after a reboot |
| 3 | internal error |
| 4 | an `UNKNOWN`: the run was incomplete, not clean |

`EPHEMERAL` catches a setting that works today and vanishes on the next reboot.
`UNSUPPORTED` is an honest no, printed without a fix because none exists. Code 4
exists so an incomplete run cannot be mistaken for a clean one.

## Every check is cited

Each names where its requirement comes from, an agave symbol or a section of a
named release's changelog, plus the version it was last verified against. Where
no source publishes a figure, the check says so and reports the number instead
of inventing a threshold. `preflight --dump-registry` prints the full list, and
[`docs/registry.md`](docs/registry.md) is that list committed, so a change to any
check's source or severity shows up as a diff.

## Status

Early. Checks run against fixtures in CI and against a live testnet validator.
Checks needing an elevated read print the command instead of running it; the
allowlist is [`src/privilege.rs`](src/privilege.rs). `scripts/pf-dump.sh`
captures a host snapshot for fixtures, redacting metrics credentials and never
reading keypairs.

## Licence

Apache-2.0
