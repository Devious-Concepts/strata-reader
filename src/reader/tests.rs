//! Tests for the Reader
//!
//! These tests are in the same narrative order as the main file, and are designed to not depend on
//! things that we've yet to have written tests for, at least narratively. While the test runner is
//! asynchronous and this does not matter there, when reading the file, it makes the flow much
//! easier to understand since we don't introduce future concepts if we can avoid them.

#![expect(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    reason = "Okay in tests"
)]

use super::*;
use crate::constants::CHUNK_SIZE;
use std::io::{Cursor, IoSliceMut, Read};

// -----------------------------------------------------------------------------
// Reader - Creation
// -----------------------------------------------------------------------------

#[test]
fn test_reader_new() {
    let cur: Cursor<&str> = Cursor::default();
    let reader = Reader::new(cur);

    // Check internal state matches expectations
    assert_eq!(reader.buffer, Buffer::default());
    assert_eq!(reader.max_capacity, DEFAULT_MAX_SIZE);
    assert_eq!(reader.reader, Cursor::default());
}

#[test]
fn test_reader_with_max_capacity() {
    let cur: Cursor<&str> = Cursor::default();
    let max_capacity = CHUNK_SIZE + 123;
    let reader = Reader::with_max_capacity(max_capacity, cur);

    // Check internal state matches expectations
    assert_eq!(reader.buffer, Buffer::default());
    assert_eq!(reader.max_capacity, 2 * CHUNK_SIZE); // Rounds up
    assert_eq!(reader.reader, Cursor::default());
}

// -----------------------------------------------------------------------------
// impl Read
// -----------------------------------------------------------------------------

#[test]
fn test_reader_read() {
    // Create a reader against no data, for an EOF test
    let cur: Cursor<&'static str> = Cursor::default();
    let mut reader = Reader::new(cur);
    let mut buf = [0u8; 100];
    let len = reader.read(&mut buf).unwrap();

    // We should get nothing
    assert_eq!(len, 0);

    // Create a reader against some data
    let data = "Hello, World!";
    let cur = Cursor::new(data);
    let mut reader = Reader::new(cur);
    let mut buf = [0u8; 100];
    let len = reader.read(&mut buf).unwrap();

    // The data should all have been read
    assert_eq!(len, data.len());
    assert_eq!(&buf[..len], data.as_bytes());

    // Create a reader against some data again, to read against, multiple times
    let data = "Hello, World!";
    let mut cur = Cursor::new(data);
    cur.set_position(6); // Simulate having already read the first 6 bytes
    let mut reader = Reader::new(cur);
    let mut buf = [0u8; 3];

    // Inject exactly 6 bytes into the internal buffer such
    // that the first two reads will consume from there
    reader.buffer.inject_test_data(&data.as_bytes()[..6]); // The 6 bytes we skipped above

    // Read once
    let len = reader.read(&mut buf).unwrap();
    assert_eq!(len, 3);
    assert_eq!(&buf[..len], &data.as_bytes()[..3]);
    assert_eq!(reader.buffer.pos(), 3); // Read 3 from internal buffer

    // Read twice
    let len = reader.read(&mut buf).unwrap();
    assert_eq!(len, 3);
    assert_eq!(&buf[..len], &data.as_bytes()[3..6]);
    assert_eq!(reader.buffer.pos(), 6); // Read another 3 from internal buffer

    // Read thrice
    let len = reader.read(&mut buf).unwrap();
    assert_eq!(len, 3);
    assert_eq!(&buf[..len], &data.as_bytes()[6..9]);
    assert_eq!(reader.buffer.pos(), 0); // Internal buffer was cleared and skipped

    // Read a fourth time
    let len = reader.read(&mut buf).unwrap();
    assert_eq!(len, 3);
    assert_eq!(&buf[..len], &data.as_bytes()[9..12]);

    // And finally read the last bit
    let len = reader.read(&mut buf).unwrap();
    assert_eq!(len, 1);
    assert_eq!(&buf[..len], &data.as_bytes()[12..]);
}

#[test]
fn test_reader_read_vectored() {
    // Create a reader against some data with small target buffers
    let data = "Hello, World!";
    let cur = Cursor::new(data);
    let mut reader = Reader::new(cur);
    let mut buf1 = [0u8; 5];
    let mut buf2 = [0u8; 10];
    let mut buffers = [IoSliceMut::new(&mut buf1), IoSliceMut::new(&mut buf2)];
    let len = reader.read_vectored(&mut buffers).unwrap();

    // Check that the vectored read did what we'd expect
    assert_eq!(len, data.len());
    assert_eq!(&buf1, &data.as_bytes()[..5]);
    assert_eq!(&buf2[..8], &data.as_bytes()[5..]);
    assert_eq!(reader.buffer.pos(), data.len()); // Confirms fill_buf path

    // Again, create a reader against some data, this time with a large buffer
    // so we hit the code-path delegating to the given buffers reader instead
    let data = "Hello, World!";
    let cur = Cursor::new(data);
    let mut reader = Reader::new(cur);
    let mut buf = [0u8; CHUNK_SIZE]; // Must be ≥ CHUNK_SIZE to trigger delegation
    let mut buffers = [IoSliceMut::new(&mut buf)];
    let len = reader.read_vectored(&mut buffers).unwrap();

    // Check that the vectored read did what we'd expect
    assert_eq!(len, data.len());
    assert_eq!(&buf[..13], data.as_bytes());
    assert_eq!(reader.buffer.pos(), 0); // Confirms delegation path
}

#[test]
fn test_reader_read_to_end() {
    // Create a reader against some data
    let data = "Hello, World!";
    let cur = Cursor::new(data);
    let mut reader = Reader::new(cur);
    let mut result = Vec::new();

    // To mirror the test below we need to check that len is 0 before we read
    assert_eq!(reader.buffer.len(), 0); // Confirms internal buffer isn't used
    let len = reader.read_to_end(&mut result).unwrap();

    // We should have read everything without touching the internal buffer
    assert_eq!(len, data.len());
    assert_eq!(result, data.as_bytes());
    assert_eq!(reader.buffer.len(), 0); // Should still be 0

    // Create another reader with some data pre-injected
    let data = "Hello, World!";
    let mut cur = Cursor::new(data);
    cur.set_position(7); // Simulate having already read the first 7 bytes
    let mut reader = Reader::new(cur);
    let mut result = Vec::new();

    // Pre-inject some data into the internal buffer
    reader.buffer.inject_test_data(&data.as_bytes()[..7]); // The 7 bytes we skipped above
    assert_eq!(reader.buffer.len(), 7); // Confirms internal buffer usage

    // Read the rest of the data
    let len = reader.read_to_end(&mut result).unwrap();

    // We should get buffered data and cursor data
    assert_eq!(len, data.len());
    assert_eq!(result, data.as_bytes());
    assert_eq!(reader.buffer.len(), 0); // Confirms the buffer was cleared
}

#[test]
fn test_reader_read_to_string() {
    // Create a reader against some UTF-8 data
    let data = "Hello, World!";
    let cur = Cursor::new(data);
    let mut reader = Reader::new(cur);
    let mut result = String::new();
    let len = reader.read_to_string(&mut result).unwrap();

    // Reading should work fine
    assert_eq!(len, data.len());
    assert_eq!(result, data);

    // Create a reader against some non-UTF-8 data
    let data = vec![0xFF, 0xFE, 0xFD]; // Invalid UTF-8
    let cur = Cursor::new(data);
    let mut reader = Reader::new(cur);
    let mut result = String::new();
    let err = reader.read_to_string(&mut result).unwrap_err();

    // We should get an invalid data error
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn test_reader_read_exact() {
    // Create a reader against some data and try reading more than that
    let data = "Hi";
    let cur = Cursor::new(data);
    let mut reader = Reader::new(cur);
    let mut buf = [0u8; 10];
    let err = reader.read_exact(&mut buf).unwrap_err();

    // Trying to read more than we have should fail with an EOF error
    assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);

    // Create a reader against some data again
    let data = "Hello, World!";
    let cur = Cursor::new(data);
    let mut reader = Reader::new(cur);

    // To mirror the test below we need to check that len is 0 before we read
    assert_eq!(reader.buffer.len(), 0); // Confirms internal buffer isn't used
    let mut buf = [0u8; 5];
    reader.read_exact(&mut buf).unwrap();

    // We should get exactly the size we requested
    assert_eq!(&buf, &data.as_bytes()[..5]);

    // Once more, create a reader against some data with some pre-injected
    let data = "Hello, World!";
    let mut cur = Cursor::new(data);
    cur.set_position(7); // Simulate having already read the first 7 bytes
    let mut reader = Reader::new(cur);

    // Pre-inject some data into the internal buffer
    reader.buffer.inject_test_data(&data.as_bytes()[..7]); // The 7 bytes we skipped above
    assert_eq!(reader.buffer.len(), 7); // Confirms internal buffer usage

    // Request 13 bytes - more than buffered (7), forcing slow path
    let mut buf = [0u8; 13];
    reader.read_exact(&mut buf).unwrap();

    // Should successfully read all 13 bytes across buffer + inner reader
    assert_eq!(&buf, b"Hello, World!");
    assert_eq!(reader.buffer.len(), 0); // Confirms the buffer was cleared
}

// -----------------------------------------------------------------------------
// impl BufRead
// -----------------------------------------------------------------------------

// -----------------------------------------------------------------------------
// TODO
// -----------------------------------------------------------------------------

// -----------------------------------------------------------------------------
// TODO
// -----------------------------------------------------------------------------
