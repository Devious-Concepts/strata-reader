//! Check the example's header/body split, including body read-ahead and a body larger than the cap.

#![expect(clippy::unwrap_used, reason = "Okay in tests")]

use super::*;

/// A source that reports a read error after an earlier chained source has been exhausted.
struct ReadError;

impl Read for ReadError {
    fn read(&mut self, _output: &mut [u8]) -> io::Result<usize> {
        Err(io::ErrorKind::WouldBlock.into())
    }
}

#[test]
fn test_copy_body() {
    // Both header and body fit in the initial read; the body must not be skipped during transfer
    let mut output = Vec::new();
    let header = copy_body(b"kind=text\nhello".as_slice(), &mut output).unwrap();

    assert_eq!(header.buf(), b"kind=text\n");
    assert_eq!(header.pos(), header.len());
    assert_eq!(output, b"hello");

    // A body larger than the header's buffer limit continues through standard I/O
    let body = "abcdefgh".repeat(10_000);
    let input = b"kind=text\n".as_slice().chain(body.as_bytes());
    let mut output = Vec::new();
    let header = copy_body(input, &mut output).unwrap();

    assert_eq!(header.buf(), b"kind=text\n");
    assert_eq!(output, body.as_bytes());
}

#[test]
fn test_copy_body_empty_and_missing_header() {
    // An empty header and empty body are valid under this example's format
    let mut output = Vec::new();
    let header = copy_body(b"\n".as_slice(), &mut output).unwrap();

    assert_eq!(header.buf(), b"\n");
    assert!(output.is_empty());

    // EOF and the header capacity limit both reject input with no header newline
    for input in [Vec::new(), b"unfinished".to_vec(), vec![b'x'; CHUNK_SIZE]] {
        let mut output = Vec::new();
        let error = copy_body(input.as_slice(), &mut output).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(output.is_empty());
    }
}

#[test]
fn test_copy_body_io_errors() {
    // A read failure does not roll back body bytes already copied to the output
    let input = b"kind=text\npartial".as_slice().chain(ReadError);
    let mut output = Vec::new();
    let error = copy_body(input, &mut output).unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
    assert_eq!(output, b"partial");

    // A destination with no space reports its write failure
    let mut full_output = [0; 0];
    let error = copy_body(b"kind=text\nx".as_slice(), &mut full_output.as_mut_slice()).unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::WriteZero);
}
