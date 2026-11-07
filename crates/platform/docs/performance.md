# platform performance and retesting

Hot paths in `platform` include process output, log tails, document fingerprints, directory reads, and disk scans. Implementations prefer system wait interfaces, chunked processing, and bounded caches while preserving cancellation and checks for file identity, links, and mounts.

## Implementation details

- **Process waiting:** Unix uses `poll`; Linux uses `pidfd` to wait for exit when available. Fully read pipes leave the wait set. Windows waits on the process handle while idle, checking cancellation after at most 20ms per iteration.
- **Log allocations:** When scanning a tail of at most 8 MiB, retain only the positions of needed lines before creating log objects, avoiding event, timestamp, and path-string allocations for old lines.
- **Document fingerprints:** Compute XXH3-64 from `xxhash-rust` in chunks, with a 64 KiB buffer for writes. XXH3 detects changes; release verification still uses SHA-256.
- **Recovery records:** New drafts save `saved_content_hash_algorithm = "xxh3-64-v1"`. Old drafts without this marker can still have their content read, but must be rebound to the target file when saved.
- **Directory reads:** Query attributes only once for regular files, and combine ELF and script-header checks into one open. Read file headers nonblockingly without following a final-component link.
- **Disk scans:** Linux uses one `statx` call to obtain type, size, block count, device, file ID, and mount ID. Directory cursors hold open handles; maximum depth is `min(256, max(1, RLIMIT_NOFILE - 32))`.

## Performance retesting

```sh
CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 cargo run --release --locked -p platform --example performance_probe
```

The program operates only in temporary directories and performs real process, document, and directory operations through public interfaces. Each item is warmed up once, then the median of 7 runs is taken, counting Rust allocations and reallocations as well.

Samples include a 9 MiB log, an 8 MiB tail read, 200 returned records, a 64 MiB document, and 3,000 regular files. Document reads use a warm cache; results do not represent cold disks or remote filesystems.

Retest pinned to a core:

```sh
CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 cargo build --release --locked -p platform --example performance_probe
taskset --cpu-list 0 target/release/examples/performance_probe
```

## Reference results

2026-10-11, Intel Core Ultra 9 275HX, WSL2, Rust 1.98.0, release build, comparing baseline `cf3592e` with implementation `56928f8`. Units are milliseconds.

| Item | Before optimization | After optimization |
| --- | ---: | ---: |
| Wait for and collect 4 MiB of process output | 805.870–861.546 | 1.134–1.262 |
| Read an 8 MiB log tail, returning 200 records | 553.182–565.547 | 12.223–12.788 |
| Fingerprint a 64 MiB document, including file read | 57.206–57.529 | 5.236–6.082 |
| Save a 64 MiB document in 1 KiB chunks, including synchronization | 108.130–111.699 | 38.148–38.684 |
| List 3,000 regular files and their opening policies | 8.625–9.261 | 4.692–4.769 |
| Scan 3,000 files and produce results | 5.189–5.397 | 4.610–4.687 |

Rust allocations for log queries fell from 4,976,297 to 7,031, and allocations for scans fell from 53,952 to 47,948. The difference in process waiting mainly comes from removing a fixed 20ms pause; it does not mean all programs benefit by the same proportion.

## Regression checks

```sh
cargo test --locked -p platform
cargo test --locked -p platform management::disks::scan:: -- --include-ignored --test-threads=1
cargo fmt --all --check
git diff --check
```

Targeted coverage includes reading two pipes, cancellation, log truncation, different write chunk sizes, preserving the original file on failure, old drafts, symbolic links, hard links, bind mounts, directory renaming, and low handle limits. Isolated mount tests use separate user and mount namespaces.
