# Benchmarks

[Results and conclusions](BASELINE.md) group measurements by the decision being compared.
The source modules below contain the inputs, correctness checks, and timing loops.

```bash
cargo bench --bench reader -- --test       # Check every workload, without timing
cargo bench --bench reader                 # Measure every workload
cargo bench --bench reader -- records      # Measure one group
```

## Sequential reads: what does the wrapper add?

[Source](reader/sequential.rs). Read the same 1 KiB, 8 KiB, or 1 MiB input through `Reader`,
`BufReader`, or a bare `Cursor`, using an identical loop. Both wrappers start with 8 KiB capacity.
The output array is either 1 KiB (smaller than the buffer) or 16 KiB (large enough for their
buffer-bypass paths). Source bytes cycle through all 256 byte values.

One iteration reads the entire input. Timing includes wrapper construction, output-array
initialization, reads/copies, EOF detection, and destruction. Creating source data is excluded.
The `Cursor` control shows the cost of this copy loop without buffering; it is not a substitute
for a buffered reader over an expensive source. These measurements contain no system calls.

Compare Reader/BufReader at the **same input and read size**. Comparing the two read sizes also
changes the number of calls and the output array's initialization cost, not just buffering.

## Records: separate the reader from the processing recipe

[Source](reader/records.rs). Each input contains 64 LF-terminated records, sized at 64, 8192,
or 8193 bytes including LF. Those cases put many records in a buffer, fit exactly one, or make
a record cross the initial buffer capacity. One iteration processes all 64 records.

| Variant | Processing and lifetime |
| --- | --- |
| `reader_copied` | `Reader` through the shared `BufRead::read_until` loop into a reused `Vec` |
| `bufreader_copied` | `BufReader` through that same loop and reused `Vec` |
| `borrowed_compact_each` | `Reader` exposes each consumed record as a slice, then compacts |
| `borrowed_compact_on_fill` | Same borrowed recipe, but compacts only when more input is needed |

The copied pair is the direct reader comparison. The borrowed pair compares compaction schedules.
Comparing copied with borrowed is a choice of recipe: borrowed slices cannot survive the next
mutation, while the copied record lives in a separate reusable allocation. The borrowed recipes
also perform their own delimiter-position search. Timings include those differences.

All variants include construction, reading, searching, record processing via `black_box`,
copying/compaction, EOF, and destruction. Source generation is excluded. Before measurement, each
recipe is checked against an input of the same shape whose records each start with a distinct
byte, so every record's contents, boundaries, and order are verified.

## Retention: discard the prefix or keep owned bytes?

[Source](reader/retention.rs). Start with 64 KiB buffered, with either 8 KiB consumed and 56 KiB
unread, or 56 KiB consumed and 8 KiB unread. One iteration performs one operation.

| Operation | Consumed prefix afterward | Unread suffix afterward |
| --- | --- | --- |
| `compact` | Discarded | At position zero; capacity stays 64 KiB |
| `compact_and_shrink` | Discarded | At position zero; capacity shrinks to suffix size |
| `take_consumed` | Owned `Buffer` | In a replacement reader buffer sized to the suffix |
| `copy_prefix` | Owned `Vec` copied from lookbehind | Compacted and shrunk in the reader |

Compare the two owned alternatives when the requirement is to keep the prefix's bytes. Their
return types differ, but both preserve the same independent prefix and the same unread suffix.
Compare the discard alternatives when no prefix ownership is needed. A discard timing is not a
baseline for judging an ownership operation that does more work.

Fixture construction/filling and final destruction of the fixture and returned value are outside
timing (`iter_batched_ref`, batches of 16). Allocation, copying, or shrinking _inside_ the operation
are timed. This excludes the later cost of dropping a kept prefix. Capacity checks describe the
reader's observable capacity; allocation counts and process memory are not measured.

## Growth: how much capacity should this input start with?

[Source](reader/growth.rs). Fill the same 24 KiB input through EOF, starting at 8, 24, or 32 KiB,
with a 32 KiB maximum. One iteration constructs, fills, and drops the reader. Timing includes
initial allocation/initialization, reading, growth, and any completion-time shrink.

The exact-sized case is deliberate: a full buffer may grow to make the read that discovers EOF.
The final capacities are checked as 24, 24, and 32 KiB respectively. Compare complete lifecycle
times; this experiment cannot isolate the allocator's reallocation cost from everything else.

## Reproduce and interpret

The harness uses [Criterion 0.8.2](https://docs.rs/criterion/0.8.2/criterion/) as a development-only
dependency, with plotting and parallel analysis disabled. Exact output checks run before timing;
timed loops pass processed bytes to `std::hint::black_box`. All 41 cases also run in smoke mode
under `cargo test --all-targets --all-features`.

[The result record](BASELINE.md) includes the measured code revision, compiler, CPU/OS, command,
and two runs. [results.csv](results.csv) preserves every point estimate and 95% confidence
interval. [baseline.lock](baseline.lock) preserves the dependency resolution; the library's root
lockfile remains untracked. In a separate checkout, copy it to `Cargo.lock` and use `--locked`.

To compare a library change on the same machine:

```bash
cargo bench --bench reader -- --save-baseline before
# Change the library, keeping the workloads and configuration the same.
cargo bench --bench reader -- --baseline before
```

These are warm, in-memory inputs, not disk/network, short-read, or concurrent workloads.
Construction and final drops are timed in all groups except retention. Small cases include
harness overhead. Record source revision, dependency resolution, compiler, hardware, power
settings, and competing load with results. Two runs check repeatability on this host; they do
not establish other machines' performance or a CI threshold.
