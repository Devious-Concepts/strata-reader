//! Check the example's output and record policy, not just whether it compiles.

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
fn test_sample_output() {
    // The built-in sample contains two complete records in one read
    let mut output = Vec::new();
    print_records(b"name=Strata\nmode=read-only\n".as_slice(), &mut output).unwrap();

    assert_eq!(output, b"12 bytes: name=Strata\n15 bytes: mode=read-only\n");
}

#[test]
fn test_empty_input_and_empty_record() {
    // EOF without data produces no output; a lone LF is a complete empty record
    let mut output = Vec::new();
    print_records(io::empty(), &mut output).unwrap();

    assert!(output.is_empty());
    print_records(b"\n".as_slice(), &mut output).unwrap();

    assert_eq!(output, b"1 bytes: \n");
}

#[test]
fn test_unterminated_record_is_rejected_after_complete_records() {
    // Keep output from complete records, but do not print the incomplete tail
    let mut output = Vec::new();
    let error = print_records(b"ok\nunfinished".as_slice(), &mut output).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert_eq!(error.to_string(), "final record has no terminating newline");
    assert_eq!(output, b"3 bytes: ok\n");
}

#[test]
fn test_limit_includes_the_newline() {
    // A record can use the entire window only when its last byte is the delimiter
    let mut input = vec![b'x'; RECORD_CAPACITY];
    let error = print_records(input.as_slice(), io::sink()).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert_eq!(
        error.to_string(),
        "record reached the buffer limit without a newline"
    );
    input.pop();
    input.push(b'\n');
    print_records(input.as_slice(), io::sink()).unwrap();
}

#[test]
fn test_short_reads_and_output_errors() {
    // Chain supplies the newline on a later read, without a custom source implementation.
    let input = b"split".as_slice().chain(b"\n".as_slice());
    let mut output = Vec::new();
    print_records(input, &mut output).unwrap();

    assert_eq!(output, b"6 bytes: split\n");

    let mut full_output = [0; 0];
    let error = print_records(b"ok\n".as_slice(), full_output.as_mut_slice()).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::WriteZero);
}

#[test]
fn test_records_across_refills() {
    // These records exceed the initial capacity and together exceed the maximum capacity
    let record = format!("{}\n", "x".repeat(CHUNK_SIZE));
    let input = record.repeat(6);
    let mut output = Vec::new();
    print_records(input.as_bytes(), &mut output).unwrap();

    let expected = format!("8193 bytes: {record}").repeat(6);
    assert_eq!(output, expected.as_bytes());

    // Processing is byte-based; neither NUL nor invalid UTF-8 changes where a record ends
    let mut output = Vec::new();
    print_records(b"\x00\xff\n".as_slice(), &mut output).unwrap();

    assert_eq!(output, b"3 bytes: \x00\xff\n");
}

#[test]
fn test_records_read_error() {
    // Finish one record, then fail while trying to complete the next one
    let input = b"ok\npartial".as_slice().chain(ReadError);
    let mut output = Vec::new();
    let error = print_records(input, &mut output).unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
    assert_eq!(output, b"3 bytes: ok\n");
}
