//! Tests for the Reader
//!
//! These tests are in the same narrative order as the main file, and are designed to not depend on
//! things that we've yet to have written tests for, at least narratively. While the test runner is
//! asynchronous and this does not matter there, when reading the file, it makes the flow much
//! easier to understand since we don't introduce future concepts if we can avoid them.

#![expect(
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
fn test_reader_builder() {
    // Create with max_capacity only (initial defaults)
    let cur: Cursor<&str> = Cursor::default();
    let max_capacity = CHUNK_SIZE + 123;
    let reader = Reader::builder(cur)
        .max_capacity(max_capacity)
        .build();

    // Check internal state matches expectations
    assert_eq!(reader.buffer, Buffer::default());
    assert_eq!(reader.max_capacity, 2 * CHUNK_SIZE); // Rounds up
    assert_eq!(reader.reader, Cursor::default());

    // Create with initial_capacity only (max defaults)
    let cur: Cursor<&str> = Cursor::default();
    let initial_capacity = 3 * CHUNK_SIZE;
    let reader = Reader::builder(cur)
        .initial_capacity(initial_capacity)
        .build();

    // Check internal state matches expectations
    assert_eq!(reader.buffer, Buffer::with_capacity(initial_capacity));
    assert_eq!(reader.buffer.cap(), 3 * CHUNK_SIZE);
    assert_eq!(reader.max_capacity, DEFAULT_MAX_SIZE);
    assert_eq!(reader.reader, Cursor::default());

    // Create with both initial_capacity and max_capacity
    let cur: Cursor<&str> = Cursor::default();
    let initial_capacity = 4 * CHUNK_SIZE;
    let max_capacity = 8 * CHUNK_SIZE;
    let reader = Reader::builder(cur)
        .initial_capacity(initial_capacity)
        .max_capacity(max_capacity)
        .build();

    // Check internal state matches expectations
    assert_eq!(reader.buffer, Buffer::with_capacity(initial_capacity));
    assert_eq!(reader.buffer.cap(), 4 * CHUNK_SIZE);
    assert_eq!(reader.max_capacity, 8 * CHUNK_SIZE);
    assert_eq!(reader.reader, Cursor::default());

    // Create with max_capacity less than initial_capacity (max is raised to match)
    let cur: Cursor<&str> = Cursor::default();
    let initial_capacity = 8 * CHUNK_SIZE;
    let max_capacity = 2 * CHUNK_SIZE;
    let reader = Reader::builder(cur)
        .initial_capacity(initial_capacity)
        .max_capacity(max_capacity)
        .build();

    // Check internal state matches expectations
    assert_eq!(reader.buffer.cap(), 8 * CHUNK_SIZE);
    assert_eq!(reader.max_capacity, 8 * CHUNK_SIZE); // Raised to match initial
}

// -----------------------------------------------------------------------------
// Reader - Peek Methods
// -----------------------------------------------------------------------------

#[test]
fn test_reader_peek() {
    // Empty reader returns empty slice
    let cur: Cursor<&str> = Cursor::default();
    let reader = Reader::new(cur);
    assert_eq!(reader.peek(5), &[]);

    // With data in the buffer
    let data = "Hello, World!";
    let cur = Cursor::new(data);
    let mut reader = Reader::new(cur);
    reader.fill_amount(data.len()).unwrap();

    // Peek at fewer bytes than available
    assert_eq!(reader.peek(5), b"Hello");

    // Peek at exactly available bytes
    assert_eq!(reader.peek(data.len()), data.as_bytes());

    // Peek at more than available clamps to what's there
    assert_eq!(reader.peek(100), data.as_bytes());

    // Peek at zero bytes returns empty slice
    assert_eq!(reader.peek(0), &[]);

    // Peek doesn't consume — repeated calls return the same data
    assert_eq!(reader.peek(5), b"Hello");

    // After consuming, peek reflects the new position
    reader.consume(7);
    assert_eq!(reader.peek(5), b"World");
}

#[test]
fn test_reader_peek_behind() {
    // Nothing consumed returns empty slice
    let cur: Cursor<&str> = Cursor::default();
    let reader = Reader::new(cur);
    assert_eq!(reader.peek_behind(5), &[]);

    // With data in the buffer, nothing consumed yet
    let data = "Hello, World!";
    let cur = Cursor::new(data);
    let mut reader = Reader::new(cur);
    reader.fill_amount(data.len()).unwrap();
    assert_eq!(reader.peek_behind(5), &[]);

    // After consuming some bytes
    reader.consume(7);
    assert_eq!(reader.peek_behind(5), b"llo, ");
    assert_eq!(reader.peek_behind(7), b"Hello, ");

    // More than consumed clamps to what's retained
    assert_eq!(reader.peek_behind(100), b"Hello, ");

    // Zero returns empty
    assert_eq!(reader.peek_behind(0), &[]);

    // Peek behind doesn't change state — repeated calls return the same data
    assert_eq!(reader.peek_behind(7), b"Hello, ");
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

#[test]
fn test_reader_bufread_fill_buf() {
    // Create a reader against some data
    let data = "Hello, World!";
    let cur = Cursor::new(data);
    let mut reader = Reader::new(cur);

    // We should get all of the data
    assert_eq!(reader.buffer.len(), 0); // Buffer is empty at first
    let read = reader.fill_buf().unwrap();
    assert_eq!(read, data.as_bytes());
    assert_eq!(reader.buffer.len(), data.len()); // Buffer has all the data after

    // Reading again should give the already read data
    let read = reader.fill_buf().unwrap();
    assert_eq!(read, data.as_bytes());

    // Consume a bit and read again, we should get the unconsumed bit
    reader.consume(7);
    let read = reader.fill_buf().unwrap();
    assert_eq!(read, &data.as_bytes()[7..]);
    assert_eq!(reader.buffer.len(), data.len()); // The buffer hasn't changed yet

    // Reading after all data is consumed should give nothing
    reader.consume(data.len() - 7);
    let read = reader.fill_buf().unwrap();
    assert_eq!(read, &[]);
}

/* Note: There is no test for `consume` as it's just a wrapper over `Buffer::consume()`
 * So testing is deferred to the buffers tests
 * There's no need to test this method twice after all
 */

// -----------------------------------------------------------------------------
// Reader - Fill Methods
// -----------------------------------------------------------------------------

#[test]
fn test_reader_fill_amount() {
    // Basic: read a specific amount from a reader with enough data
    let data = vec![0u8; 3 * CHUNK_SIZE];
    let cur = Cursor::new(data);
    let mut reader = Reader::new(cur);
    let bytes_read = reader.fill_amount(3 * CHUNK_SIZE).unwrap();

    assert!(bytes_read >= 3 * CHUNK_SIZE);
    assert!(reader.buffer.len() >= 3 * CHUNK_SIZE);

    // EOF: request more bytes than available
    let data = b"short";
    let cur = Cursor::new(data.as_slice());
    let mut reader = Reader::new(cur);
    let bytes_read = reader.fill_amount(1000).unwrap();

    assert_eq!(bytes_read, 5);
    assert_eq!(reader.buffer.len(), 5);

    // Exceeds max_capacity: should return an error
    let data = vec![0u8; 4 * CHUNK_SIZE];
    let cur = Cursor::new(data);
    let mut reader = Reader::builder(cur)
        .max_capacity(2 * CHUNK_SIZE)
        .build();
    let err = reader.fill_amount(3 * CHUNK_SIZE).unwrap_err();

    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn test_reader_fill_exact() {
    // Basic: read exactly N bytes
    let data = vec![42u8; 100];
    let cur = Cursor::new(data);
    let mut reader = Reader::new(cur);
    reader.fill_exact(100).unwrap();

    assert_eq!(reader.buffer.len(), 100);
    assert!(reader.buffer.buf().iter().all(|&b| b == 42));

    // EOF: request more than available
    let data = b"short";
    let cur = Cursor::new(data.as_slice());
    let mut reader = Reader::new(cur);
    let err = reader.fill_exact(1000).unwrap_err();

    assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);

    // Exceeds max_capacity: should return an error
    let data = vec![0u8; 4 * CHUNK_SIZE];
    let cur = Cursor::new(data);
    let mut reader = Reader::builder(cur)
        .max_capacity(2 * CHUNK_SIZE)
        .build();
    let err = reader.fill_exact(3 * CHUNK_SIZE).unwrap_err();

    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn test_reader_fill_to_end() {
    // Basic: read all data from a small reader
    let data = vec![42u8; 100];
    let cur = Cursor::new(data);
    let mut reader = Reader::new(cur);
    let bytes_read = reader.fill_to_end().unwrap();

    assert_eq!(bytes_read, 100);
    assert_eq!(reader.buffer.len(), 100);

    // Empty reader
    let cur: Cursor<&[u8]> = Cursor::new(b"");
    let mut reader = Reader::new(cur);
    let bytes_read = reader.fill_to_end().unwrap();

    assert_eq!(bytes_read, 0);

    // Capped: data larger than max_capacity, reading should stop at the cap
    let data = vec![0u8; 4 * CHUNK_SIZE];
    let cur = Cursor::new(data);
    let mut reader = Reader::builder(cur)
        .max_capacity(2 * CHUNK_SIZE)
        .build();
    let bytes_read = reader.fill_to_end().unwrap();

    // Should have read up to the cap, not all data
    assert_eq!(reader.buffer.cap(), 2 * CHUNK_SIZE);
    assert!(bytes_read <= 2 * CHUNK_SIZE);
    assert!(reader.buffer.len() <= 2 * CHUNK_SIZE);
}

#[test]
fn test_reader_fill_until() {
    // Delimiter found
    let cur = Cursor::new(b"key=value\nother".as_slice());
    let mut reader = Reader::new(cur);
    let bytes_read = reader.fill_until(b'\n').unwrap();

    assert!(bytes_read > 0);
    assert!(reader.buffer.buf()[..reader.buffer.len()].contains(&b'\n'));

    // Delimiter not found, EOF
    let cur = Cursor::new(b"no newline here".as_slice());
    let mut reader = Reader::new(cur);
    let bytes_read = reader.fill_until(b'\n').unwrap();

    assert_eq!(bytes_read, 15);
    assert!(!reader.buffer.buf()[..reader.buffer.len()].contains(&b'\n'));

    // Delimiter not found, capped
    let data = vec![b'x'; 4 * CHUNK_SIZE];
    let cur = Cursor::new(data);
    let mut reader = Reader::builder(cur)
        .max_capacity(2 * CHUNK_SIZE)
        .build();
    let bytes_read = reader.fill_until(b'\n').unwrap();

    assert!(bytes_read <= 2 * CHUNK_SIZE);
    assert!(reader.buffer.len() <= 2 * CHUNK_SIZE);
}

#[test]
fn test_reader_fill_until_char() {
    // Delimiter found (multi-byte UTF-8)
    let cur = Cursor::new("Hello, 世界!".as_bytes());
    let mut reader = Reader::new(cur);
    let bytes_read = reader.fill_until_char('界').unwrap();

    assert!(bytes_read > 0);

    // Delimiter not found, EOF
    let cur = Cursor::new("no match".as_bytes());
    let mut reader = Reader::new(cur);
    let bytes_read = reader.fill_until_char('界').unwrap();

    assert_eq!(bytes_read, 8);

    // Delimiter not found, capped
    let data = "x".repeat(4 * CHUNK_SIZE);
    let cur = Cursor::new(data.as_bytes().to_vec());
    let mut reader = Reader::builder(cur)
        .max_capacity(2 * CHUNK_SIZE)
        .build();
    let bytes_read = reader.fill_until_char('界').unwrap();

    assert!(bytes_read <= 2 * CHUNK_SIZE);
}

#[test]
fn test_reader_fill_until_str() {
    // Needle found
    let cur = Cursor::new(b"Hello, World!END more data".as_slice());
    let mut reader = Reader::new(cur);
    let bytes_read = reader.fill_until_str("END").unwrap();

    assert!(bytes_read > 0);

    // Needle not found, EOF
    let cur = Cursor::new(b"no match here".as_slice());
    let mut reader = Reader::new(cur);
    let bytes_read = reader.fill_until_str("END").unwrap();

    assert_eq!(bytes_read, 13);

    // Needle not found, capped
    let data = vec![b'x'; 4 * CHUNK_SIZE];
    let cur = Cursor::new(data);
    let mut reader = Reader::builder(cur)
        .max_capacity(2 * CHUNK_SIZE)
        .build();
    let bytes_read = reader.fill_until_str("END").unwrap();

    assert!(bytes_read <= 2 * CHUNK_SIZE);

    // Empty needle returns immediately
    let cur = Cursor::new(b"anything".as_slice());
    let mut reader = Reader::new(cur);
    let bytes_read = reader.fill_until_str("").unwrap();

    assert_eq!(bytes_read, 0);
}
