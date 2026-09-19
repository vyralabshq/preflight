# preflight

A read-only CLI that tells you whether a Linux box can run a Solana validator,
and if it cannot, what is missing and how to fix it.

Built and maintained by [vyralabshq](https://github.com/vyralabshq).

## What it does

Two questions, in this order, because a machine that cannot run a validator
makes every question about its configuration beside the point.

**Can this machine run a validator?** Works on a bare box with nothing
installed. CPU, memory, disks, filesystems, free space, and the kernel values
agave refuses to start without.

**Is the validator configured correctly?** Needs one installed. Whether the
command line survived the last upgrade, whether the systemd unit depends on the
disks it writes to, and whether the Linux capabilities the XDP transmit path
needs actually reached the process.

40 checks: 8 hardware, 5 kernel, 6 filesystem, 3 network card, 14 command line,
1 systemd unit, 3 XDP capability. Process limits and security are not built.
Firedancer is detected and skipped rather than checked.

Four only ever report a number, because nobody publishes a figure to judge it
against: core count, memory, whether your CPU is one somebody has measured, and
storage headroom.

It does not tell you whether the node is keeping up. Every check reads
configuration; skip rate and replay timing need the cluster, and preflight makes
no network calls.

## How it helps

Validator problems do not announce themselves. A renamed flag still parses, so
nothing looks wrong until the setting it controlled quietly stops applying. A
power saving CPU governor holds your cores below their own base clock while every
tool on the box reports the machine healthy.

It never writes to your system, never uses sudo, and runs two commands, both
against your own validator binary and both printed in the report:
`--version` and `--help`. The help text is read for flag existence only, never
for default values. `--no-exec` disables both. When it cannot read something it
says `UNKNOWN` and why.

## What you see

Every finding has the same shape: what is there, what should be, why it matters,
what to run, how to confirm it, and where the requirement comes from.

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

## Every check is cited

Each names its source: an agave symbol, a section of a named changelog, or the
doc page, plus the version it was verified against. Where nobody publishes a
figure the check says so rather than inventing a threshold.
`preflight --dump-registry` prints the list; [`docs/registry.md`](docs/registry.md)
is it committed, so a change to any source or severity shows up as a diff.

## Status

Early. Checks run against fixtures in CI and against a live testnet validator.
A check needing an elevated read prints the command rather than running it.
`scripts/pf-dump.sh` captures a host snapshot for fixtures, redacting metrics
credentials and never reading keypairs.

## Licence

Apache-2.0
