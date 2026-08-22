# preflight

A read-only CLI that tells you whether a Linux box can run a Solana validator,
and if it cannot, exactly what is missing and how to fix it yourself.

Built and maintained by [vyralabshq](https://github.com/vyralabshq).

## Three promises

1. **It never writes to your system.** No installing, configuring or restarting. The only file it creates is a report you ask for with `--out`.
2. **It never runs a command you have not seen.** One command, ever: `<your validator> --version`, unprivileged, printed in every report. `--no-exec` disables even that. It never uses sudo. Checks needing an elevated read print the command for you to run.
3. **It never guesses.** If it cannot read something it says `UNKNOWN` and why. It prints no fix it is unsure of, and no threshold it cannot cite.

## What it answers

Two questions, in that order, because a box that cannot run a validator makes
every finding about a validator's configuration beside the point.

**Can this machine run a validator?** Works on a bare box with nothing
installed. Covers CPU, memory and architecture; the kernel values agave refuses
to start without; and disks, filesystems and free space.

**Is the validator configured correctly?** Needs one installed. Covers whether
the command line survived the last upgrade, and the Linux capabilities the v4.2
XDP transmit path requires.

30 checks today. Process limits, network, systemd and security are not built yet.

## What a run looks like

A fresh Ubuntu box, nothing installed, asked about a testnet validator.

```
preflight 0.1.0

SYSTEM
  cpu         AMD EPYC 9354P 32-Core Processor
              32 cores  ·  64 threads  ·  3800 MHz  ·  AVX2 yes
  memory      504 GB  ·  no swap
  disks       sda              500 GB  SSD or NVMe
  storage     /             ext4    free space not measured
              /mnt/accounts ext4    free space not measured
              /mnt/ledger   ext4    free space not measured
  os          Ubuntu 24.04.1 LTS  ·  kernel 6.8.0-31-generic
  validator   none installed
  preflight   read-only, running as uid 501, 1 check needs elevated reads

CAN THIS MACHINE RUN A TESTNET VALIDATOR?
  no. 4 things must be fixed first

  ...

Kernel settings

  PF-KRN-0001  net.core.rmem_max                               FAIL  fatal

  observed  net.core.rmem_max = 212992
  expected  net.core.rmem_max at or above 134217728
  why       agave refuses to start when this is below its recommendation, so
            the node does not boot. Catching it here saves a snapshot download.
  fix       echo 'net.core.rmem_max = 134217728' | sudo tee -a /etc/sysctl.d/21-agave-validator.conf
            sudo sysctl -p /etc/sysctl.d/21-agave-validator.conf
            (applies it now; the file is what makes it survive a reboot)
  verify    cat /proc/sys/net/core/rmem_max
  source    INTERESTING_LIMITS [v4.2.1] · check_os_network_limits() [v4.2.1]

  ...

IS THE VALIDATOR CONFIGURED CORRECTLY?
  no validator installed, nothing to check

next      4 fatal findings stop the validator from starting
          preflight explain <id>  for one finding on its own
```

## Install

```
cargo install --git https://github.com/vyralabshq/preflight
```

From a clone: `make install` puts it on your PATH, `make` lists the rest.

Nothing needs to go on the validator itself if you would rather not. See
`--invocation` and `--root` below.

## Commands

```
preflight                     check this machine
preflight explain PF-KRN-0001 one check's docs and sources, runs nothing
preflight --help              everything below
```

On a validator host, `preflight` with no arguments detects the client, its
version, where its command line lives, and what the machine is.

## Flags

| Flag | What it does |
|---|---|
| `--profile <local\|testnet\|mainnet>` | What the machine is judged against. Detected when not given |
| `--only <ids or layers>` | Run a subset: `--only ARG`, `--only PF-KRN-0001,FS` |
| `--skip <ids or layers>` | Same syntax, inverted |
| `--format <text\|json\|markdown>` | `json` for CI, `markdown` for pasting into a thread |
| `--out <path>` | Write the report to a file. The only file preflight writes |
| `--no-color` | Plain output |
| `-v`, `--verbose` | Show passing and skipped checks too |
| `--invocation <file>` | Read a command line from a file instead of the host, so you can check someone else's node from your laptop |
| `--client <name@version>` | Override client detection. Needed with `--invocation`, since text has no version to read |
| `--root <dir>` | Read a captured directory tree instead of this machine |
| `--no-exec` | Run nothing at all, then supply `--client` yourself |
| `--dump-registry` | Print every check with its source and exit |

## Exit codes and states

| Code | |
|---|---|
| 0 | everything applicable passed |
| 1 | a `FAIL`, or an `UNSUPPORTED` that no command can fix |
| 2 | an `EPHEMERAL`: correct now, gone after a reboot |
| 4 | an `UNKNOWN`: the run was incomplete, not clean |
| 3 | internal error |

`EPHEMERAL` and `UNSUPPORTED` are the two that matter. The first catches a
setting that works today and vanishes on reboot. The second is an honest no,
printed without a fix because none exists. Code 4 exists so an incomplete run
cannot be mistaken for a clean one.

## Every check is cited

Each names where its requirement comes from, an agave symbol or a section of a
named release's changelog, plus the version it was last verified against. A
check sourced to an unreleased channel is marked provisional and cannot fire
against a client that exists. `preflight --dump-registry` prints the full list.

## Status

Early, and honest about it. Every check is verified against fixtures, not yet
against a real validator host. That is the next thing.

preflight does not yet run anything with sudo. Checks needing an elevated read
say so and print the command. The allowlist they will come from is
[`src/privilege.rs`](src/privilege.rs), kept short enough to read in a minute.

`scripts/pf-dump.sh` captures a read-only snapshot of a host for building
fixtures. It redacts metrics credentials and never reads keypairs.

## Licence

Apache-2.0
