//! Consumer scenarios for retained streaming input, using only the public API.

#![expect(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    reason = "Small, bounded fixtures; invalid offsets should fail the test"
)]

use std::io::{self, BufRead, Cursor, Read, Seek, SeekFrom};
use strata_reader::{DynamicRead, DynamicReadExt, Reader, constants::CHUNK_SIZE};

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

#[test]
fn test_records_cross_reads_and_transfer_consumed_data() -> io::Result<()> {
    let data = b"first\r\nsecond\r\nlast";
    let source = ShortReads {
        inner: Cursor::new(data),
        limit: 2,
    };
    let mut reader = Reader::new(source);

    // CR and LF arrive separately. Filling may also read beyond the delimiter.
    reader.fill_until_str("\r\n")?;
    assert_eq!(reader.peek(7), b"first\r\n");
    reader.consume(7);
    assert_eq!(reader.peek_behind(7), b"first\r\n");
    let first = reader.take_consumed();
    assert_eq!(first.buf(), b"first\r\n");
    assert_eq!(first.pos(), first.len());
    assert_eq!(reader.pos(), 0);

    reader.fill_until_str("\r\n")?;
    assert_eq!(reader.peek(8), b"second\r\n");
    reader.consume(8);
    reader.compact();
    assert!(reader.peek_behind(1).is_empty());

    // A final unterminated record is still available when filling reaches EOF.
    reader.fill_until_str("\r\n")?;
    assert_eq!(reader.peek(usize::MAX), b"last");
    assert_eq!(reader.fill_until_str("\r\n")?, 0);
    drop(reader);
    assert_eq!(first.buf(), b"first\r\n");
    Ok(())
}

#[test]
fn test_utf8_delimiters_cross_reads_and_invalid_text_is_reported() -> io::Result<()> {
    let source = ShortReads {
        inner: Cursor::new("a界b終端tail"),
        limit: 1,
    };
    let mut reader = Reader::new(source);
    reader.fill_until_char('界')?;
    assert_eq!(reader.peek(usize::MAX), "a界".as_bytes());
    reader.consume("a界".len());
    reader.fill_until_str("終端")?;
    assert_eq!(reader.peek(usize::MAX), "b終端".as_bytes());
    assert_eq!(reader.peek_behind(usize::MAX), "a界".as_bytes());

    let mut invalid = Reader::new(Cursor::new(b"a\xffend"));
    assert_eq!(
        invalid.fill_until_str("end").unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
    // Byte searches remain usable for binary input already retained by a failed text search.
    assert_eq!(invalid.fill_until(b'd')?, 0);
    assert_eq!(invalid.peek(usize::MAX), b"a\xffend");
    Ok(())
}

#[test]
fn test_capacity_stop_is_not_eof_and_consumption_alone_does_not_free_space() -> io::Result<()> {
    let mut data = vec![b'x'; CHUNK_SIZE];
    data.extend_from_slice(b"\nnext");
    let mut reader = Reader::builder(Cursor::new(&data))
        .max_capacity(CHUNK_SIZE)
        .build();

    assert_eq!(reader.fill_until(b'\n')?, CHUNK_SIZE);
    assert!(!reader.buffer().contains(&b'\n'));
    assert_eq!(reader.fill_until(b'\n')?, 0);
    reader.consume(CHUNK_SIZE);
    // Dynamic fills preserve consumed bytes, so even an exhausted buffer can be full.
    assert_eq!(reader.fill_until(b'\n')?, 0);
    assert_eq!(reader.peek_behind(CHUNK_SIZE), &data[..CHUNK_SIZE]);
    reader.compact();
    assert_eq!(reader.capacity(), CHUNK_SIZE);
    assert_eq!(reader.fill_until(b'\n')?, 5);
    assert_eq!(reader.peek(usize::MAX), b"\nnext");
    Ok(())
}

/// A short prefix, an interrupt, a recoverable error, then the remainder.
struct PausedSource {
    step: usize,
}

impl Read for PausedSource {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        self.step += 1;
        match self.step {
            1 => b"prefix".as_slice().read(output),
            2 => Err(io::ErrorKind::Interrupted.into()),
            3 => Err(io::ErrorKind::WouldBlock.into()),
            4 => b"\nrest".as_slice().read(output),
            _ => Ok(0),
        }
    }
}

#[test]
fn test_partial_progress_survives_error_and_retry() -> io::Result<()> {
    let mut reader = Reader::new(PausedSource { step: 0 });
    assert_eq!(
        reader.fill_until(b'\n').unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
    // Interrupted was retried, but WouldBlock returned control with the prefix retained.
    assert_eq!(reader.get_ref().step, 3);
    assert_eq!(reader.buffer(), b"prefix");
    assert_eq!(reader.pos(), 0);
    assert_eq!(reader.fill_until(b'\n')?, 5);
    assert_eq!(reader.peek(7), b"prefix\n");
    reader.consume(7);
    reader.compact();
    let mut rest = String::new();
    reader.read_to_string(&mut rest)?;
    assert_eq!(rest, "rest");
    Ok(())
}

#[test]
fn test_standard_io_and_relative_seek_respect_the_logical_position() -> io::Result<()> {
    let mut reader = Reader::new(Cursor::new(b"header:payload"));
    reader.fill_until(b':')?;
    reader.consume(7);
    assert_eq!(reader.stream_position()?, 7);
    let inner_position = reader.get_ref().position();
    reader.seek_relative(-7)?;
    assert_eq!(reader.get_ref().position(), inner_position);
    assert_eq!(reader.peek(7), b"header:");

    let mut header = [0; 7];
    reader.read_exact(&mut header)?;
    assert_eq!(&header, b"header:");
    assert_eq!(reader.peek_behind(7), &header);
    reader.seek(SeekFrom::Current(2))?;
    assert!(reader.buffer().is_empty());
    assert_eq!(reader.stream_position()?, 9);
    let mut rest = String::new();
    reader.read_to_string(&mut rest)?;
    assert_eq!(rest, "yload");
    Ok(())
}

#[test]
fn test_dynamic_trait_object_checks_existing_unconsumed_data() -> io::Result<()> {
    let mut reader = Reader::new(Cursor::new(b"old\nnew\n"));
    reader.fill_to_end()?;
    reader.consume(4);
    let dynamic: &mut dyn DynamicRead = &mut reader;
    let mut seen = Vec::new();
    assert_eq!(
        dynamic.fill_while(|bytes| {
            seen.extend_from_slice(bytes);
            !bytes.contains(&b'\n')
        })?,
        0
    );
    assert_eq!(seen, b"new\n");
    assert_eq!(dynamic.pos(), 4);
    assert_eq!(dynamic.buffer(), b"old\nnew\n");
    Ok(())
}

#[test]
fn test_growth_compaction_and_explicit_reclamation() -> io::Result<()> {
    let data = vec![b'x'; 3 * CHUNK_SIZE];
    let mut reader = Reader::new(Cursor::new(&data));
    assert_eq!(reader.fill()?, CHUNK_SIZE);
    assert_eq!(reader.fill()?, 0); // A single fill never grows a full buffer.
    assert_eq!(reader.capacity(), CHUNK_SIZE);
    assert_eq!(reader.fill_to_end()?, 2 * CHUNK_SIZE);
    // Growth reached four chunks, then the completed fill released its excess chunk.
    assert_eq!(reader.capacity(), 3 * CHUNK_SIZE);
    reader.consume(2 * CHUNK_SIZE);
    reader.compact();
    assert_eq!(reader.buffer(), &data[..CHUNK_SIZE]);
    assert_eq!(reader.capacity(), 3 * CHUNK_SIZE);
    reader.shrink();
    assert_eq!(reader.capacity(), CHUNK_SIZE);
    reader.clear();
    assert!(reader.buffer().is_empty());
    assert_eq!(reader.capacity(), CHUNK_SIZE);
    reader.discard();
    assert_eq!(reader.pos(), 0);
    assert_eq!(reader.capacity(), CHUNK_SIZE);
    Ok(())
}

#[test]
fn test_handoff_preserves_read_ahead_and_the_remaining_source() -> io::Result<()> {
    let source = ShortReads {
        inner: Cursor::new(b"headtailrest"),
        limit: 8,
    };
    let mut reader = Reader::new(source);
    reader.fill()?;
    reader.consume(4);
    let saved = reader.take_buffer();
    assert_eq!(saved.buf(), b"headtail");
    assert_eq!(saved.pos(), 4);
    assert!(reader.buffer().is_empty());
    reader.fill()?;
    let (mut source, buffered) = reader.into_parts();
    let mut remaining = saved.buf()[saved.pos()..].to_vec();
    remaining.extend_from_slice(&buffered.buf()[buffered.pos()..]);
    source.read_to_end(&mut remaining)?;
    assert_eq!(remaining, b"tailrest");
    Ok(())
}

#[test]
fn test_exact_fill_error_can_advance_the_source_without_exposing_new_bytes() -> io::Result<()> {
    let mut reader = Reader::new(Cursor::new(b"header:short"));
    reader.fill_exact(7)?;
    reader.consume(7);
    assert_eq!(
        reader.fill_exact(10).unwrap_err().kind(),
        io::ErrorKind::UnexpectedEof
    );
    assert_eq!(reader.get_ref().position(), 12);
    assert_eq!(reader.peek_behind(7), b"header:");
    assert!(reader.peek(usize::MAX).is_empty());
    // Retrying cannot recover bytes the source supplied to the failed exact read.
    assert_eq!(reader.fill_to_end()?, 0);
    Ok(())
}

#[test]
fn test_additional_fills_and_capacity_alignment() -> io::Result<()> {
    let mut reader = Reader::builder(Cursor::new(b"abcdefghijkl"))
        .initial_capacity(2 * CHUNK_SIZE + 1)
        .max_capacity(2 * CHUNK_SIZE + 1)
        .build();
    assert_eq!(reader.capacity(), 3 * CHUNK_SIZE);
    assert_eq!(reader.max_capacity(), 4 * CHUNK_SIZE);
    reader.fill_exact(3)?;
    reader.consume(2);
    // The next request is additional input, independent of both retained and unread lengths.
    assert_eq!(reader.fill_amount(4)?, 9);
    assert_eq!(reader.peek(usize::MAX), b"cdefghijkl");
    assert_eq!(reader.peek_behind(2), b"ab");
    assert_eq!(reader.capacity(), 3 * CHUNK_SIZE);
    reader.clear();
    assert!(reader.buffer().is_empty());
    assert_eq!(reader.capacity(), 3 * CHUNK_SIZE);
    reader.discard();
    assert_eq!(reader.capacity(), CHUNK_SIZE);
    Ok(())
}

#[test]
fn test_non_exact_fills_retain_partial_progress() {
    type Fill = fn(&mut Reader<PausedSource>) -> io::Result<usize>;
    let fills: [(&str, Fill); 6] = [
        ("amount", |reader| reader.fill_amount(7)),
        ("to_end", Reader::fill_to_end),
        ("while", |reader| {
            reader.fill_while(|bytes| !bytes.contains(&b'\n'))
        }),
        ("byte", |reader| reader.fill_until(b'\n')),
        ("char", |reader| reader.fill_until_char('\n')),
        ("str", |reader| reader.fill_until_str("\n")),
    ];
    for (name, fill) in fills {
        let mut reader = Reader::new(PausedSource { step: 0 });
        assert_eq!(
            fill(&mut reader).unwrap_err().kind(),
            io::ErrorKind::WouldBlock,
            "{name}"
        );
        assert_eq!(reader.buffer(), b"prefix", "{name}");
        assert_eq!(reader.pos(), 0, "{name}");
    }
}

#[test]
fn test_long_record_grows_across_buffer_and_delimiter_boundaries() -> io::Result<()> {
    let mut data = vec![b'x'; CHUNK_SIZE - 1];
    data.extend_from_slice(b"\r\ntail");
    let mut reader = Reader::builder(Cursor::new(&data))
        .max_capacity(2 * CHUNK_SIZE)
        .build();
    reader.fill_until_str("\r\n")?;
    let record_len = CHUNK_SIZE + 1;
    assert_eq!(reader.peek(record_len), &data[..record_len]);
    assert_eq!(reader.capacity(), 2 * CHUNK_SIZE);
    reader.consume(record_len);
    let record = reader.take_consumed();
    assert_eq!(record.buf(), &data[..record_len]);
    assert_eq!(reader.peek(usize::MAX), b"tail");
    assert_eq!(reader.capacity(), CHUNK_SIZE);
    assert_eq!(reader.fill_to_end()?, 0);
    Ok(())
}

struct ReadError;

impl Read for ReadError {
    fn read(&mut self, _output: &mut [u8]) -> io::Result<usize> {
        Err(io::ErrorKind::Other.into())
    }
}

#[test]
fn test_growth_before_error_is_retained_until_explicit_reclamation() {
    let prefix_len = 3 * CHUNK_SIZE;
    let source = io::repeat(b'x')
        .take(u64::try_from(prefix_len).unwrap())
        .chain(ReadError);
    let mut reader = Reader::new(source);
    assert_eq!(
        reader.fill_to_end().unwrap_err().kind(),
        io::ErrorKind::Other
    );
    assert_eq!(reader.buffer(), vec![b'x'; prefix_len]);
    assert_eq!(reader.capacity(), 4 * CHUNK_SIZE);
    reader.shrink();
    assert_eq!(reader.capacity(), prefix_len);
    assert_eq!(reader.buffer().len(), prefix_len);
}
