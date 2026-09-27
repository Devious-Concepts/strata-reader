# Reader benchmarks

Run from the crate root:

```bash
# Execute every workload once and check its results, without collecting timings.
cargo bench --bench reader -- --test
# Collect measurements with Criterion's normal sampling defaults.
cargo bench --bench reader
# Filter to one group or case when investigating a change.
cargo bench --bench reader -- retention
```

The [harness](reader.rs) uses [Criterion](https://docs.rs/criterion/0.8.2/criterion/) as a
development dependency, with plotting and parallel analysis features disabled. There are no
runtime dependencies or sibling crate requirements. `cargo test --all-targets --all-features`
runs Criterion's test mode as part of the fast checks. Rust 1.87.0 remains the declared minimum;
check both the library and all targets there when updating development dependencies.

## What each group measures

| Group | Input and work | Timing boundary |
| --- | --- | --- |
| `sequential` | 1 KiB, 8 KiB, and 1 MiB of cycling byte values; identical 1 KiB `Read` calls through `Reader` and `BufReader`, both starting with 8 KiB buffers | Reader construction, allocation, copying to the output array, EOF, and destruction are timed; generating source bytes is not |
| `records` | 64 LF-terminated records, each 64, 8192, or 8193 bytes including LF; process every record | Construction, filling/searching, consumption/compaction or copying, EOF, and destruction are timed; generating input is not |
| `retention` | A filled 64 KiB reader, with 8 KiB or 56 KiB consumed; compact, compact then shrink, or transfer the consumed prefix | Fixture construction/filling and final input/output destruction are excluded by `iter_batched_ref`; allocation/copying/shrinking performed by the operation itself are included |
| `growth` | Fill 24 KiB through EOF with an 8 KiB initial buffer versus a 32 KiB initial buffer, both capped at 32 KiB | Construction, allocation/initialization, growth, filling, completion-time shrinking, and destruction are timed |

The sequential pair does equivalent work. The records pair produces the same record bytes but
uses different ownership strategies: reader exposes retained slices and compacts, whereas
`BufReader::read_until` copies each record into a reused `Vec`. It measures those complete
recipes, including their searches; it is not an isolated comparison of search algorithms.
The 8193-byte records cross the initial capacity boundary and force reader growth.

Retention cases offer different guarantees. `compact` discards lookbehind while retaining
capacity. `compact_and_shrink` additionally releases spare capacity. `take_consumed` returns an
owned prefix and preserves unread bytes in a replacement buffer. Their timings help choose a
lifecycle when ownership requirements permit it, not rank equivalent operations. Transfer is
not assumed to be allocation-free, and the returned buffer's later destruction is not timed.
Batches contain 16 fixtures, keeping their input storage near 1 MiB rather than allowing it to
grow with the iteration count. Timing still includes harness/batching overhead.

Before timing, exact byte assertions validate both sequential and record recipes, the retained
prefix/suffix, compact/shrink results, and growth fills. Timed loops pass processed slices to
`std::hint::black_box`; correctness assertions are outside the timed region. This keeps the
benchmark from silently measuring a path that skips input.

## Reproduction and interpretation

[The initial run](BASELINE.md) records the measured source revision, compiler, host, invocation,
and confidence intervals. It is a starting observation, not a speed promise or CI threshold.
Criterion stores samples and estimates under `target/criterion/`. To compare on the same host:

```bash
cargo bench --bench reader -- --save-baseline before
# Change the implementation, keeping workloads and configuration equivalent.
cargo bench --bench reader -- --baseline before
```

Record `rustc -Vv`, CPU/OS, source revision, dependency resolution, command, power settings, and
other load when sharing results. The library leaves the root `Cargo.lock` untracked; keep the
resolved lockfile with benchmark evidence so a dependency update is not mistaken for a library
change. The initial run's [lockfile](baseline.lock) can be copied to root `Cargo.lock` in a
separate checkout to reproduce that resolution, then use `cargo bench --locked`.

These are warm in-memory `Cursor` workloads. They do not model disk/network latency, short reads,
concurrent users, or error recovery. They measure elapsed time and, where configured, source
bytes per second. They do not measure allocations, peak live bytes, RSS, or prove universal
superiority over `BufReader`. Capacity assertions are correctness checks, not memory profiles.
For tuning, rerun on representative hardware and input distributions; do not infer a general
performance claim from one noisy run.
