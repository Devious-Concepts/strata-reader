# Reader

A `BufReader` with a dynamically growing buffer and manual memory control.

`std::io::BufReader` allocates a fixed buffer (8 KiB by default, configurable via `with_capacity`).
Once set, that size never changes. `Reader` starts small and grows its buffer automatically as
data arrives, up to a configurable maximum. It also gives you explicit control over memory: you
decide when to compact, shrink, or discard buffered data.

This makes it a better fit for tokenizers, protocol parsers, and other use cases where input sizes
are unpredictable and you want to manage buffer lifetime yourself.

## Features

- **Automatic buffer growth**: the buffer grows in `CHUNK_SIZE` (8 KiB) units using exponential
  growth by default, then switches to linear growth when nearing the configured maximum. See the
  growth logic in `src/buffer.rs` around `Buffer::grow`, `Buffer::grow_targeted_linear`, and
  `read_once`.
- **Manual memory management**: `compact()` reclaims consumed space, `shrink()` releases unused
  capacity, `clear()` and `discard()` reset the buffer. Nothing happens behind your back.
- **Builder pattern**: configure initial and maximum capacity via `Reader::builder(reader)`.
- **`DynamicRead` trait**: extends `BufRead` with buffer inspection (`buffer()`, `pos()`,
  `capacity()`) and the management methods above.
- **Delimiter-based fills**: `fill_until(byte)`, `fill_until_char(ch)`, `fill_until_str(needle)`,
  and `fill_while(predicate)` read from the underlying reader until a condition is met or the buffer
  is full.
- **Peek methods**: `peek(n)` returns up to _n_ unconsumed bytes without advancing the read
  position; `peek_behind(n)` returns up to _n_ already-consumed bytes still in the buffer.

## Usage

```rust
use strata_reader::{Reader, DynamicRead};
use std::io::{BufRead, Cursor};

// Simulate a stream of data (e.g. from a file or network socket)
let data = b"key=value\nother=data\n";
let mut reader = Reader::new(Cursor::new(data.as_slice()));

/* Read from the underlying reader until '=' is found in the buffer. This is not
an exact operation, the buffer may receive more data than just "key=", depending
on how much the underlying reader provides in a single read. */
reader.fill_until(b'=').unwrap();

/* In this case the entire input landed in the buffer at once, even though we
only asked to stop at '='. */
assert_eq!(reader.buffer(), b"key=value\nother=data\n");

// Peek at just the key without consuming anything
assert_eq!(reader.peek(3), b"key");

// Consume the key and the '=' delimiter (4 bytes: "key=")
reader.consume(4);

/* The read position has advanced, but consumed data is still in the buffer,
peek_behind lets you look at it. */
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

## Minimum Supported Rust Version

The MSRV is **1.87.0**. This is checked in CI across Linux, Windows, and macOS.
