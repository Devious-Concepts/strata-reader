//! Check the invented frame format, especially read-ahead and truncated input.

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
fn test_print_frames() {
    // These frames arrive together; the newline in the first payload is just another byte
    let mut output = Vec::new();
    print_frames(b"\x00\x03\x00\xff\n\x00\x00".as_slice(), &mut output).unwrap();

    assert_eq!(output, b"3 bytes: 00 ff 0a\n0 bytes:\n");

    // The length field can also be split between reads
    let input = b"\x00".as_slice().chain(b"\x01x".as_slice());
    let mut output = Vec::new();
    print_frames(input, &mut output).unwrap();

    assert_eq!(output, b"1 bytes: 78\n");

    // No frames is different from an empty frame: it produces no output
    let mut output = Vec::new();
    print_frames(io::empty(), &mut output).unwrap();

    assert!(output.is_empty());
}

#[test]
fn test_print_frames_truncated_input() {
    for input in [b"\x00".as_slice(), b"\x00\x03ab"] {
        let mut output = Vec::new();
        let error = print_frames(input, &mut output).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
        assert!(output.is_empty()); // No partial frame was printed
    }

    // An earlier complete frame stays written when the following frame is incomplete
    let mut output = Vec::new();
    let error = print_frames(b"\x00\x01x\x00".as_slice(), &mut output).unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
    assert_eq!(output, b"1 bytes: 78\n");
}

#[test]
fn test_print_frames_capacity_limit() {
    // Reject the length before trying to read its unavailable payload
    let error = print_frames(b"\xff\xff".as_slice(), io::sink()).unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn test_print_frames_at_capacity_and_across_refills() {
    // An 8190-byte payload plus its two-byte length exactly fills the retention window
    let mut frame = b"\x1f\xfe".to_vec();
    frame.extend(vec![0; 8190]);
    let input = frame.repeat(3);
    let mut output = Vec::new();
    print_frames(input.as_slice(), &mut output).unwrap();

    let expected = format!("8190 bytes:{}\n", " 00".repeat(8190)).repeat(3);
    assert_eq!(output, expected.as_bytes());
}

#[test]
fn test_print_frames_io_errors() {
    // The first frame is printed before a read error interrupts the next payload
    let input = b"\x00\x01x\x00\x03a".as_slice().chain(ReadError);
    let mut output = Vec::new();
    let error = print_frames(input, &mut output).unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
    assert_eq!(output, b"1 bytes: 78\n");

    // Output errors are returned to the caller as well
    let mut full_output = [0; 0];
    let error = print_frames(b"\x00\x00".as_slice(), full_output.as_mut_slice()).unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::WriteZero);
}
