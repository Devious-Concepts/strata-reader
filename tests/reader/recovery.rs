//! Continue after capacity stops and recoverable read errors, and inspect exact-fill failure.
//! These use controlled sources so each assertion can account for which bytes reached the reader.

use std::io::{self, BufRead, Cursor, Read};
use strata_reader::{DynamicRead, DynamicReadExt, Reader, constants::CHUNK_SIZE};

/// Pause once after a prefix, then resume. Slices remember progress across short output buffers.
struct PausedSource {
    prefix: &'static [u8],
    suffix: &'static [u8],
    step: usize,
}

impl PausedSource {
    fn new() -> Self {
        Self {
            prefix: b"prefix",
            suffix: b"\nrest",
            step: 0,
        }
    }
}

impl Read for PausedSource {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if output.is_empty() {
            return Ok(0);
        }
        if !self.prefix.is_empty() {
            return self.prefix.read(output);
        }
        self.step += 1;
        match self.step {
            1 => Err(io::ErrorKind::Interrupted.into()),
            2 => Err(io::ErrorKind::WouldBlock.into()),
            _ => self.suffix.read(output),
        }
    }
}

struct ReadError;

impl Read for ReadError {
    fn read(&mut self, _output: &mut [u8]) -> io::Result<usize> {
        Err(io::ErrorKind::Other.into())
    }
}
/// Free a full retained window explicitly, then continue reading from the same source.
#[test]
fn test_capacity_stop_is_not_eof_and_consumption_alone_does_not_free_space() {
    let mut data = vec![b'x'; CHUNK_SIZE];
    data.extend_from_slice(b"\nnext");
    let mut reader = Reader::builder(Cursor::new(&data))
        .max_capacity(CHUNK_SIZE)
        .build();

    let len = reader.fill_until(b'\n').unwrap();

    // Check the number of new bytes, separately from the retained buffer below
    assert_eq!(len, CHUNK_SIZE);
    assert!(!reader.buffer().contains(&b'\n'));
    let len = reader.fill_until(b'\n').unwrap();

    // Check the number of new bytes, separately from the retained buffer below
    assert_eq!(len, 0);
    reader.consume(CHUNK_SIZE);
    // Dynamic fills preserve consumed bytes, so even an exhausted buffer can be full.
    let len = reader.fill_until(b'\n').unwrap();

    // Check the number of new bytes, separately from the retained buffer below
    assert_eq!(len, 0);
    assert_eq!(reader.peek_behind(CHUNK_SIZE), &data[..CHUNK_SIZE]);
    reader.compact();
    assert_eq!(reader.capacity(), CHUNK_SIZE);
    let len = reader.fill_until(b'\n').unwrap();

    // Check the number of new bytes, separately from the retained buffer below
    assert_eq!(len, 5);
    assert_eq!(reader.peek(usize::MAX), b"\nnext");
}

/// Retry after `WouldBlock` and check that the previously read prefix is neither lost nor repeated.
#[test]
fn test_partial_progress_survives_error_and_retry() {
    let mut reader = Reader::new(PausedSource::new());
    assert_eq!(
        reader.fill_until(b'\n').unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
    // Interrupted was retried, but WouldBlock returned control with the prefix retained.
    assert_eq!(reader.get_ref().step, 2);
    assert_eq!(reader.buffer(), b"prefix");
    assert_eq!(reader.pos(), 0);
    let len = reader.fill_until(b'\n').unwrap();

    // Check the number of new bytes, separately from the retained buffer below
    assert_eq!(len, 5);
    assert_eq!(reader.peek(7), b"prefix\n");
    reader.consume(7);
    reader.compact();
    let mut rest = String::new();
    reader.read_to_string(&mut rest).unwrap();
    assert_eq!(rest, "rest");
}

/// Show why a failed exact fill cannot be retried as though no input had been consumed.
#[test]
fn test_exact_fill_error_can_advance_the_source_without_exposing_new_bytes() {
    let mut reader = Reader::new(Cursor::new(b"header:short"));
    reader.fill_exact(7).unwrap();
    reader.consume(7);
    assert_eq!(
        reader.fill_exact(10).unwrap_err().kind(),
        io::ErrorKind::UnexpectedEof
    );
    assert_eq!(reader.get_ref().position(), 12);
    assert_eq!(reader.peek_behind(7), b"header:");
    assert!(reader.peek(usize::MAX).is_empty());
    // Retrying cannot recover bytes the source supplied to the failed exact read.
    let len = reader.fill_to_end().unwrap();

    // Check the number of new bytes, separately from the retained buffer below
    assert_eq!(len, 0);
}

/// Check the same interrupted prefix across the non-exact fill entry points.
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
        let mut reader = Reader::new(PausedSource::new());
        assert_eq!(
            fill(&mut reader).unwrap_err().kind(),
            io::ErrorKind::WouldBlock,
            "{name}"
        );
        assert_eq!(reader.buffer(), b"prefix", "{name}");
        assert_eq!(reader.pos(), 0, "{name}");
    }
}

/// Inspect the retained bytes and capacity after growth followed by an I/O error.
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
