| id | layer | severity | profiles | clients | check | source | verified |
|---|---|---|---|---|---|---|---|
| `PF-HW-0001` | HW | fatal | testnet mainnet | agave-validator solana-test-validator firedancer unknown | CPU architecture is x86-64 | docs.anza.xyz/operations/requirements | 2026-08 |
| `PF-HW-0002` | HW | fatal | testnet mainnet | agave-validator solana-test-validator firedancer unknown | CPU supports AVX2 | docs.anza.xyz/operations/requirements | 2026-08 |
| `PF-HW-0003` | HW | degraded | testnet mainnet | agave-validator solana-test-validator firedancer unknown | CPU base clock is 2.8 GHz or faster | docs.anza.xyz/operations/requirements | 2026-08 |
| `PF-HW-0004` | HW | advisory | testnet mainnet | agave-validator solana-test-validator firedancer unknown | CPU core and thread count | docs.anza.xyz/operations/requirements<br>solanahcl.org, agave CPU list | 2026-08<br>2026-08 |
| `PF-HW-0005` | HW | degraded | local testnet mainnet | agave-validator solana-test-validator firedancer unknown | Installed memory | docs.anza.xyz/operations/requirements | 2026-08 |
| `PF-HW-0006` | HW | degraded | testnet mainnet | agave-validator solana-test-validator firedancer unknown | CPU is one somebody has reported on | solanahcl.org, agave CPU list | 2026-08 |
| `PF-HW-0007` | HW | advisory | testnet mainnet | agave-validator solana-test-validator firedancer unknown | Operating system is still in standard support | ubuntu.com/about/release-cycle | 2026-08 |
| `PF-KRN-0001` | KRN | fatal | local testnet mainnet | agave-validator solana-test-validator firedancer unknown | net.core.rmem_max | INTERESTING_LIMITS<br>check_os_network_limits() | v4.2.1<br>v4.2.1 |
| `PF-KRN-0002` | KRN | fatal | local testnet mainnet | agave-validator solana-test-validator firedancer unknown | net.core.wmem_max | INTERESTING_LIMITS<br>check_os_network_limits() | v4.2.1<br>v4.2.1 |
| `PF-KRN-0003` | KRN | fatal | local testnet mainnet | agave-validator solana-test-validator firedancer unknown | vm.max_map_count | INTERESTING_LIMITS<br>check_os_network_limits() | v4.2.1<br>v4.2.1 |
| `PF-KRN-0004` | KRN | fatal | local testnet mainnet | agave-validator solana-test-validator firedancer unknown | fs.nr_open | docs.anza.xyz/operations/setup-a-validator, System Tuning | 2026-08 |
| `PF-KRN-0005` | KRN | degraded | testnet mainnet | agave-validator | Kernel carries the XDP mode in use | anza.xyz/blog/agave-xdp-setup-guide, zero copy kernel versions | 2026-08 |
| `PF-FS-0001` | FS | degraded | testnet mainnet | agave-validator solana-test-validator firedancer unknown | Storage capacity and headroom | docs.anza.xyz/operations/requirements, Disk Storage<br>headroom figure is preflight's own, not published | 2026-08<br>2026-08 |
| `PF-FS-0002` | FS | degraded | testnet mainnet | agave-validator solana-test-validator firedancer unknown | Accounts and ledger on separate devices | docs.anza.xyz/operations/requirements, Disk Storage | 2026-08 |
| `PF-FS-0003` | FS | degraded | testnet mainnet | agave-validator solana-test-validator firedancer unknown | Storage is solid-state and local | docs.anza.xyz/operations/requirements, Disk Storage | 2026-08 |
| `PF-FS-0004` | FS | advisory | testnet mainnet | agave-validator solana-test-validator firedancer unknown | noatime on validator filesystems | operator practice, not published by Anza | 2026-08 |
| `PF-FS-0005` | FS | advisory | testnet mainnet | agave-validator solana-test-validator firedancer unknown | Filesystem is ext4 or xfs | docs.anza.xyz/operations/requirements, Disk Storage | 2026-08 |
| `PF-FS-0006` | FS | degraded | testnet mainnet | agave-validator solana-test-validator firedancer unknown | Accounts filesystem supports direct I/O | v4.0 Validator/Changes | v4.2.1 |
| `PF-FS-0007` | FS | degraded | testnet mainnet | agave-validator | Ledger retention fits the disk holding it | DEFAULT_MAX_BLOCKSTORE_SHREDS and the sizing comment above it in cleanup_service.rs | agave master |
| `PF-NET-0001` | NET | degraded | testnet mainnet | agave-validator solana-test-validator firedancer unknown | Network driver carries the XDP transmit path | solanahcl.org, network card list | 2026-08 |
| `PF-ARG-0001` | ARG | fatal | testnet mainnet | agave-validator | Dynamic port range is wide enough | MINIMUM_VALIDATOR_PORT_RANGE_WIDTH<br>v4.1 Validator/Breaking | v4.2.1<br>v4.2.1 |
| `PF-ARG-0002` | ARG | fatal | local testnet mainnet | agave-validator | Private addressing requires --no-xdp | v4.2 Validator/Changes | v4.2.1 |
| `PF-ARG-0003` | ARG | fatal | local testnet mainnet | agave-validator solana-test-validator | No arguments removed in v4.0 | v4.0 Validator/Breaking | v4.2.1 |
| `PF-ARG-0004` | ARG | fatal | testnet mainnet | agave-validator | Block verification method is supported | v4.0 Validator/Breaking | v4.2.1 |
| `PF-ARG-0005` | ARG | degraded | testnet mainnet | agave-validator | Block production method is supported | v4.1 Validator/Breaking | v4.2.1 |
| `PF-ARG-0006` | ARG | degraded | testnet mainnet | agave-validator | No deprecated experimental XDP flags | v4.1 Validator/Deprecations | v4.2.1 |
| `PF-ARG-0007` | ARG | degraded | testnet mainnet | agave-validator | PoH pinned core flag uses the current name | v4.2 Validator/Deprecations | v4.2.1 |
| `PF-ARG-0008` | ARG | degraded | testnet mainnet | agave-validator | No deprecated accounts-db arguments | v4.1 Validator/Deprecations<br>v4.2 Validator/Deprecations | v4.2.1<br>v4.2.1 |
| `PF-ARG-0009` | ARG | degraded | testnet mainnet | agave-validator | Accounts index limit is set explicitly | v4.1 Validator/Deprecations | v4.2.1 |
| `PF-ARG-0010` | ARG | degraded | testnet mainnet | agave-validator | Direct I/O setting matches the accounts filesystem | v4.0 Validator/Changes | v4.2.1 |
| `PF-ARG-0011` | ARG | degraded | testnet mainnet | agave-validator | Ledger size limit uses the current flag | BlockstoreCleanupStrategy, CountDataShreds vs CountDataAndCodingShreds<br>LEGACY_DEFAULT_MAX_LEDGER_SHREDS 200000000, DEFAULT_MAX_BLOCKSTORE_SHREDS 400000000<br>--limit-blockstore-size, present in agave-validator --help | agave master<br>agave master<br>4.3.0-beta.0 |
| `PF-ARG-0012` | ARG | advisory | testnet mainnet | agave-validator | Banking trace flag still expresses something *(provisional)* | 4.3.0-Unreleased Validator/Deprecations | master 2026-08 |
| `PF-ARG-0013` | ARG | fatal | testnet mainnet | agave-validator | No removed TPU connection pool argument | --tpu-connection-pool-size, absent from agave-validator --help | 4.3.0-beta.0 |
| `PF-ARG-0014` | ARG | degraded | local testnet mainnet | agave-validator | Every flag that takes a value has one | value placeholders in agave-validator --help | 4.3.0-beta.0 |
| `PF-XDP-0001` | XDP | degraded | local testnet mainnet | agave-validator | XDP capabilities are in the permitted set | v4.0 Validator/Breaking (#9133)<br>v4.2 Validator/Breaking | v4.2.1<br>v4.2.1 |
| `PF-XDP-0007` | XDP | degraded | testnet mainnet | agave-validator | Capabilities come from the unit, not from setcap | v4.0 Validator/Breaking (#9133) | v4.2.1 |

