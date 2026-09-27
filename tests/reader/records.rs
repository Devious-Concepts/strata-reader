//! Read complete records when the source splits delimiters or the buffer must grow.
//! These scenarios check the bytes kept for the next record as well as the record just read.

use std::io::{self, BufRead, Cursor, Read};
use strata_reader::{DynamicRead, Reader, constants::CHUNK_SIZE};

/// Limit each read without changing the source's bytes or EOF behavior.
struct ShortReads<R> {
    inner: R,
    limit: usize,
}

impl<R: Read> Read for ShortReads<R> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let len = output.len().min(self.limit);
        self.inner.read(&mut output[..len])
    }
}

/// Read two CRLF records, save the first, then inspect an unterminated tail at EOF.
#[test]
fn test_records_cross_reads_and_transfer_consumed_data() {
    let data = b"first\r\nsecond\r\nlast";
    let source = ShortReads {
        inner: Cursor::new(data),
        limit: 2,
    };
    let mut reader = Reader::new(source);

    // CR and LF arrive separately. Filling may also read beyond the delimiter.
    reader.fill_until_str("\r\n").unwrap();
    assert_eq!(reader.peek(7), b"first\r\n");
    reader.consume(7);
    assert_eq!(reader.peek_behind(7), b"first\r\n");
    let first = reader.take_consumed();
    assert_eq!(first.buf(), b"first\r\n");
    assert_eq!(first.pos(), first.len());
    assert_eq!(reader.pos(), 0);

    reader.fill_until_str("\r\n").unwrap();
    assert_eq!(reader.peek(8), b"second\r\n");
    reader.consume(8);
    reader.compact();
    assert!(reader.peek_behind(1).is_empty());

    // A final unterminated record is still available when filling reaches EOF.
    reader.fill_until_str("\r\n").unwrap();
    assert_eq!(reader.peek(usize::MAX), b"last");
    let len = reader.fill_until_str("\r\n").unwrap();

    // Check the number of new bytes, separately from the retained buffer below
    assert_eq!(len, 0);
    drop(reader);
    assert_eq!(first.buf(), b"first\r\n");
}

/// Read UTF-8 delimiters byte by byte, then try text and byte searches on invalid UTF-8.
#[test]
fn test_utf8_delimiters_cross_reads_and_invalid_text_is_reported() {
    let source = ShortReads {
        inner: Cursor::new("a界b終端tail"),
        limit: 1,
    };
    let mut reader = Reader::new(source);
    reader.fill_until_char('界').unwrap();
    assert_eq!(reader.peek(usize::MAX), "a界".as_bytes());
    reader.consume("a界".len());
    reader.fill_until_str("終端").unwrap();
    assert_eq!(reader.peek(usize::MAX), "b終端".as_bytes());
    assert_eq!(reader.peek_behind(usize::MAX), "a界".as_bytes());

    let mut invalid = Reader::new(Cursor::new(b"a\xffend"));
    assert_eq!(
        invalid.fill_until_str("end").unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
    // Byte searches remain usable for binary input already retained by a failed text search.
    assert_eq!(invalid.fill_until(b'd').unwrap(), 0);
    assert_eq!(invalid.peek(usize::MAX), b"a\xffend");
}

/// Split CRLF at the initial buffer boundary and preserve the following record during transfer.
#[test]
fn test_long_record_grows_across_buffer_and_delimiter_boundaries() {
    let mut data = vec![b'x'; CHUNK_SIZE - 1];
    data.extend_from_slice(b"\r\ntail");
    let mut reader = Reader::builder(Cursor::new(&data))
        .max_capacity(2 * CHUNK_SIZE)
        .build();
    reader.fill_until_str("\r\n").unwrap();
    let record_len = CHUNK_SIZE + 1;
    assert_eq!(reader.peek(record_len), &data[..record_len]);
    assert_eq!(reader.capacity(), 2 * CHUNK_SIZE);
    reader.consume(record_len);
    let record = reader.take_consumed();
    assert_eq!(record.buf(), &data[..record_len]);
    assert_eq!(reader.peek(usize::MAX), b"tail");
    assert_eq!(reader.capacity(), CHUNK_SIZE);
    let len = reader.fill_to_end().unwrap();

    // Check the number of new bytes, separately from the retained buffer below
    assert_eq!(len, 0);
}

/// Decode length-prefixed binary frames without treating their payload as text or delimiters.
#[test]
fn test_binary_frames_across_short_reads() {
    // Three frames: binary bytes, an empty payload, and a final text payload
    let data = b"\x00\x03a\x00\xff\x00\x00\x00\x04tail";
    let source = ShortReads {
        inner: Cursor::new(data),
        limit: 1,
    };
    let mut reader = Reader::new(source);

    for expected in [b"a\x00\xff".as_slice(), b"", b"tail"] {
        // Read the length first, then request exactly that many additional bytes
        reader.fill_exact(2).unwrap();
        let header = reader.peek(2);
        let payload_len = usize::from(u16::from_be_bytes([header[0], header[1]]));
        reader.consume(2);
        reader.fill_exact(payload_len).unwrap();

        // The payload is unconsumed; the two-byte header is still available behind it
        assert_eq!(payload_len, expected.len());
        assert_eq!(reader.peek(payload_len), expected);
        assert_eq!(reader.pos(), 2);
        assert_eq!(reader.buffer().len(), 2 + payload_len);

        // Save the entire frame before making room for the next one
        reader.consume(payload_len);
        let frame = reader.take_consumed();

        assert_eq!(&frame.buf()[2..], expected);
        assert_eq!(frame.pos(), frame.len());
        assert_eq!(reader.pos(), 0);
        assert!(reader.buffer().is_empty());
    }

    // All three frames were consumed, including the empty one
    let len = reader.fill().unwrap();
    assert_eq!(len, 0);
    assert_eq!(
        reader.get_ref().inner.position(),
        u64::try_from(data.len()).unwrap()
    );
}
