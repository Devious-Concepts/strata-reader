//! A [`BufReader`](std::io::BufReader) with a dynamically growing buffer and
//! manual memory control.
//!
//! [`std::io::BufReader`] allocates a fixed buffer (8 KiB by default). Once
//! set, that size never changes. [`Reader`] starts at the same 8 KiB and
//! grows automatically as data arrives, up to a configurable maximum. It also
//! gives you explicit control over memory: you decide when to compact, shrink,
//! or discard buffered data.
//!
//! # When to use this
//!
//! This crate is a good fit for tokenizers, protocol parsers, and other use
//! cases where input sizes are unpredictable and you want to manage buffer
//! lifetime yourself. If you know your input fits in a fixed buffer, prefer
//! [`std::io::BufReader`].
//!
//! # Quick start
//!
//! ```
//! use strata_reader::{Reader, DynamicRead};
//! use std::io::{BufRead, Cursor};
//!
//! let data = b"Hello, World!";
//! let mut reader = Reader::new(Cursor::new(data.as_slice()));
//!
//! // Fill the buffer until we hit a delimiter
//! reader.fill_until(b',').unwrap();
//!
//! // Peek at buffered data without consuming it
//! assert_eq!(reader.peek(5), b"Hello");
//!
//! // Consume the bytes we've processed
//! reader.consume(7); // consume "Hello, "
//!
//! // Look back at already-consumed data still in the buffer
//! assert_eq!(reader.peek_behind(7), b"Hello, ");
//!
//! // Reclaim consumed space when you're ready
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
//! # Crate organisation
//!
//! - [`Reader`] — the primary type, wrapping any [`Read`](std::io::Read)
//!   with a managed buffer.
//! - [`ReaderBuilder`] — configures initial and maximum capacity before
//!   constructing a [`Reader`].
//! - [`DynamicRead`] — trait extending [`BufRead`](std::io::BufRead) with buffer
//!   inspection and memory management methods.
//! - [`buffer::Buffer`] — the standalone buffer for users who want direct
//!   control without wrapping a reader.
//! - [`constants`] — buffer size constants ([`CHUNK_SIZE`](constants::CHUNK_SIZE),
//!   [`DEFAULT_MAX_SIZE`](constants::DEFAULT_MAX_SIZE)) used throughout the
//!   crate.

pub mod buffer;
pub mod constants;
mod read;
mod reader;

pub use read::DynamicRead;
pub use reader::{Reader, ReaderBuilder};
