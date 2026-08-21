# preflight

A read-only CLI that tells you whether a Linux box can run a Solana validator, and if it cannot, exactly what is missing and how to fix it yourself.

Built and maintained by [vyralabshq](https://github.com/vyralabshq).

## Three promises

1. **preflight never writes to your system.** It does not install, configure, restart, or create files anywhere except a report file you ask for with `--out`. That file is data, not an executable.
2. **preflight never runs a command you have not seen.** Today it runs exactly one thing, ever: `<your validator> --version`, unprivileged, and the command is printed in the header of every report. `--no-exec` disables even that. It never runs anything with sudo. Checks that would need elevated reads report what they can see as your user and print the command for you to run yourself. The allowlist those commands will come from is [`src/privilege.rs`](src/privilege.rs), kept short so it can be read in a minute.
3. **preflight never guesses.** If it cannot read something it says `UNKNOWN` and why. It does not print a fix it is not sure about, and it does not print a threshold it cannot cite.

## What it currently checks

**Can this machine run a validator?** The `HW` and `KRN` layers answer that on a
box with nothing installed yet. Architecture and AVX2 come back `UNSUPPORTED`
when the hardware simply cannot, with no fix offered, because none exists. The
four kernel values agave itself refuses to start without are checked twice: the
running value, and the file that restores it after a reboot. Correct-but-unsaved
is `EPHEMERAL`, not `PASS`.

```
preflight --profile testnet      on a bare box, before you install anything
```

**Did the last upgrade break my command line?** The `ARG` and `XDP` layers
answer that on a box that already runs one.

Between v4.0 and v4.2, Agave removed eleven validator arguments outright, stopped supporting two `--block-*-method` values, raised the required `--dynamic-port-range` width to 26, and deprecated seven more flags. Every one of those is detectable by reading a command line, and nothing else checks it.

Anza's own validator-start guide still shows `--dynamic-port-range 11000-11020`. That is 20 wide. `MINIMUM_VALIDATOR_PORT_RANGE_WIDTH` is 26, and agave validates it at startup, so a node built from the guide does not start.

## What a run looks like

A fresh Ubuntu box with nothing installed, asked whether it could run a testnet
validator. Stock kernel values, one 500 GB disk.

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

  ...

IS THE VALIDATOR CONFIGURED CORRECTLY?
  no validator installed, nothing to check


next      4 fatal findings stop the validator from starting: PF-FS-0001, PF-KRN-0001, PF-KRN-0002, PF-KRN-0003
          fix those first, then re-run
          the other 1 is drift: the node runs, but not as configured
          preflight explain <id>  for one finding on its own
```

## Install

```
cargo install --git https://github.com/vyralabshq/preflight
```

Nothing needs to be installed on the validator itself if you would rather not.
See `--invocation` and `--root` below.

## Commands

```
preflight                     check this machine
preflight explain PF-KRN-0001 one check's documentation and sources, runs nothing
preflight --help              everything below
```

That is the whole interface. On a validator host, `preflight` with no arguments
detects the client, its version, where its command line lives, and what the
machine is, then reports against all of it.

## Flags

| Flag                                  | What it does                                                                                                                                                 |
| ------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `--profile <local\|testnet\|mainnet>` | What the machine is judged against. Detected when not given. On a box with no validator yet, `testnet` is what asks whether it could run a real voting node. |
| `--only <ids or layers>`              | Run a subset. Takes check ids or layer codes: `--only ARG`, `--only PF-KRN-0001,FS`                                                                          |
| `--skip <ids or layers>`              | Same syntax, inverted                                                                                                                                        |
| `--format <text\|json\|markdown>`     | `json` for CI or an `ExecStartPre=`, `markdown` for pasting into a thread                                                                                    |
| `--out <path>`                        | Write the report to a file. The only file preflight ever writes                                                                                              |
| `--no-color`                          | Plain output                                                                                                                                                 |
| `-v`, `--verbose`                     | Show passing and skipped checks too, not just findings                                                                                                       |
| `--invocation <file>`                 | Read a validator command line from a file instead of the host. Lets you check someone else's node from your laptop                                           |
| `--client <name@version>`             | Override client detection, for example `agave-validator@4.2.1`. Needed with `--invocation`, since text has no version to read                                |
| `--root <dir>`                        | Read a captured directory tree instead of this machine. How the tests work, and how you check a box you have a capture from                                  |
| `--no-exec`                           | Never execute anything, not even `<validator> --version`. You then supply `--client` yourself                                                                |
| `--dump-registry`                     | Print every check with its source and exit                                                                                                                   |

## Exit codes

| Code | Meaning                                          |
| ---- | ------------------------------------------------ |
| 0    | everything applicable passed                     |
| 1    | a `FAIL` or `UNSUPPORTED`                        |
| 2    | an `EPHEMERAL`: correct now, gone after a reboot |
| 4    | an `UNKNOWN`: the run was incomplete, not clean  |
| 3    | internal error                                   |

Code 4 exists so a run with declined privileges or an unreadable host cannot be
mistaken for a green result.

## Result states

| State         | Meaning                                                                     |
| ------------- | --------------------------------------------------------------------------- |
| `PASS`        | correct, and it will survive a reboot                                       |
| `EPHEMERAL`   | correct now, but nothing on disk restores it                                |
| `FAIL`        | wrong, and fixable                                                          |
| `UNSUPPORTED` | cannot be satisfied on this hardware. No fix is printed because none exists |
| `SKIPPED`     | does not apply to this profile, client, or version                          |
| `UNKNOWN`     | could not be read. preflight says why rather than guessing                  |

## Layers

| Code  | What it covers                                     | Needs a validator installed |
| ----- | -------------------------------------------------- | --------------------------- |
| `HW`  | CPU, memory, architecture, AVX2                    | no                          |
| `KRN` | kernel values agave refuses to start without       | no                          |
| `FS`  | disks, filesystems, free space, direct I/O         | no                          |
| `ARG` | whether the command line survived the last upgrade | yes                         |
| `XDP` | capabilities the v4.2 transmit path requires       | yes                         |

### Finding the invocation

Anza's guide puts `ExecStart=/home/sol/bin/validator.sh` in the unit and `exec agave-validator ...` inside the script, so preflight resolves in this order:

1. `/proc/<pid>/cmdline` of a running validator
2. the unit's `ExecStart`, if it names the binary directly
3. the wrapper script `ExecStart` points at
4. failure, reported as `UNKNOWN` with the full resolution trail

Unexpanded `$VAR` tokens are reported, never guessed.

## Every check is cited

Every check names where its requirement comes from: an agave symbol, or a
section of a named release's changelog, along with the client version it was
last verified against. A check sourced to an unreleased channel is marked
provisional and cannot fire against a client that exists.

You see the citation on any finding, and `preflight explain PF-KRN-0001` prints
one check on its own. For the full list:

```
preflight --dump-registry
```

## Collecting a fixture

`scripts/pf-dump.sh` gathers a read-only snapshot of a validator host for building test fixtures. It redacts `SOLANA_METRICS_CONFIG` credentials and never reads keypair contents. Review the output before sharing it.

```
./scripts/pf-dump.sh              # unprivileged reads only
./scripts/pf-dump.sh --with-sudo  # adds ethtool, capabilities, sockets, firewall
```

## Status

Early. `HW`, `KRN`, `FS`, `ARG` and the command-line half of `XDP` are in. Still to build: the XDP checks that need `ethtool`, plus process limits, network, systemd and security.

Every check is verified against fixtures, not yet against a real validator host. That is the next thing.

preflight does not yet run anything with sudo. Checks needing an elevated read say so and print the command; the batched prompt described in the build plan is not built. `preflight explain <id>` shows the exact commands a check would use.

## Licence

Apache-2.0
