# Reader

A `BufReader` with a dynamically growing buffer and manual memory control.

`std::io::BufReader` allocates a fixed buffer (8 KiB by default, configurable
via `with_capacity`). Once set, that size never changes. `Reader` starts
small and grows its buffer automatically as data arrives, up to a configurable
maximum. It also gives you explicit control over memory: you decide when to
compact, shrink, or discard buffered data.

This makes it a better fit for tokenizers, protocol parsers, and other use cases
where input sizes are unpredictable and you want to manage buffer lifetime
yourself.

## Features

- **Automatic buffer growth** — the buffer grows in power-of-two multiples of
  8 KiB as needed, up to a configurable maximum.
- **Manual memory management** — `compact()` reclaims consumed space,
  `shrink()` releases unused capacity, `clear()` and `discard()` reset the
  buffer. Nothing happens behind your back.
- **Builder pattern** — configure initial and maximum capacity via
  `Reader::builder(reader)`.
- **`DynamicRead` trait** — extends `BufRead` with buffer inspection
  (`buffer()`, `pos()`, `capacity()`) and the management methods above.
- **Delimiter-based fills** — `fill_until(byte)`, `fill_until_char(ch)`,
  `fill_until_str(needle)`, and `fill_while(predicate)` read from the
  underlying reader until a condition is met or the buffer is full.
- **Peek methods** — `peek(n)` returns up to *n* unconsumed bytes without
  advancing the read position; `peek_behind(n)` returns up to *n*
  already-consumed bytes still in the buffer.

## Usage

```rust
use strata_reader::{Reader, DynamicRead};
use std::io::{BufRead, Cursor};

let data = b"Hello, World!";
let mut reader = Reader::new(Cursor::new(data.as_slice()));

// Fill the buffer until we hit a delimiter
reader.fill_until(b',').unwrap();

// Peek at buffered data without consuming it
assert_eq!(reader.peek(5), b"Hello");

// Consume the bytes we've processed
reader.consume(7); // consume "Hello, "

// Look back at already-consumed data still in the buffer
assert_eq!(reader.peek_behind(7), b"Hello, ");

// The remaining unconsumed data
assert_eq!(reader.peek(6), b"World!");

// Reclaim consumed space when you're ready
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
