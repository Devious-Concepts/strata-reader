//! Check the example's output and record policy, not just whether it compiles.

#![expect(
    clippy::unwrap_used,
    reason = "An unexpected success or failure should fail the test"
)]

use super::*;

#[test]
fn test_sample_output() -> io::Result<()> {
    let mut output = Vec::new();
    print_records(b"name=Strata\nmode=read-only\n".as_slice(), &mut output)?;
    assert_eq!(output, b"12 bytes: name=Strata\n15 bytes: mode=read-only\n");
    Ok(())
}

#[test]
fn test_empty_input_and_empty_record() -> io::Result<()> {
    let mut output = Vec::new();
    print_records(io::empty(), &mut output)?;
    assert!(output.is_empty());
    print_records(b"\n".as_slice(), &mut output)?;
    assert_eq!(output, b"1 bytes: \n");
    Ok(())
}

#[test]
fn test_unterminated_record_is_rejected_after_complete_records() {
    let mut output = Vec::new();
    let error = print_records(b"ok\nunfinished".as_slice(), &mut output).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert_eq!(error.to_string(), "final record has no terminating newline");
    assert_eq!(output, b"3 bytes: ok\n");
}

#[test]
fn test_limit_includes_the_newline() -> io::Result<()> {
    let mut input = vec![b'x'; CHUNK_SIZE];
    let error = print_records(input.as_slice(), io::sink()).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert_eq!(
        error.to_string(),
        "record reached the buffer limit without a newline"
    );
    input.pop();
    input.push(b'\n');
    print_records(input.as_slice(), io::sink())?;
    Ok(())
}

#[test]
fn test_short_reads_and_output_errors() -> io::Result<()> {
    // Chain supplies the newline on a later read, without a custom source implementation.
    let input = b"split".as_slice().chain(b"\n".as_slice());
    let mut output = Vec::new();
    print_records(input, &mut output)?;
    assert_eq!(output, b"6 bytes: split\n");

    let mut full_output = [0; 0];
    let error = print_records(b"ok\n".as_slice(), full_output.as_mut_slice()).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::WriteZero);
    Ok(())
}
