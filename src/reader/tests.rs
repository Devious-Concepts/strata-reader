//! Tests for the Reader

#![expect(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    reason = "Okay in tests"
)]

use super::*;
use crate::buffer::tests::{DirectReadBuf, InterruptOnceReader};
use crate::constants::CHUNK_SIZE;
use std::io::{self, BorrowedBuf, Cursor, IoSliceMut, Read, Seek, SeekFrom};
use std::mem::MaybeUninit;

/// A reader that counts `Seek::seek` calls and optionally fails the Nth one.
///
/// `fail_on_nth` is 1-indexed: `Some(2)` makes the second seek call return
/// `InvalidInput` without delegating to `inner`.
struct SeekCountingReader<R> {
    inner: R,
    seek_count: usize,
    fail_on_nth: Option<usize>,
}

impl<R: Read> Read for SeekCountingReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf)
    }
}

impl<R: Seek> Seek for SeekCountingReader<R> {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.seek_count += 1;
        if Some(self.seek_count) == self.fail_on_nth {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "seek failed"));
        }
        self.inner.seek(pos)
    }
}

// -----------------------------------------------------------------------------
// Reader - impl Read
// -----------------------------------------------------------------------------

#[test]
fn test_reader_read_read() {
    // Create a reader against no data
    let cur = Cursor::<&str>::default();
    let mut reader = Reader::new(cur);
    let mut buf = [0u8; 123];
    let len = reader.read(&mut buf).unwrap();

    // Check that the state matches expectations
    assert_eq!(len, 0);

    // Create a reader against some data
    let data = "Hello, World!";
    let cur = Cursor::new(data);
    let mut reader = Reader::new(cur);
    let mut buf = [0u8; 5];
    let len = reader.read(&mut buf).unwrap();

    // Check that the state matches expectations
    assert_eq!(len, buf.len()); // buf was filled
    assert_eq!(buf, &data.as_bytes()[..5]); // data matches

    // Create a reader with buffered data
    let mut cur = Cursor::new(data);
    cur.set_position(6); // simulate having read the first 6 bytes
    let mut reader = Reader::new(cur);
    reader.buffer.inject_test_data(&data.as_bytes()[..6]); // inject the above 6 bytes
    let mut buf = [0u8; 3];

    // Check that the state matches expectations
    assert_eq!(reader.buffer.pos(), 0);
    assert_eq!(reader.buffer.len(), 6);

    // First read, should hit the buffer
    let len = reader.read(&mut buf).unwrap();

    // Check that the state matches expectations
    assert_eq!(len, 3);
    assert_eq!(buf, &data.as_bytes()[..3]);
    assert_eq!(reader.buffer.pos(), 3); // read 3 from the buffer
    assert_eq!(reader.buffer.len(), 6);

    // Second read, should hit the buffer
    let len = reader.read(&mut buf).unwrap();

    // Check that the state matches expectations
    assert_eq!(len, 3);
    assert_eq!(buf, &data.as_bytes()[3..6]);
    assert_eq!(reader.buffer.pos(), 6); // read another 3 from the buffer
    assert_eq!(reader.buffer.len(), 6);

    // Third read, should cause a fill_buf, then hit the buffer
    let len = reader.read(&mut buf).unwrap();

    // Check that the state matches expectations
    assert_eq!(len, 3);
    assert_eq!(buf, &data.as_bytes()[6..9]);
    assert_eq!(reader.buffer.pos(), 3); // read 3 from the newly filled buffer
    assert_eq!(reader.buffer.len(), 7); // buffer contains rest of the data

    // Fourth read, should hit the buffer
    let len = reader.read(&mut buf).unwrap();

    // Check that the state matches expectations
    assert_eq!(len, 3);
    assert_eq!(buf, &data.as_bytes()[9..12]);
    assert_eq!(reader.buffer.pos(), 6); // read another 3 from the buffer
    assert_eq!(reader.buffer.len(), 7);

    // Fifth read, we now hit EOF after 1 byte
    let len = reader.read(&mut buf).unwrap();

    // Check that the state matches expectations
    assert_eq!(len, 1);
    assert_eq!(&buf[..len], &data.as_bytes()[12..]);
    assert_eq!(reader.buffer.pos(), 7); // read another 1 from the buffer
    assert_eq!(reader.buffer.len(), 7);

    /* Create a reader against some data, using a buffer bigger than the current capacity.
    This should cause the operation to delegate to the inner reader to save copying data. */
    let cur = Cursor::new(data);
    let mut reader = Reader::new(cur); // initial capacity is `CHUNK_SIZE`
    let mut buf = [0u8; 2 * CHUNK_SIZE]; // bigger than initial capacity
    let len = reader.read(&mut buf).unwrap();

    // Check that the state matches expectations
    assert_eq!(len, data.len());
    assert_eq!(&buf[..len], data.as_bytes());
    assert_eq!(reader.buffer.len(), 0); // buffer was skipped
}

#[test]
fn test_reader_read_read_buf() {
    let data = b"direct read_buf";
    let inner = DirectReadBuf {
        data,
        observed_init: Vec::new(),
    };
    let mut reader = Reader::new(inner);
    let mut storage = [MaybeUninit::<u8>::uninit(); CHUNK_SIZE];
    let mut destination = BorrowedBuf::from(storage.as_mut_slice());

    // A large destination bypasses the empty internal buffer without initializing it.
    reader.read_buf(destination.unfilled()).unwrap();
    assert_eq!(destination.filled(), data);
    assert_eq!(reader.get_ref().observed_init, [false]);
    assert!(reader.buffer.is_empty());

    // Existing buffered data is copied directly into another uninitialized destination.
    let inner = DirectReadBuf {
        data: b"inner",
        observed_init: Vec::new(),
    };
    let mut reader = Reader::new(inner);
    reader.buffer.inject_test_data(b"buffered");
    let mut storage = [MaybeUninit::<u8>::uninit(); 4];
    let mut destination = BorrowedBuf::from(storage.as_mut_slice());

    reader.read_buf(destination.unfilled()).unwrap();
    assert_eq!(destination.filled(), b"buff");
    assert_eq!(reader.get_ref().observed_init.as_slice(), &[]);
    assert_eq!(reader.buffer.pos(), 4);
}

#[test]
fn test_reader_read_read_vectored() {
    // Create a reader against some data with small target buffers
    let data = "Hello, World!";
    let cur = Cursor::new(data);
    let mut reader = Reader::new(cur);
    let mut buf1 = [0u8; 5];
    let mut buf2 = [0u8; 2];

    // First read, should fill the buffer and copy from there
    let mut buffers = [IoSliceMut::new(&mut buf1), IoSliceMut::new(&mut buf2)];
    let len = reader.read_vectored(&mut buffers).unwrap();

    // Check that the state matches expectations
    assert_eq!(len, 7);
    assert_eq!(buf1, &data.as_bytes()[..5]);
    assert_eq!(buf2, &data.as_bytes()[5..7]);
    assert_eq!(reader.buffer.pos(), 7); // confirm buffer usage, and bytes being consumed

    // Second read, should hit the buffer
    let mut buffers = [IoSliceMut::new(&mut buf1), IoSliceMut::new(&mut buf2)];
    let len = reader.read_vectored(&mut buffers).unwrap();

    // Check that the state matches expectations
    assert_eq!(len, 6);
    assert_eq!(buf1, &data.as_bytes()[7..12]);
    assert_eq!(&buf2[..1], &data.as_bytes()[12..]);
    assert_eq!(reader.buffer.pos(), data.len()); // all bytes consumed

    /* Create a reader against some data, using a buffer bigger than the current capacity.
    This should cause the operation to delegate to the inner reader to save copying data. */
    let cur = Cursor::new(data);
    let mut reader = Reader::new(cur); // initial capacity is `CHUNK_SIZE`
    let mut buf1 = [0u8; 5]; // reset buf1, replace buf2
    let mut buf2 = [0u8; 2 * CHUNK_SIZE]; // bigger than initial capacity
    let mut buffers = [IoSliceMut::new(&mut buf1), IoSliceMut::new(&mut buf2)];
    let len = reader.read_vectored(&mut buffers).unwrap();

    // Check that the state matches expectations
    assert_eq!(len, data.len());
    assert_eq!(buf1, data.as_bytes()[..5]);
    assert_eq!(&buf2[..8], &data.as_bytes()[5..]);
    assert_eq!(reader.buffer.len(), 0); // buffer was skipped
}

#[test]
fn test_reader_read_read_to_end() {
    // Create a reader against some data
    let data = "Hello, World!";
    let mut cur = Cursor::new(data);
    cur.set_position(5); // simulate having read the first 5 bytes
    let mut reader = Reader::new(cur);
    reader.buffer.inject_test_data(&data.as_bytes()[..5]); // inject the above 5 bytes

    // Check that the state matches expectations
    assert_eq!(reader.buffer.pos(), 0);
    assert_eq!(reader.buffer.len(), 5);

    // Read to end, should take data from the buffer then delegate to the internal reader
    let mut buf = Vec::new();
    let len = reader.read_to_end(&mut buf).unwrap();

    // Check that the state matches expectations
    assert_eq!(len, data.len());
    assert_eq!(buf, data.as_bytes());
    assert_eq!(reader.buffer.len(), 0); // buffer should be cleared after being exhausted
}

#[test]
fn test_reader_read_read_to_string() {
    // Create a reader against some UTF-8 data, with an empty string target
    let data = "Hello, World!";
    let cur = Cursor::new(data);
    let mut reader = Reader::new(cur);
    let mut buf = String::new();
    let len = reader.read_to_string(&mut buf).unwrap();

    // Check that the state matches expectations
    assert_eq!(len, data.len());
    assert_eq!(buf, data);

    // Create a reader against some non-UTF-8 data,  with an empty string target
    let data = vec![0xFF, 0xFE, 0xFD]; // Invalid UTF-8
    let cur = Cursor::new(data);
    let mut reader = Reader::new(cur);
    let mut buf = String::new();
    let err = reader.read_to_string(&mut buf).unwrap_err();

    // Check that the state matches expectations
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);

    // The above two tests hit the optimized path, the below two the fallback path

    // Create a reader against some UTF-8 data, with a non-empty target string
    let data = "World!";
    let cur = Cursor::new(data);
    let mut reader = Reader::new(cur);
    let mut buf = String::from("Hello, ");
    let len = reader.read_to_string(&mut buf).unwrap();

    // Check that the state matches expectations
    assert_eq!(len, data.len());
    assert_eq!(buf, "Hello, World!");

    // Create a reader against some non-UTF-8 data, with a non-empty target string
    let data = vec![0xFF, 0xFE, 0xFD]; // Invalid UTF-8
    let cur = Cursor::new(data);
    let mut reader = Reader::new(cur);
    let mut buf = String::from("keep me");
    let err = reader.read_to_string(&mut buf).unwrap_err();

    // Check that the state matches expectations
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    assert_eq!(buf, "keep me");
}

#[test]
fn test_reader_read_read_exact() {
    // Create a reader against some buffered data and read less data
    let data = "Hello, World!";
    let mut cur = Cursor::new(data);
    cur.set_position(13); // simulate having read all bytes
    let mut reader = Reader::new(cur);
    reader.buffer.inject_test_data(data.as_bytes()); // inject all bytes

    // Check that the state matches expectations
    assert_eq!(reader.buffer.len(), 13); // all data is buffered

    // Read some of it
    let mut buf = [0u8; 5];
    reader.read_exact(&mut buf).unwrap();

    // Check that the state matches expectations
    assert_eq!(buf, &data.as_bytes()[..5]);
    assert_eq!(reader.buffer.pos(), 5); // bytes were consumed

    // Create a reader against some unbuffered data and read less data
    let cur = Cursor::new(data);
    let mut reader = Reader::new(cur);

    // Check that the state matches expectations
    assert_eq!(reader.buffer.len(), 0); // no data is buffered

    // Read some of it
    let mut buf = [0u8; 5];
    reader.read_exact(&mut buf).unwrap();

    // Check that the state matches expectations
    assert_eq!(buf, &data.as_bytes()[..5]);
    assert_eq!(reader.buffer.len(), 13); // data was buffered
    assert_eq!(reader.buffer.pos(), 5); // bytes were consumed

    // Create a reader against some data and attempt to read more data
    let cur = Cursor::new(data);
    let mut reader = Reader::new(cur);
    let mut buf = [0u8; 123];
    let err = reader.read_exact(&mut buf).unwrap_err();

    // Check that the state matches expectations
    assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
    /* This assertion is "undefined behavior" according to the `Read` contract.
    But all bytes up-to an EOF will be read in our implementation. */
    assert_eq!(&buf[..data.len()], data.as_bytes());

    // Create a reader that is interrupted once against some data
    let cur = Cursor::new(data);
    let reader = InterruptOnceReader {
        inner: cur,
        interrupted: false,
    };
    let mut reader = Reader::new(reader);
    let mut buf = [0u8; 13];
    reader.read_exact(&mut buf).unwrap();

    // Check that the state matches expectations
    assert_eq!(buf, data.as_bytes());
    assert_eq!(reader.buffer.len(), 13); // data was buffered
    assert_eq!(reader.buffer.pos(), 13); // bytes were consumed
}

// -----------------------------------------------------------------------------
// Reader - impl BufRead
// -----------------------------------------------------------------------------

#[test]
fn test_reader_bufread_fill_buf() {
    // Create a reader against some data
    let data = "Hello, World!";
    let cur = Cursor::new(data);
    let mut reader = Reader::new(cur);

    // Check that the state matches expectations
    assert_eq!(reader.buffer.len(), 0); // no data is buffered

    // Read the data
    let slice = reader.fill_buf().unwrap();

    // Check that the state matches expectations
    assert_eq!(slice, data.as_bytes()); // all data was read
    assert_eq!(reader.buffer.len(), 13); // data was buffered
    assert_eq!(reader.buffer.pos(), 0); // no bytes were consumed

    // Consume some data
    reader.consume(7);

    // Read again
    let slice = reader.fill_buf().unwrap();

    // Check that the state matches expectations
    assert_eq!(slice, &data.as_bytes()[7..]); // non-consumed data was read
    assert_eq!(reader.buffer.len(), 13); // no change yet
    assert_eq!(reader.buffer.pos(), 7); // bytes were consumed

    // Consume the rest
    reader.consume(data.len() - 7);

    // Check that the state matches expectations
    assert_eq!(reader.buffer.pos(), data.len()); // all bytes were consumed

    // Attempt to read more
    let slice = reader.fill_buf().unwrap();

    // Check that the state matches expectations
    assert_eq!(slice, &[]); // nothing was read
    assert_eq!(reader.buffer.len(), 0); // data was cleared

    // Make a new reader against more data than can be read in one go
    let mut big_data = "A".repeat(CHUNK_SIZE);
    big_data.push_str(data);
    let cur = Cursor::new(big_data);
    let mut reader = Reader::new(cur); // initial capacity is `CHUNK_SIZE`

    // Read the "new" data
    let slice = reader.fill_buf().unwrap();

    // Check that the state matches expectations
    assert_eq!(&slice[slice.len() - 1..], b"A"); // last byte is "A"
    assert_eq!(reader.buffer.len(), CHUNK_SIZE); // buffer was filled

    // Consume all read data
    reader.consume(CHUNK_SIZE);

    // Check that the state matches expectations
    assert_eq!(reader.buffer.pos(), CHUNK_SIZE); // all bytes were consumed

    // Read the remaining data
    let slice = reader.fill_buf().unwrap();

    // Check that the state matches expectations
    assert_eq!(&slice[..data.len()], data.as_bytes()); // data matches
    assert_eq!(reader.buffer.len(), data.len()); // data was buffered
}

/* Coverage note: `BufRead::consume` is a thin wrapper over `Buffer::consume`.
 * Its behavior is covered by the `Buffer::consume` test.
 */

// -----------------------------------------------------------------------------
// Reader - impl Seek
// -----------------------------------------------------------------------------

#[test]
fn test_reader_seek_seek() {
    // Create a reader against some data
    let data = "Hello, World!";
    let mut cur = Cursor::new(data);
    cur.set_position(13); // simulate having read all bytes
    let mut reader = Reader::new(cur);
    reader.buffer.inject_test_data(data.as_bytes()); // inject the above 13 bytes
    reader.buffer.consume(5); // consume a bit

    // Seeking from an absolute position clears retained buffered data
    let pos = reader.seek(SeekFrom::Start(1)).unwrap();

    // Check that the state matches expectations
    assert_eq!(pos, 1);
    assert_eq!(reader.inner.position(), 1);
    assert_eq!(reader.buffer.pos(), 0); // seek clears the buffer
    assert_eq!(reader.buffer.len(), 0); // seek clears the buffer

    // Create a reader against some data
    let mut cur = Cursor::new(data);
    cur.set_position(13); // simulate having read all bytes
    let mut reader = Reader::new(cur);
    reader.buffer.inject_test_data(data.as_bytes()); // inject the above 13 bytes
    reader.buffer.consume(5); // consume a bit

    /* Seeking from the current logical position factors in unconsumed buffered data to calculate
    the correct inner offset. */
    let pos = reader.seek(SeekFrom::Current(2)).unwrap();

    // Check that the state matches expectations
    assert_eq!(pos, 7); // logical position 5 + 2
    assert_eq!(reader.inner.position(), 7);
    assert_eq!(reader.buffer.pos(), 0); // seek clears the buffer
    assert_eq!(reader.buffer.len(), 0); // seek clears the buffer

    // Create a reader against some data
    let mut cur = Cursor::new(data);
    cur.set_position(5); // simulate having read the first 5 bytes
    let mut reader = Reader::new(cur);
    reader.buffer.inject_test_data(&data.as_bytes()[..5]); // inject the above 5 bytes

    // A failed delegated seek preserves retained buffered data
    let err = reader.seek(SeekFrom::Current(-1)).unwrap_err();

    // Check that the state matches expectations
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    assert_eq!(reader.buffer.pos(), 0); // failed seek does not consume buffered data
    assert_eq!(reader.buffer.len(), 5); // failed seek does not clear buffered data
    assert_eq!(reader.inner.position(), 5);

    // Create a seek-counting reader against a massive amount of data
    let mut cur = Cursor::new(Vec::<u8>::new());
    cur.set_position((1_u64 << 63) + 1); // high enough for both delegated seeks to succeed
    let inner = SeekCountingReader {
        inner: cur,
        seek_count: 0,
        fail_on_nth: None,
    };
    let mut reader = Reader::new(inner);
    reader.buffer.inject_test_data(b"x"); // one unconsumed byte triggers offset underflow

    // If the adjusted current seek would underflow `i64`, it seeks in two steps
    let pos = reader.seek(SeekFrom::Current(i64::MIN)).unwrap();

    // Check that the state matches expectations
    assert_eq!(pos, 0); // logical position 2^63 + i64::MIN
    assert_eq!(reader.inner.inner.position(), 0);
    assert_eq!(reader.inner.seek_count, 2); // underflow path issues two inner seeks
    assert_eq!(reader.buffer.pos(), 0); // underflow fallback clears the buffer
    assert_eq!(reader.buffer.len(), 0); // underflow fallback clears the buffer

    // Create a reader against a tiny amount of data
    let mut cur = Cursor::new(Vec::<u8>::new());
    cur.set_position(1);
    let inner = SeekCountingReader {
        inner: cur,
        seek_count: 0,
        fail_on_nth: Some(1), // fail the rewind-by-buffered-tail step
    };
    let mut reader = Reader::new(inner);
    reader.buffer.inject_test_data(b"x");

    // If the first step of the underflow path fails, the buffer is preserved
    let err = reader.seek(SeekFrom::Current(i64::MIN)).unwrap_err();

    // Check that the state matches expectations
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    assert_eq!(reader.inner.seek_count, 1); // bailed out after the first step
    assert_eq!(reader.inner.inner.position(), 1); // inner reader was not moved
    assert_eq!(reader.buffer.pos(), 0); // failed first step preserves buffered data
    assert_eq!(reader.buffer.len(), 1); // failed first step preserves buffered data

    // Create a reader against a tiny amount of data
    let mut cur = Cursor::new(Vec::<u8>::new());
    cur.set_position(1); // one byte ahead of the logical position
    let inner = SeekCountingReader {
        inner: cur,
        seek_count: 0,
        fail_on_nth: Some(2), // explicitly fail the second step of the underflow path
    };
    let mut reader = Reader::new(inner);
    reader.buffer.inject_test_data(b"x"); // one unconsumed byte triggers offset underflow

    /* If the second step fails, the reader is left at the old logical position with an empty
    buffer. (fully synchronized) */
    let err = reader.seek(SeekFrom::Current(i64::MIN)).unwrap_err();

    // Check that the state matches expectations
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    assert_eq!(reader.inner.seek_count, 2); // first step succeeded, second step failed
    assert_eq!(reader.inner.inner.position(), 0); // synchronized before the failing second seek
    assert_eq!(reader.buffer.pos(), 0); // synchronized buffer was cleared
    assert_eq!(reader.buffer.len(), 0); // synchronized buffer was cleared
}

#[test]
fn test_reader_seek_stream_position() {
    // Create a reader against some data
    let data = "Hello, World!";
    let mut cur = Cursor::new(data);
    cur.set_position(5);
    let mut reader = Reader::new(cur);

    // Without buffered data, the logical position is the inner reader position
    let pos = reader.stream_position().unwrap();

    // Check that the state matches expectations
    assert_eq!(pos, 5);
    assert_eq!(reader.buffer.len(), 0); // no data was buffered

    // Create a reader against some data
    let mut cur = Cursor::new(data);
    cur.set_position(13); // simulate having read all bytes
    let mut reader = Reader::new(cur);
    reader.buffer.inject_test_data(data.as_bytes()); // inject the above 13 bytes

    // With unconsumed buffered data, the logical position excludes the unconsumed bytes
    let pos = reader.stream_position().unwrap();

    // Check that the state matches expectations
    assert_eq!(pos, 0); // inner position 13 - unconsumed bytes 13
    assert_eq!(reader.buffer.pos(), 0); // no bytes were consumed
    assert_eq!(reader.buffer.len(), 13); // stream_position preserves the buffer

    // Consumed buffered data counts toward the logical position
    reader.buffer.consume(5);
    let pos = reader.stream_position().unwrap();

    // Check that the state matches expectations
    assert_eq!(pos, 5); // inner position 13 - unconsumed bytes 8
    assert_eq!(reader.buffer.pos(), 5); // consumed bytes were preserved
    assert_eq!(reader.buffer.len(), 13); // stream_position still preserves the buffer

    // If all buffered data is consumed, the logical position matches the inner reader again
    reader.buffer.consume(8);
    let pos = reader.stream_position().unwrap();

    // Check that the state matches expectations
    assert_eq!(pos, 13);
    assert_eq!(reader.buffer.pos(), 13); // all bytes were consumed
    assert_eq!(reader.buffer.len(), 13); // consumed lookbehind is preserved

    // Create a reader against some data
    let mut cur = Cursor::new(data);
    cur.set_position(3); // simulate an impossible state for the injected buffer
    let mut reader = Reader::new(cur);
    reader.buffer.inject_test_data(&data.as_bytes()[..5]); // inject more bytes than inner read

    // If the inner reader is before the unconsumed buffer, the positions are out of sync
    let err = reader.stream_position().unwrap_err();

    // Check that the state matches expectations
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    assert_eq!(reader.buffer.pos(), 0); // the error does not consume buffered data
    assert_eq!(reader.buffer.len(), 5); // the error does not clear buffered data
}

#[test]
fn test_reader_seek_relative() {
    // Create a reader against some data
    let data = "Hello, World!";
    let mut cur = Cursor::new(data);
    cur.set_position(13); // simulate having read all bytes
    let mut reader = Reader::new(cur);
    reader.buffer.inject_test_data(data.as_bytes()); // inject the above 13 bytes

    // Seeking forward inside unconsumed buffered data only moves the buffer position
    reader.seek_relative(5).unwrap();

    // Check that the state matches expectations
    assert_eq!(reader.buffer.pos(), 5);
    assert_eq!(reader.buffer.len(), 13); // buffer preserved when seek is within bounds
    assert_eq!(reader.inner.position(), 13); // reader isn't touched when seek is within bounds

    // Create a reader against some data
    let mut cur = Cursor::new(data);
    cur.set_position(13); // simulate having read all bytes
    let mut reader = Reader::new(cur);
    reader.buffer.inject_test_data(data.as_bytes()); // inject the above 13 bytes
    reader.buffer.consume(7);

    // Seeking backward inside retained consumed data only moves the buffer position
    reader.seek_relative(-5).unwrap();

    // Check that the state matches expectations
    assert_eq!(reader.buffer.pos(), 2);
    assert_eq!(reader.buffer.len(), 13); // buffer preserved when seek is within bounds
    assert_eq!(reader.inner.position(), 13); // reader isn't touched when seek is within bounds

    // Create a reader against some data
    let mut cur = Cursor::new(data);
    cur.set_position(5); // simulate having read the first 5 bytes
    let mut reader = Reader::new(cur);
    reader.buffer.inject_test_data(&data.as_bytes()[..5]); // inject the above 5 bytes
    reader.buffer.consume(3);

    // Seeking forward outside unconsumed buffered data falls back to the Seek implementation
    reader.seek_relative(5).unwrap();

    // Check that the state matches expectations
    assert_eq!(reader.inner.position(), 8); // logical position 3 + 5
    assert_eq!(reader.buffer.pos(), 0); // fallback seek clears the buffer
    assert_eq!(reader.buffer.len(), 0); // fallback seek clears the buffer

    // Create a reader against some data
    let mut cur = Cursor::new(data);
    cur.set_position(13); // simulate having read all bytes
    let mut reader = Reader::new(cur);
    reader.buffer.inject_test_data(&data.as_bytes()[7..]); // inject compacted retained data
    reader.buffer.consume(1);

    // Seeking backward outside retained consumed data falls back to the Seek implementation
    reader.seek_relative(-2).unwrap();

    // Check that the state matches expectations
    assert_eq!(reader.inner.position(), 6); // logical position 8 - 2
    assert_eq!(reader.buffer.pos(), 0); // fallback seek clears the buffer
    assert_eq!(reader.buffer.len(), 0); // fallback seek clears the buffer
}

// -----------------------------------------------------------------------------
// Reader - impl DynamicRead
// -----------------------------------------------------------------------------

/* Coverage note: `DynamicRead` methods are all thin wrappers over `Buffer` methods.
 * Their behavior are covered by the `Buffer` tests.
 */

// -----------------------------------------------------------------------------
// Reader - Creation
// -----------------------------------------------------------------------------

#[test]
fn test_reader_new() {
    let cur = Cursor::<&str>::default();
    let reader = Reader::new(cur);

    // Check that the state matches expectations
    assert_eq!(reader.buffer, Buffer::default());
    assert_eq!(reader.buffer.cap(), CHUNK_SIZE);
    assert_eq!(reader.max_capacity, DEFAULT_MAX_CAPACITY);
    assert_eq!(reader.inner, Cursor::default());
}

#[test]
fn test_reader_builder() {
    // Create a default reader using the builder
    let cur = Cursor::<&str>::default();
    let reader = Reader::builder(cur).build();

    // Check that the state matches expectations
    assert_eq!(reader.buffer, Buffer::default());
    assert_eq!(reader.buffer.cap(), CHUNK_SIZE);
    assert_eq!(reader.max_capacity, DEFAULT_MAX_CAPACITY);
    assert_eq!(reader.inner, Cursor::default());

    // Create a reader with a custom initial_capacity
    let cur = Cursor::<&str>::default();
    let initial_capacity = 2 * CHUNK_SIZE + 123;
    let reader = Reader::builder(cur)
        .initial_capacity(initial_capacity)
        .build();

    // Check that the state matches expectations
    assert_eq!(reader.buffer, Buffer::default());
    assert_eq!(reader.buffer.cap(), 3 * CHUNK_SIZE); // Rounds up linearly
    assert_eq!(reader.max_capacity, DEFAULT_MAX_CAPACITY);
    assert_eq!(reader.inner, Cursor::default());

    // Create a reader with a custom max_capacity
    let cur = Cursor::<&str>::default();
    let max_capacity = 4 * CHUNK_SIZE + 123;
    let reader = Reader::builder(cur).max_capacity(max_capacity).build();

    // Check that the state matches expectations
    assert_eq!(reader.buffer, Buffer::default());
    assert_eq!(reader.buffer.cap(), CHUNK_SIZE);
    assert_eq!(reader.max_capacity, 8 * CHUNK_SIZE); // Rounds up exponentially
    assert_eq!(reader.inner, Cursor::default());

    // Create a reader with a custom initial_capacity and max_capacity
    let cur = Cursor::<&str>::default();
    let reader = Reader::builder(cur)
        .initial_capacity(initial_capacity)
        .max_capacity(max_capacity)
        .build();

    // Check that the state matches expectations
    assert_eq!(reader.buffer, Buffer::default());
    assert_eq!(reader.buffer.cap(), 3 * CHUNK_SIZE); // Rounds up linearly
    assert_eq!(reader.max_capacity, 8 * CHUNK_SIZE); // Rounds up exponentially
    assert_eq!(reader.inner, Cursor::default());

    // Create a reader with a smaller max_capacity than initial_capacity
    let cur = Cursor::<&str>::default();
    let reader = Reader::builder(cur)
        .initial_capacity(initial_capacity)
        .max_capacity(CHUNK_SIZE)
        .build();

    // Check that the state matches expectations
    assert_eq!(reader.buffer, Buffer::default());
    assert_eq!(reader.buffer.cap(), 3 * CHUNK_SIZE); // Rounds up linearly
    assert_eq!(reader.max_capacity, 3 * CHUNK_SIZE); // Raised to match initial
    assert_eq!(reader.inner, Cursor::default());
}

// -----------------------------------------------------------------------------
// Reader - Accessors
// -----------------------------------------------------------------------------

#[test]
fn test_reader_into_parts() {
    let data = "Hello, World!";
    let cur = Cursor::new(data);
    let mut reader = Reader::new(cur);

    // Read a bit then get the inner parts
    reader.buffer.fill_exact(&mut reader.inner, 5).unwrap();
    /* We used Buffer::fill_exact here to avoid using Reader::fill_exact before it has been
    tested. This maintains the narrative style of our test files by using a black box instead. */
    let (inner_reader, buffer) = reader.into_parts();

    // Create expected state
    let mut expected_cur = Cursor::new(data);
    expected_cur.consume(5);
    let mut expected_buffer = Buffer::default();
    expected_buffer.inject_test_data(&data.as_bytes()[..5]);

    // Check that the state matches expectations
    assert_eq!(inner_reader, expected_cur);
    assert_eq!(buffer, expected_buffer);
}

#[test]
fn test_reader_get_ref() {
    let data = "Hello, World!";
    let cur = Cursor::new(data);
    let mut reader = Reader::new(cur);

    // Read a bit then get a reference to the inner reader
    reader.buffer.fill_exact(&mut reader.inner, 5).unwrap();
    /* We used Buffer::fill_exact here to avoid using Reader::fill_exact before it has been
    tested. This maintains the narrative style of our test files by using a black box instead. */
    let inner_reader = reader.get_ref();

    // Check that the state matches expectations
    assert_eq!(inner_reader.position(), 5);
    assert_eq!(reader.buffer.len(), 5);
}

#[test]
fn test_reader_get_mut() {
    let data = "Hello, World!";
    let cur = Cursor::new(data);
    let mut reader = Reader::new(cur);

    // Read a bit then get a mutable reference to the inner reader and move it a bit
    reader.buffer.fill_exact(&mut reader.inner, 5).unwrap(); // Hello

    /* We used Buffer::fill_exact here to avoid using Reader::fill_exact before it has been
    tested. This maintains the narrative style of our test files by using a black box instead. */

    // We scope the mutable reference so we can drop it implicitly to check it propagated
    {
        let inner_reader = reader.get_mut();
        inner_reader.set_position(7); // 5+2, this skips the comma and space
    }

    // Read a bit more and well see we skipped a bit
    reader.buffer.fill_exact(&mut reader.inner, 5).unwrap(); // World

    /* We used Buffer::fill_exact here to avoid using Reader::fill_exact before it has been
    tested. This maintains the narrative style of our test files by using a black box instead. */

    // Check that the state matches expectations
    assert_eq!(reader.buffer.len(), 10);
    assert_eq!(reader.buffer.buf(), b"HelloWorld");
}

#[test]
fn test_reader_take_buffer() {
    // Create a reader against no data
    let cur = Cursor::<&str>::default();
    let mut reader = Reader::new(cur);

    // Taking from a fresh reader yields a default buffer and leaves a default buffer behind
    let taken = reader.take_buffer();

    // Check that the state matches expectations
    assert_eq!(taken, Buffer::default());
    assert_eq!(reader.buffer, Buffer::default());

    // Create a reader against some data
    let data = "Hello, World!";
    let cur = Cursor::new(data);
    let mut reader = Reader::new(cur);
    reader.buffer.fill_exact(&mut reader.inner, 5).unwrap();
    reader.buffer.consume(2);
    /* We used Buffer::fill_exact here to avoid using Reader::fill_exact before it has been
    tested. This maintains the narrative style of our test files by using a black box instead. */

    // Build the expected taken buffer
    let mut expected_taken = Buffer::default();
    expected_taken.inject_test_data(&data.as_bytes()[..5]);
    expected_taken.consume(2);

    // Take the buffer
    let taken = reader.take_buffer();

    // Check that the state matches expectations
    assert_eq!(taken, expected_taken); // buffered data and cursor were preserved in the taken buffer
    assert_eq!(reader.buffer, Buffer::default()); // reader holds a fresh default buffer
    assert_eq!(reader.buffer.cap(), CHUNK_SIZE); // replacement starts at default capacity
}

#[test]
fn test_reader_max_capacity() {
    // Create a default reader
    let data = "Hello, World!";
    let cur = Cursor::new(data);
    let reader = Reader::new(cur);

    // Check that the state matches expectations
    assert_eq!(reader.max_capacity(), DEFAULT_MAX_CAPACITY);

    // Create a reader with a custom max capacity
    let cur = Cursor::new(data);
    let reader = Reader::builder(cur).max_capacity(4 * CHUNK_SIZE).build();

    // Check that the state matches expectations
    assert_eq!(reader.max_capacity(), 4 * CHUNK_SIZE);
}

#[test]
fn test_reader_peek() {
    // Create a reader against no data
    let cur = Cursor::<&str>::default();
    let reader = Reader::new(cur);
    let slice = reader.peek(5);

    // Check that the state matches expectations
    assert_eq!(slice, &[]);

    // Create a reader against some data
    let data = "Hello, World!";
    let mut cur = Cursor::new(data);
    cur.set_position(5); // simulate having read the first 5 bytes
    let mut reader = Reader::new(cur);
    reader.buffer.inject_test_data(&data.as_bytes()[..5]); // inject the above 5 bytes

    // Peek within the existing data, and more, and no bytes
    let slice1 = reader.peek(5);
    let slice2 = reader.peek(10);
    let slice3 = reader.peek(0);

    // Check that the state matches expectations
    assert_eq!(slice1, &data.as_bytes()[..5]);
    assert_eq!(slice2, &data.as_bytes()[..5]); // clamped, and confirms peeking didn't consume
    assert_eq!(slice3, &[]);

    // Consume the previous data and read the rest of the data
    reader.consume(5);
    reader.fill().unwrap();

    // Check that the state matches expectations
    assert_eq!(reader.pos(), 5);

    // Peek at the new data
    let slice = reader.peek(data.len() - 5);

    // Check that the state matches expectations
    assert_eq!(slice, &data.as_bytes()[5..]); // the data should match
}

#[test]
fn test_reader_peek_behind() {
    // Create a reader against no data
    let cur = Cursor::<&str>::default();
    let reader = Reader::new(cur);
    let slice = reader.peek_behind(5);

    // Check that the state matches expectations
    assert_eq!(slice, &[]);

    // Create a reader against some data
    let data = "Hello, World!";
    let cur = Cursor::new(data);
    let mut reader = Reader::new(cur);
    reader.fill().unwrap(); // read all the data
    reader.consume(5); // mark the first 5 bytes as consumed

    // Peek within the existing data, and more, and no bytes
    let slice1 = reader.peek_behind(5);
    let slice2 = reader.peek_behind(10);
    let slice3 = reader.peek_behind(0);

    // Check that the state matches expectations
    assert_eq!(slice1, &data.as_bytes()[..5]);
    assert_eq!(slice2, &data.as_bytes()[..5]); // clamped, and confirms peeking didn't unconsume
    assert_eq!(slice3, &[]);

    // Consume the rest of the data
    reader.consume(data.len() - 5);

    // Check that the state matches expectations
    assert_eq!(reader.pos(), data.len());

    // Peek at the newly consumed data and all data
    let slice1 = reader.peek_behind(6);
    let slice2 = reader.peek_behind(data.len());

    // Check that the state matches expectations
    assert_eq!(slice1, &data.as_bytes()[data.len() - 6..]);
    assert_eq!(slice2, data.as_bytes());
}

// -----------------------------------------------------------------------------
// Reader - Fill methods
// -----------------------------------------------------------------------------

#[test]
fn test_reader_ensure_fill_within_max_capacity() {
    // Create a reader with a set max capacity
    let cur = Cursor::<&str>::default();
    let reader = Reader::builder(cur).max_capacity(4 * CHUNK_SIZE).build();

    // Check that the state matches expectations
    assert_eq!(reader.max_capacity(), 4 * CHUNK_SIZE);

    // An empty buffer may fill exactly to max_capacity
    reader
        .ensure_fill_within_max_capacity(4 * CHUNK_SIZE)
        .unwrap();

    // A request beyond max_capacity is rejected
    let err = reader
        .ensure_fill_within_max_capacity(4 * CHUNK_SIZE + 1)
        .unwrap_err();

    // Check that the state matches expectations
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);

    // Create a reader with a set initial and max capacity
    let cur = Cursor::<&str>::default();
    let mut reader = Reader::builder(cur)
        .initial_capacity(2 * CHUNK_SIZE)
        .max_capacity(4 * CHUNK_SIZE)
        .build();
    reader.buffer.inject_test_data(&vec![0u8; CHUNK_SIZE + 123]);
    let remaining = reader.max_capacity() - reader.buffer.len();

    // Check that the state matches expectations
    assert_eq!(reader.buffer.len(), CHUNK_SIZE + 123);

    // A non-aligned buffer length may fill the exact remaining amount
    reader.ensure_fill_within_max_capacity(remaining).unwrap();

    // Anything beyond the exact remaining amount is rejected
    let err = reader
        .ensure_fill_within_max_capacity(remaining + 1)
        .unwrap_err();

    // Check that the state matches expectations
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn test_reader_fill_amount() {
    // Create a reader against more than the max capacity data
    let data = "A".repeat(5 * CHUNK_SIZE);
    let cur = Cursor::new(&data);
    let mut reader = Reader::builder(cur).max_capacity(4 * CHUNK_SIZE).build();

    // Read a bit
    let len = reader.fill_amount(10).unwrap();

    // Check that the state matches expectations
    assert!(len >= 10); // "at least" the requested amount was read

    // Read an amount that would have fit exactly if we didn't read already
    let err = reader.fill_amount(4 * CHUNK_SIZE).unwrap_err();

    // Check that the state matches expectations
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput); // too much data requested

    // Create a reader against more than the max capacity data
    let cur = Cursor::new(&data);
    let mut reader = Reader::builder(cur).max_capacity(4 * CHUNK_SIZE).build();

    // Read exactly the max capacity amount
    let len = reader.fill_amount(4 * CHUNK_SIZE).unwrap();

    // Check that the state matches expectations
    assert_eq!(len, 4 * CHUNK_SIZE); // exact, since we're capped
}

#[test]
fn test_reader_fill_exact() {
    // Create a reader against more than the max capacity data
    let data = "A".repeat(5 * CHUNK_SIZE);
    let cur = Cursor::new(&data);
    let mut reader = Reader::builder(cur).max_capacity(4 * CHUNK_SIZE).build();

    // Read a bit
    reader.fill_exact(10).unwrap();

    // Check that the state matches expectations
    assert_eq!(reader.buffer.len(), 10); // exactly the requested amount was read

    // Read an amount that would have fit exactly if we didn't read already
    let err = reader.fill_exact(4 * CHUNK_SIZE).unwrap_err();

    // Check that the state matches expectations
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput); // too much data requested

    // Read exactly what remains until the max capacity amount
    reader.fill_exact(4 * CHUNK_SIZE - 10).unwrap();

    // Check that the state matches expectations
    assert_eq!(reader.buffer.len(), reader.max_capacity()); // exactly matches max capacity
}

/* Coverage note: `fill_to_end` is a thin wrapper over `DynamicRead::fill_while_dyn` which is a thin
 * wrapper over `Buffer::fill_while`.
 * Its behavior is covered by the `Buffer::fill_while` test.
 */

/* Coverage note: `fill_until` is a thin wrapper over `Buffer::fill_until`.
 * Its behavior is covered by the `Buffer::fill_until` test.
 */

/* Coverage note: `fill_until_char` is a thin wrapper over `Buffer::fill_until_char`.
 * Its behavior is covered by the `Buffer::fill_until_char` test.
 */

/* Coverage note: `fill_until_str` is a thin wrapper over `Buffer::fill_until_str`.
 * Its behavior is covered by the `Buffer::fill_until_str` test.
 */
