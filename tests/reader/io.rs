//! Mix retained-buffer operations with standard I/O consumers and trait-object callers.
//! The stream position and read-ahead must still agree when control changes hands.

use std::io::{BufRead, Cursor, Read, Seek, SeekFrom};
use strata_reader::{DynamicRead, DynamicReadExt, Reader};

/// Revisit a buffered header, then use an ordinary seek and Read consumer for its body.
#[test]
fn test_standard_io_and_relative_seek_respect_the_logical_position() {
    // Filling reads ahead to the end, but the logical position follows consumption
    let mut reader = Reader::new(Cursor::new(b"header:payload"));
    reader.fill_until(b':').unwrap();
    reader.consume(7);
    let position = reader.stream_position().unwrap();
    let inner_position = reader.get_ref().position();

    assert_eq!(position, 7);
    assert_eq!(inner_position, 14);

    // Revisit the header within retained data without moving the underlying cursor
    reader.seek_relative(-7).unwrap();

    assert_eq!(reader.get_ref().position(), inner_position);
    assert_eq!(reader.peek(7), b"header:");

    let mut header = [0; 7];
    reader.read_exact(&mut header).unwrap();

    assert_eq!(&header, b"header:");
    assert_eq!(reader.peek_behind(7), &header);

    // An ordinary seek is relative to the logical position and invalidates retained bytes
    let position = reader.seek(SeekFrom::Current(2)).unwrap();

    assert_eq!(position, 9);
    assert!(reader.buffer().is_empty());

    let mut rest = String::new();
    reader.read_to_string(&mut rest).unwrap();

    assert_eq!(rest, "yload");
}

/// Call the generic predicate wrapper through a trait object after consuming an earlier record.
#[test]
fn test_dynamic_trait_object_checks_existing_unconsumed_data() {
    // The generic caller receives only the trait object, after the first line was consumed
    let mut reader = Reader::new(Cursor::new(b"old\nnew\n"));
    reader.fill_to_end().unwrap();
    reader.consume(4);
    let dynamic: &mut dyn DynamicRead = &mut reader;
    let mut seen = Vec::new();

    let len = dynamic
        .fill_while(|bytes| {
            seen.extend_from_slice(bytes);
            !bytes.contains(&b'\n')
        })
        .unwrap();

    // The existing second line satisfies the predicate without reading or exposing lookbehind
    assert_eq!(len, 0);
    assert_eq!(seen, b"new\n");
    assert_eq!(dynamic.pos(), 4);
    assert_eq!(dynamic.buffer(), b"old\nnew\n");
}

/// Pass a reader with consumed lookbehind and unread read-ahead to standard library consumers.
#[test]
fn test_standard_consumers_after_retained_reads() {
    let mut reader = Reader::new(Cursor::new("skip\nfirst\n\n界\nlast"));
    reader.fill_until(b'\n').unwrap();
    reader.consume(5);

    // lines() must begin at the logical position, including empty and unterminated final lines
    let lines = (&mut reader)
        .lines()
        .collect::<std::io::Result<Vec<_>>>()
        .unwrap();

    assert_eq!(lines, ["first", "", "界", "last"]);
    assert!(reader.peek(1).is_empty());

    // Try a bulk consumer with both read-ahead and input that still resides in the source
    let data = "abcde".repeat(10_000);
    let mut reader = Reader::new(Cursor::new(data.as_bytes()));
    reader.fill().unwrap();
    reader.consume(7);
    let mut output = Vec::new();
    let len = std::io::copy(&mut reader, &mut output).unwrap();

    assert_eq!(len, u64::try_from(data.len() - 7).unwrap());
    assert_eq!(output, &data.as_bytes()[7..]);
}
