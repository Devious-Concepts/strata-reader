//! Use Buffer directly when the caller supplies each input fragment and controls reclamation.
//! Unlike Reader's byte counts, Buffer's fill results also distinguish EOF from a capacity stop.

use std::io::Cursor;
use strata_reader::{
    buffer::{Buffer, UnboundedFillResult},
    constants::CHUNK_SIZE,
};

/// Preserve an incomplete UTF-8 character while processing and compacting earlier text.
#[test]
fn test_text_fragments_with_an_incomplete_character() {
    let data = "A界B".as_bytes();
    let mut buffer = Buffer::new();

    // The first fragment ends after the leading byte of 界
    buffer.fill(Cursor::new(&data[..2])).unwrap();

    assert_eq!(buffer.buf(), &data[..2]);
    assert_eq!(buffer.as_str().unwrap(), "A");

    // Consume the complete text, preserving the unfinished character through compaction
    buffer.consume(1);
    buffer.compact();
    buffer.fill(Cursor::new(&data[2..3])).unwrap();

    assert_eq!(buffer.pos(), 0);
    assert_eq!(buffer.buf(), &data[1..3]);
    assert_eq!(buffer.as_str().unwrap(), ""); // Two of the character's three bytes

    // Completing the character makes it available with the following text
    buffer.fill(Cursor::new(&data[3..])).unwrap();

    assert_eq!(buffer.as_str().unwrap(), "界B");
    buffer.consume("界".len());
    let saved = buffer.take_consumed();

    assert_eq!(saved.buf(), "界".as_bytes());
    assert_eq!(buffer.as_str().unwrap(), "B");
    assert_eq!(buffer.pos(), 0);
}

/// Respond differently to an exhausted retention window and an exhausted source.
#[test]
fn test_capacity_stop_then_eof() {
    let mut data = vec![b'x'; CHUNK_SIZE];
    data.extend_from_slice(b"\ntail");
    let mut source = Cursor::new(&data);
    let mut buffer = Buffer::new();

    // The buffer limit stops the search before the newline is read
    let result = buffer
        .fill_until(&mut source, b'\n', Some(CHUNK_SIZE))
        .unwrap();

    assert_eq!(result, UnboundedFillResult::Capped(CHUNK_SIZE));
    assert_eq!(source.position(), u64::try_from(CHUNK_SIZE).unwrap());
    assert_eq!(buffer.buf(), &data[..CHUNK_SIZE]);

    // Process that prefix and free its space, then resume the same search
    buffer.consume(CHUNK_SIZE);
    buffer.compact();
    let result = buffer
        .fill_until(&mut source, b'\n', Some(CHUNK_SIZE))
        .unwrap();

    assert_eq!(result, UnboundedFillResult::Complete(5));
    assert_eq!(buffer.buf(), b"\ntail");

    // Consume the newline; the next search reaches EOF with an unterminated tail
    buffer.consume(1);
    let result = buffer
        .fill_until(&mut source, b'\n', Some(CHUNK_SIZE))
        .unwrap();

    assert_eq!(result, UnboundedFillResult::Eof(0));
    assert_eq!(buffer.as_str().unwrap(), "tail");
    assert_eq!(buffer.pos(), 1);
}
