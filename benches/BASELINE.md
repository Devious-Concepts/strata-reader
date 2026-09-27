# Initial benchmark observation

Measured 2026-09-27 from `02a3a97548ce89999a17821a0e6bf1c9bc3963e9`.
The subsequent documentation commit adds this record without changing the measured Rust source.
See the [methodology](README.md) for workloads and timing boundaries.

- Compiler: `rustc 1.98.1 (48a229cea 2026-09-01)`, LLVM 22.1.8.
- Target: `x86_64-unknown-linux-gnu`; Linux `7.2.7-zen1-1-zen`.
- CPU: Intel Core i9-11900K, advertised 3.50 GHz; CPU 0 governor reported `powersave`.
- Cargo's standard optimized bench profile, no custom compiler flags supplied for this run.
- Criterion 0.8.2; resolved dependencies captured in [baseline.lock](baseline.lock).
  SHA-256: `4196999a51c33b3f2fd4b59f66a0923d7d2f587659945a81c5a09b0d23ab7776`.
- No CPU pinning or frequency controls were applied. Background system load was not controlled.
  Other crate validation jobs were kept outside this timing run.

```bash
cargo bench --bench reader -- --warm-up-time 1 --measurement-time 2 \
  --sample-size 30 --save-baseline initial
```

All correctness preflights passed. Values below are nanoseconds per iteration (the whole input
for sequential/records/growth, one operation for retention), with Criterion's point estimate
and 95% confidence interval. These intervals quantify this run, not variation across machines
or independent sessions. Thirty samples and short measurement windows make this an initial
observation; use the normal defaults and repeated sessions for performance decisions.

| Case | Estimate (ns) | 95% interval (ns) |
| --- | ---: | ---: |
| `growth/32768` | 476.259 | 471.973–481.084 |
| `growth/8192` | 547.073 | 542.620–551.943 |
| `records/bufreader_copied/64` | 806.948 | 801.064–812.949 |
| `records/bufreader_copied/8192` | 38567.811 | 38173.108–39056.957 |
| `records/bufreader_copied/8193` | 42270.312 | 41687.736–42890.545 |
| `records/reader_borrowed/64` | 2980.378 | 2930.393–3023.858 |
| `records/reader_borrowed/8192` | 212400.774 | 210474.191–214491.968 |
| `records/reader_borrowed/8193` | 209273.212 | 206962.038–211140.226 |
| `retention/compact/57344` | 225.688 | 224.674–226.699 |
| `retention/compact/8192` | 1231.039 | 1201.389–1272.170 |
| `retention/compact_and_shrink/57344` | 253.056 | 250.285–256.460 |
| `retention/compact_and_shrink/8192` | 1172.395 | 1138.685–1222.816 |
| `retention/take_consumed/57344` | 337.708 | 333.674–343.130 |
| `retention/take_consumed/8192` | 2383.451 | 2367.805–2400.389 |
| `sequential/bufreader/1024` | 43.327 | 42.844–43.810 |
| `sequential/bufreader/1048576` | 25605.829 | 25498.047–25709.812 |
| `sequential/bufreader/8192` | 123.584 | 122.709–124.289 |
| `sequential/reader/1024` | 86.998 | 86.154–87.932 |
| `sequential/reader/1048576` | 27561.419 | 27377.502–27772.000 |
| `sequential/reader/8192` | 240.371 | 239.558–241.027 |

`BufReader` took less time in the measured sequential and record pairs on this host. The record
recipes have different ownership behavior and search implementations; the measurement does not
identify which component explains the difference. The growth case includes initial allocation,
so it does not isolate reallocation cost. Compaction and compact-then-shrink intervals overlap
for the smaller consumed prefix; do not infer that adding a shrink makes compaction faster.
No result establishes a universal ordering, allocator count, memory footprint, or CI threshold.
