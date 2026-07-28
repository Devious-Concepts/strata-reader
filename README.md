# strata-reader

A `BufReader`-style wrapper with a dynamically growing buffer and explicit memory control.

`std::io::BufReader` allocates a fixed buffer (8 KiB by default, configurable via `with_capacity`).
Once set, that size never changes. The `Reader` from this crate starts small and grows its buffer
automatically as data arrives, up to a configurable maximum. It also gives you explicit control
over memory: you decide when to compact, shrink, or discard buffered data.

This is intended for tokenizers, protocol parsers, and other use cases where input sizes are
unpredictable, lookbehind is useful, and you want to manage buffer lifetime yourself.

## Features

- **Automatic buffer growth**: the buffer starts at `CHUNK_SIZE` (8 KiB), grows automatically as
  reads require more room, and never grows past the configured maximum capacity.
- **Manual memory management**: `compact()` reclaims consumed space, `shrink()` releases unused
  capacity, while `clear()` and `discard()` reset retained data.
- **Builder pattern**: configure initial and maximum capacity via `Reader::builder(reader)`.
- **`DynamicRead` trait**: extends `BufRead` with buffer inspection (`buffer()`, `pos()`,
  `capacity()`) and the management methods above.
- **Delimiter-based fills**: `fill_until(byte)`, `fill_until_char(ch)`, `fill_until_str(needle)`,
  and `fill_while(predicate)` read from the underlying reader until a condition is met, EOF is
  reached, or the configured maximum capacity is reached.
- **Peek methods**: `peek(n)` returns up to _n_ unconsumed bytes without advancing the read
  position; `peek_behind(n)` returns up to _n_ already-consumed bytes still in the buffer.
- **Seek support**: `Seek::stream_position()` reports the logical stream position, `Seek::seek()`
  clears the buffer after successful seeks, and `Reader::seek_relative()` can move within
  retained buffered data without touching the inner reader.

## Usage

```rust
use strata_reader::{Reader, DynamicRead};
use std::io::{BufRead, Cursor};

// Simulate a stream of data (e.g. from a file or network socket)
let data = b"key=value\nother=data\n";
let mut reader = Reader::new(Cursor::new(data.as_slice()));

/* Read until '=' is present in the buffer. This is not an exact-length read:
the buffer may receive more than "key=", depending on how much the underlying
reader provides in one read. */
reader.fill_until(b'=').unwrap();

/* In this case the entire input landed in the buffer at once, even though we
only asked to stop once '=' was found. */
assert_eq!(reader.buffer(), b"key=value\nother=data\n");

// Peek at just the key without consuming anything
assert_eq!(reader.peek(3), b"key");

// Consume the key and the '=' delimiter (4 bytes: "key=")
reader.consume(4);

// The read position has advanced, but consumed data is still retained
assert_eq!(reader.peek_behind(4), b"key=");

// The remaining unconsumed data is still available going forward
assert_eq!(reader.peek(5), b"value");

// Move the remaining data to the front to make room for future reads
reader.compact();
```

## Configuring capacity

```rust
use strata_reader::Reader;
use std::io::Cursor;

let reader = Reader::builder(Cursor::new(vec![0u8; 1024]))
    .initial_capacity(16 * 1024)   // start at 16 KiB
    .max_capacity(1024 * 1024)     // grow up to 1 MiB
    .build();
```

Requested capacities are rounded up to the crate's internal chunk alignment. If the maximum
capacity is smaller than the initial capacity, it is raised to match the initial capacity.

## Minimum Supported Rust Version

The MSRV is **1.87.0**.

## License

Licensed under the Mozilla Public License, Version 2.0 (see [LICENSE](LICENSE)
or <https://www.mozilla.org/en-US/MPL/2.0/>).

MPL-2.0 is a file-level weak copyleft license: its obligations attach to this crate's files (in
source and compiled form) and to modifications of them, not to your own source files that merely
use this crate. Distributing a binary that includes this crate obliges you to tell recipients
that the crate is included and how to obtain its source; for an unmodified crate, pointing them
at the exact crates.io release is a reasonable way to do that. Example material (documentation
code examples, `examples/`, integration tests, and benchmarks) is additionally licensed under
[MIT-0](LICENSE-MIT-0), so it can be copied freely. See
[LICENSE-CLARIFICATION.md](LICENSE-CLARIFICATION.md) for the maintainers' full statement of
intent regarding normal Rust usage (linking, monomorphization, macro expansion), binary
distribution, and generated code, and [CONTRIBUTING.md](CONTRIBUTING.md) for the contribution
policy.
