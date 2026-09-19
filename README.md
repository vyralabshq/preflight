# preflight

A read-only CLI that tells you whether a Linux box can run a Solana validator,
and if it cannot, what is missing and how to fix it.

Built and maintained by [vyralabshq](https://github.com/vyralabshq).

## What it does

Two questions, in this order.

**Can this machine run a validator?** Works on a bare box with nothing
installed. CPU, memory, disks, filesystems, free space, and the kernel values
agave refuses to start without.

**Is the validator configured correctly?** Needs one installed. Whether the
command line survived the last upgrade, whether the systemd unit depends on the
disks it writes to, and whether the Linux capabilities the XDP transmit path
needs reached the process.

41 checks: 8 hardware, 5 kernel, 6 filesystem, 3 network card, 15 command line,
1 systemd unit, 3 XDP capability. Process limits and security are not built.
Firedancer is detected and skipped.

Four report a number without judging it, because nobody publishes a threshold:
core count, memory, whether your CPU is one somebody has measured, and storage
headroom.

It does not tell you whether the node is keeping up. Skip rate and replay timing
need the cluster, and preflight makes no network calls.

## How it helps

Some misconfigurations are silent. A renamed flag still parses and the setting
it controlled stops applying. A power saving CPU governor holds your cores below
their base clock. Neither raises an error.

It never writes to your system and never uses sudo. It runs two commands against
your own validator binary, both printed in the report: `--version` and `--help`.
The help text is read for flag existence only, never for defaults. `--no-exec`
disables both. When it cannot read something it says `UNKNOWN` and why.

## What you see

Every finding: what is there, what should be, why it matters, what to run, how
to confirm it, where the requirement comes from.

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

A run closes with the two questions, each naming the finding behind it:

```
CAN THIS MACHINE RUN A TESTNET VALIDATOR?
  yes, with 1 thing worth fixing:  PF-HW-0007 operating system is still in standard support

IS THE VALIDATOR CONFIGURED CORRECTLY?
  no. 1 requirement not met:  PF-ARG-0011 ledger size limit uses the current flag

2 fail · 1 reported · 37 pass
```

Passing checks are listed by name, so you can see what was looked at.

## Install

```
curl -fsSL https://github.com/vyralabshq/preflight/releases/latest/download/preflight-x86_64-linux -o preflight && chmod +x preflight && ./preflight
```

One static binary. No toolchain, no build, nothing left on the box. Delete it
when you are done.

Verify the download instead:

```
base=https://github.com/vyralabshq/preflight/releases/latest/download
curl -fsSLO $base/preflight-x86_64-linux -O $base/preflight-x86_64-linux.sha256
sha256sum -c preflight-x86_64-linux.sha256 && chmod +x preflight-x86_64-linux
```

Or build it, which needs Rust 1.88 or newer:

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

| Flag                                  | What it does                                                                           |
| ------------------------------------- | -------------------------------------------------------------------------------------- |
| `--profile <local\|testnet\|mainnet>` | What the machine is judged against. Detected when not given                            |
| `--only <ids or layers>`              | Run a subset: `--only ARG`, `--only PF-KRN-0001,FS`. `--skip` is the inverse           |
| `--format <text\|json\|markdown>`     | `json` for CI, `markdown` for pasting into a thread                                    |
| `--out <path>`                        | Write the report to a file. The only file preflight writes                             |
| `--no-color`                          | Plain output                                                                           |
| `--invocation <file>`                 | Read a command line from a file, so you can check someone else's node from your laptop |
| `--client <name@version>`             | Override client detection. Needed with `--invocation`, since text carries no version   |
| `--root <dir>`                        | Read a captured directory tree instead of this machine                                 |
| `--no-exec`                           | Run nothing at all, then supply `--client` yourself                                    |

## Exit codes

| Code |                                                       |
| ---- | ----------------------------------------------------- |
| 0    | everything applicable passed                          |
| 1    | a `FAIL`, or an `UNSUPPORTED` that no command can fix |
| 2    | an `EPHEMERAL`: correct now, gone after a reboot      |
| 3    | internal error                                        |
| 4    | an `UNKNOWN`: the run was incomplete                  |

## Every check is cited

Each names its source: an agave symbol, a changelog section, or the doc page,
plus the version it was verified against. Where nobody publishes a figure the
check says so. It does not invent one.

`preflight --dump-registry` prints the list. [`docs/registry.md`](docs/registry.md)
is that list committed, so a changed source or severity shows up as a diff.

## Status

Early. Checks run against fixtures in CI and against a live testnet validator.
A check needing an elevated read prints the command for you to run.
`scripts/pf-dump.sh` captures a host snapshot, redacting metrics credentials and
never reading keypairs.

## Licence

Apache-2.0
