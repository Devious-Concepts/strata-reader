# Reader benchmark results

These measurements answer four separate questions: wrapper overhead, record-processing strategy,
retained-prefix ownership, and initial capacity. The comparisons below use two complete runs of
the same code on one machine. They replace the first submission's mixed comparison table.

**How to read the tables:** times are point estimates from run A, per complete input or operation
as stated in each section. Ratio columns show **A / B**, the same comparison repeated in run B.
A time ratio above 1 means the numerator took longer. Ratios use unrounded estimates.
[results.csv](results.csv) contains all 82 estimates and their 95% confidence intervals in
nanoseconds; the displayed A/B ratios are repeat observations, not confidence intervals.

## 1. Sequential reads

**Question:** what does a wrapper add when reading an in-memory source with the same copy loop?
Both wrappers start at 8 KiB. The bare Cursor has no buffer allocation. Each iteration reads the
entire input, including construction, the fixed output array, EOF, and destruction.

| Read call size | Input | Cursor | Reader | BufReader | Reader / BufReader (A / B) |
| --- | --- | ---: | ---: | ---: | ---: |
| 1 KiB | 1 KiB | 14.1 ns | 84.4 ns | 43.8 ns | 1.93× / 1.97× |
| 1 KiB | 8 KiB | 81.4 ns | 202.4 ns | 137.8 ns | 1.47× / 1.37× |
| 1 KiB | 1 MiB | 15.84 µs | 24.75 µs | 25.96 µs | 0.95× / 0.95× |
| 16 KiB | 1 KiB | 69.1 ns | 120.6 ns | 84.0 ns | 1.44× / 1.44× |
| 16 KiB | 8 KiB | 99.1 ns | 150.8 ns | 108.8 ns | 1.39× / 1.32× |
| 16 KiB | 1 MiB | 15.90 µs | 16.76 µs | 16.62 µs | 1.01× / 1.02× |

**Conclusion:** Reader costs more for the two small inputs in these runs. On 1 MiB, the wrappers
are much closer: Reader takes about 5% less time with 1 KiB calls, and 1–2% more with 16 KiB calls.
Neither wrapper wins every case. The 16 KiB calls let the wrappers bypass their buffers, but
also change the call count and stack-array initialization cost; that is not an isolated measure
of bypassing. Cursor remains a useful control for this memory-copy loop, not a prediction for
unbuffered disk or socket I/O.

## 2. Record processing

**Question A:** how do the readers compare when the caller does the same work? Both use the
same `read_until` loop and a reused record Vec. Each iteration processes **64 records**; sizes
include LF. Construction, searching, copies, EOF, and destruction are timed.

| Bytes per record | Reader, copied | BufReader, copied | Reader / BufReader (A / B) |
| ---: | ---: | ---: | ---: |
| 64 | 819.2 ns | 794.4 ns | 1.03× / 1.03× |
| 8192 | 39.75 µs | 40.04 µs | 0.99× / 1.01× |
| 8193 | 38.34 µs | 38.39 µs | 1.00× / 1.01× |

**Conclusion:** the matched copied recipes are close: Reader is about 3% slower for 64-byte
records, while the ordering for the two larger record sizes reverses between runs. The earlier
large gap between a Reader borrowed recipe and a BufReader copied recipe did not isolate the
choice of reader. This pair does not support that earlier interpretation.

**Question B:** when borrowing retained records, how often should this recipe compact? Both
variants use Reader and the same search/processing loop; one compacts after every record and
the other waits until the next delimiter is missing and it needs to fill. Processing receives
a temporary slice. The last column also compares this borrowed recipe with Reader's copied one.

| Bytes per record | Compact each | Compact on fill | On fill / each (A / B) | On fill / Reader copied (A / B) |
| ---: | ---: | ---: | ---: | ---: |
| 64 | 2.81 µs | 2.26 µs | 0.80× / 0.81× | 2.76× / 2.84× |
| 8192 | 206.03 µs | 206.43 µs | 1.00× / 1.02× | 5.19× / 5.09× |
| 8193 | 235.01 µs | 236.90 µs | 1.01× / 1.01× | 6.18× / 6.05× |

**Conclusion:** delaying compaction saves about 19% for the small records in these runs, where
many records arrive together. For records around 8 KiB there is no measured benefit from that
schedule. Both borrowed recipes still take substantially longer than the copied loop here.
Avoiding a record copy alone therefore does not make this recipe faster. Its searches and
retention management are part of the measured cost; identifying the dominant component would
require a separate profiling experiment. Borrowing remains an ownership choice, not a speed
claim established by this benchmark.

## 3. Retention operations

Every fixture starts with **64 KiB buffered**. One iteration performs one operation. Fixture
construction/filling and final destruction are excluded; the operation's own copying,
allocation, and shrinking are included. Prefix/suffix sizes matter to what each operation does.

**Question A:** what does discarding lookbehind cost with or without releasing spare capacity?

| Consumed prefix | Unread suffix | Compact (keep 64 KiB capacity) | Compact and shrink (suffix-sized capacity) |
| --- | --- | ---: | ---: |
| 8 KiB | 56 KiB | 1.13 µs | 1.12 µs |
| 56 KiB | 8 KiB | 230.4 ns | 238.9 ns |

These operations discard the prefix. Their resulting capacity differs, so choose based on
whether the spare capacity is still useful. The small timing differences here do not establish
that shrinking is free: allocator behavior and later reuse/drop costs are outside that claim.
Both operations move the unread suffix to the start; the larger suffix costs more in this run.

**Question B:** what if the prefix must survive independently? `take_consumed` returns a Buffer;
`copy_prefix` copies that prefix to a Vec, then compacts and shrinks the reader. Both return the
same prefix bytes and leave the same suffix bytes in a suffix-sized reader buffer.

| Consumed prefix | Unread suffix | Take consumed | Copy prefix, compact, shrink | Take / copy (A / B) |
| --- | --- | ---: | ---: | ---: |
| 8 KiB | 56 KiB | 2.32 µs | 1.30 µs | 1.78× / 1.82× |
| 56 KiB | 8 KiB | 316.5 ns | 2.03 µs | 0.16× / 0.16× |

**Conclusion:** the preferred timing reverses with the split. For the small prefix/large suffix,
copying the prefix costs less; for the large prefix/small suffix, transfer takes about 16% of
the copy recipe's time. This is consistent with the different bytes each recipe copies, but
the benchmark does not isolate copying from allocation/shrink costs. Compare these owned
alternatives to each other, not to `compact`, which does not preserve an owned prefix. Neither
variant is shown to be allocation-free, and a Buffer and Vec have different APIs.

## 4. Initial capacity and growth

**Question:** how long does it take to construct, fill through EOF, and drop a reader for the
same **24 KiB input**, with a 32 KiB maximum? Here both runs' times are shown because the gain
from pre-sizing changed noticeably between them. The final capacities are checked by the harness.

| Initial capacity | Final capacity | Run A | Run B | Time / 8 KiB start (A / B) |
| --- | --- | ---: | ---: | ---: |
| 8 KiB | 24 KiB | 666.9 ns | 539.5 ns | 1.00× / 1.00× |
| 24 KiB | 24 KiB | 431.3 ns | 497.2 ns | 0.65× / 0.92× |
| 32 KiB | 32 KiB | 424.3 ns | 466.6 ns | 0.64× / 0.86× |

**Conclusion:** pre-sizing lowered time for this known input in both runs, but the exact-sized
case's reduction ranged from about 8% to 35%. The extra-capacity case retained 8 KiB more than
the input. An exactly full initial buffer can still grow for the read that discovers EOF;
final capacity alone does not describe its allocation history. These lifecycle timings do not
settle a generally optimal capacity or isolate reallocation cost.

## Reproduce these observations

Measured code: `0ae007a939af9331617e9b1d150100599f098dbd`, based on main after PR #8.
The following documentation commit adds this report without changing the measured Rust code.

- Date: 2026-09-27. CPU: Intel Core i9-11900K, advertised 3.50 GHz.
- OS/target: Linux `7.2.7-zen1-1-zen`, `x86_64-unknown-linux-gnu`.
- Compiler: `rustc 1.98.1 (48a229cea 2026-09-01)`, LLVM 22.1.8; standard optimized bench profile.
- Criterion 0.8.2, dependency resolution in [baseline.lock](baseline.lock).
  SHA-256: `4196999a51c33b3f2fd4b59f66a0923d7d2f587659945a81c5a09b0d23ab7776`.
- CPU 0 governor reported `powersave`. No affinity or frequency controls were applied.
  Background load was not controlled; other crate checks were kept outside the timed runs.

```bash
cargo bench --bench reader -- --warm-up-time 1 --measurement-time 2 \
  --sample-size 30 --save-baseline review-a
cargo bench --bench reader -- --warm-up-time 1 --measurement-time 2 \
  --sample-size 30 --save-baseline review-b
```

All 41 workloads passed their output checks in both runs. CSV estimates use Criterion's slope
when available and mean otherwise, with its per-run 95% intervals. Those intervals do not capture
all cross-run variation; the growth results illustrate why repeated runs matter. Read the
[methodology](README.md) for precise timing and ownership boundaries. These are warm memory
workloads, not disk/network, short-read, concurrent, allocation-count, or RSS measurements.
