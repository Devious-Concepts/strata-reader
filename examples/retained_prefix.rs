//! Keep a consumed header while continuing to read its body.

use std::io::{self, BufRead, Cursor, Write};
use strata_reader::{DynamicRead, Reader, constants::CHUNK_SIZE};

fn demonstrate(mut output: impl Write) -> io::Result<()> {
    let mut reader = Reader::builder(Cursor::new(b"kind=text\nhello"))
        .max_capacity(CHUNK_SIZE)
        .build();
    reader.fill_until(b'\n')?;
    let newline = reader
        .peek(usize::MAX)
        .iter()
        .position(|&byte| byte == b'\n')
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing header newline"))?;
    reader.consume(newline.saturating_add(1));

    // Keep the returned Buffer itself alive, then borrow its bytes when needed.
    // take_consumed allocates a replacement and copies any unconsumed read-ahead into it.
    let header = reader.take_consumed();
    reader.fill_to_end()?;
    let body = reader.peek(usize::MAX);
    write!(output, "body: ")?;
    output.write_all(body)?;
    writeln!(output)?;

    // Reclaim the reader's buffer; the transferred header is independently owned.
    reader.discard();
    drop(reader);
    write!(output, "saved header: ")?;
    output.write_all(header.buf())?;
    Ok(())
}

fn main() -> io::Result<()> {
    demonstrate(io::stdout().lock())
}

#[cfg(test)]
#[path = "retained_prefix/tests.rs"]
mod tests;
