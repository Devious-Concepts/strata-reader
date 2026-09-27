use super::*;

#[test]
fn test_output_after_reader_is_dropped() -> io::Result<()> {
    let mut output = Vec::new();
    demonstrate(&mut output)?;
    assert_eq!(output, b"body: hello\nsaved header: kind=text\n");
    Ok(())
}
