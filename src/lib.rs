//! A [`BufReader`](std::io::BufReader) alternative with a dynamically growing buffer and manual
//! memory control.
//!
//! [`BufReader`](std::io::BufReader) allocates a fixed buffer (8 KiB by default). Once set, that
//! size never changes. [`Reader`] starts at the same 8 KiB and grows automatically as data
//! arrives, up to a configurable maximum. It also gives you explicit control over memory: you
//! decide when to compact the buffer, shrink it's capacity, or discard it.
//!
//! # When to use this
//!
//! This crate is a good fit for tokenizers, protocol parsers, and other use cases where input sizes
//! are unpredictable and you want to manage buffer lifetime yourself. If you know your input fits
//! in a fixed buffer, prefer [`std::io::BufReader`].
//!
//! # Quick start
//!
//! ```
//! use strata_reader::{Reader, DynamicRead};
//! use std::io::{BufRead, Cursor};
//!
//! // Simulate a stream of data (e.g. from a file or network socket)
//! let data = b"key=value\nother=data\n";
//! let mut reader = Reader::new(Cursor::new(data.as_slice()));
//!
//! // Read from the underlying reader until '=' is found in the buffer.
//! // This is not an exact operation, the buffer may receive more data
//! // than just "key=", depending on how much the underlying reader
//! // provides in a single read.
//! reader.fill_until(b'=').unwrap();
//!
//! // In this case the entire input landed in the buffer at once, even
//! // though we only asked to stop at '='.
//! assert_eq!(reader.buffer(), b"key=value\nother=data\n");
//!
//! // Peek at just the key without consuming anything
//! assert_eq!(reader.peek(3), b"key");
//!
//! // Consume the key and the '=' delimiter (4 bytes: "key=")
//! reader.consume(4);
//!
//! // The read position has advanced, but consumed data is still in
//! // the buffer, peek_behind lets you look at it.
//! assert_eq!(reader.peek_behind(4), b"key=");
//!
//! // The remaining unconsumed data is still available going forward
//! assert_eq!(reader.peek(5), b"value");
//!
//! // Move the remaining data to the front to make room for future reads
//! reader.compact();
//! ```
//!
//! # Capacity configuration
//!
//! ```
//! use strata_reader::Reader;
//! use std::io::Cursor;
//!
//! let reader = Reader::builder(Cursor::new(vec![0u8; 1024]))
//!     .initial_capacity(16 * 1024)   // start at 16 KiB
//!     .max_capacity(1024 * 1024)     // grow up to 1 MiB
//!     .build();
//! ```
//!
//! # Crate organization
//!
//! - [`Reader`]: the primary type, wrapping any [`Read`](std::io::Read) with a managed
//!   buffer.
//! - [`ReaderBuilder`]: configures initial and maximum capacity before constructing a
//!   [`Reader`].
//! - [`DynamicRead`]: trait extending [`BufRead`](std::io::BufRead) with buffer inspection and
//!   memory management methods.
//! - [`buffer::Buffer`]: the standalone buffer for users who want direct control without wrapping a
//!   reader.
//! - [`constants`]: buffer size constants ([`CHUNK_SIZE`](constants::CHUNK_SIZE),
//!   [`DEFAULT_MAX_CAPACITY`](constants::DEFAULT_MAX_CAPACITY)) used throughout the crate.

pub mod buffer;
pub mod constants;
mod read;
mod reader;

pub use read::{DynamicRead, DynamicReadExt};
pub use reader::{Reader, ReaderBuilder};
