//! Reuse a reader while deciding when to discard or keep already-consumed bytes.
//! Each test follows both sides of a transfer or checks the data remaining after reclamation.

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

/// Fill a large input, consume a prefix, then compact and shrink without losing its suffix.
#[test]
fn test_growth_compaction_and_explicit_reclamation() {
    // A single fill uses the initial capacity and stops when that storage is full
    let data = vec![b'x'; 3 * CHUNK_SIZE];
    let mut reader = Reader::new(Cursor::new(&data));
    let len = reader.fill().unwrap();

    assert_eq!(len, CHUNK_SIZE);
    let len = reader.fill().unwrap();

    assert_eq!(len, 0);
    assert_eq!(reader.capacity(), CHUNK_SIZE);

    // The multi-read fill grows to read the rest and shrinks its excess space at EOF
    let len = reader.fill_to_end().unwrap();

    assert_eq!(len, 2 * CHUNK_SIZE);
    assert_eq!(reader.buffer(), data);
    assert_eq!(reader.capacity(), 3 * CHUNK_SIZE);

    // Compaction removes the consumed prefix, but does not release its capacity
    reader.consume(2 * CHUNK_SIZE);
    reader.compact();

    assert_eq!(reader.buffer(), &data[..CHUNK_SIZE]);
    assert_eq!(reader.pos(), 0);
    assert_eq!(reader.capacity(), 3 * CHUNK_SIZE);

    // Shrink explicitly after a large record has passed through
    reader.shrink();

    assert_eq!(reader.buffer(), &data[..CHUNK_SIZE]);
    assert_eq!(reader.capacity(), CHUNK_SIZE);

    reader.discard();

    assert!(reader.buffer().is_empty());
    assert_eq!(reader.pos(), 0);
    assert_eq!(reader.capacity(), CHUNK_SIZE);
}

/// Reassemble unread input after transferring a whole buffer and then decomposing the reader.
#[test]
fn test_handoff_preserves_read_ahead_and_the_remaining_source() {
    // Only the header is consumed; part of the body is buffered and part is still in the source
    let source = ShortReads {
        inner: Cursor::new(b"headtailrest"),
        limit: 8,
    };
    let mut reader = Reader::new(source);
    reader.fill().unwrap();
    reader.consume(4);
    let saved = reader.take_buffer();

    assert_eq!(saved.buf(), b"headtail");
    assert_eq!(saved.pos(), 4);
    assert!(reader.buffer().is_empty());

    // Refill once, then hand both the source and this second buffer to another consumer
    reader.fill().unwrap();
    let (mut source, buffered) = reader.into_parts();

    assert_eq!(buffered.buf(), b"rest");
    assert_eq!(buffered.pos(), 0);

    // The consumer must use both unread buffer regions before continuing with the source
    let mut remaining = saved.buf()[saved.pos()..].to_vec();
    remaining.extend_from_slice(&buffered.buf()[buffered.pos()..]);
    source.read_to_end(&mut remaining).unwrap();

    assert_eq!(remaining, b"tailrest");
}

/// Alternate keeping and discarding lookbehind over many refills of one bounded reader.
#[test]
fn test_repeated_transfer_and_compaction() {
    // Different record contents expose skipped or duplicated bytes between cycles
    let records: Vec<_> = (0..40)
        .map(|index| format!("record {index}: {}\n", "x".repeat(997)))
        .collect();
    let data = records.concat();
    let mut reader = Reader::builder(Cursor::new(data.as_bytes()))
        .max_capacity(CHUNK_SIZE)
        .build();
    let mut saved = Vec::new();

    for (index, expected) in records.iter().enumerate() {
        reader.fill_until(b'\n').unwrap();
        let len = reader
            .peek(usize::MAX)
            .iter()
            .position(|&byte| byte == b'\n')
            .unwrap()
            + 1;

        // Check the current record before advancing the reader
        assert_eq!(reader.peek(len), expected.as_bytes());
        assert_eq!(reader.pos(), 0);
        assert!(reader.capacity() <= reader.max_capacity());

        reader.consume(len);
        if index % 2 == 0 {
            saved.push(reader.take_consumed());
        } else {
            reader.compact();
        }

        // Either operation makes the next record start at position zero
        assert_eq!(reader.pos(), 0);
        assert!(reader.peek_behind(1).is_empty());
    }

    let len = reader.fill().unwrap();
    assert_eq!(len, 0);
    assert!(reader.buffer().is_empty());
    drop(reader);

    // Earlier transferred records still hold their own bytes after all subsequent refills
    assert_eq!(saved.len(), 20);
    for (buffer, expected) in saved.iter().zip(records.iter().step_by(2)) {
        assert_eq!(buffer.buf(), expected.as_bytes());
        assert_eq!(buffer.pos(), buffer.len());
    }
}
