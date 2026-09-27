//! Keep a header after handing its body to an ordinary Read consumer.
//! `take_consumed` returns the consumed header as an owned Buffer and leaves any body read-ahead
//! in the reader. `io::copy` can then stream the body without retaining it alongside the header.
//!
//! Run: `cargo run --example retained_prefix`.
//! Output: `body: hello` followed by `saved header: kind=text` on the next line.
//! This example's header is one LF-terminated byte line, limited to 8 KiB including LF. The body
//! has no length limit here. Errors stop processing; already-written body bytes are not rolled
//! back. Transfer allocates a replacement buffer and copies unread read-ahead into it.

use std::io::{self, BufRead, Cursor, Read, Write};
use strata_reader::{Reader, buffer::Buffer, constants::CHUNK_SIZE};

fn copy_body(input: impl Read, output: &mut impl Write) -> io::Result<Buffer> {
    let mut reader = Reader::builder(input).max_capacity(CHUNK_SIZE).build();
    reader.fill_until(b'\n')?;
    let newline = reader
        .peek(usize::MAX)
        .iter()
        .position(|&byte| byte == b'\n')
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "missing header newline within 8 KiB",
            )
        })?;
    reader.consume(newline.saturating_add(1));

    // Save the header before a standard I/O consumer starts reusing the reader's buffer
    let header = reader.take_consumed();
    io::copy(&mut reader, output)?;
    Ok(header)
}

fn main() -> io::Result<()> {
    let mut output = io::stdout().lock();
    write!(output, "body: ")?;
    let header = copy_body(Cursor::new(b"kind=text\nhello"), &mut output)?;

    // copy_body's reader has been dropped; the returned header owns its bytes
    write!(output, "\nsaved header: ")?;
    output.write_all(header.buf())
}

#[cfg(test)]
#[path = "retained_prefix/tests.rs"]
mod tests;
