//! Inspect frames with a two-byte big-endian length followed by that many payload bytes.
//! `fill_amount` can read several frames at once; this example checks how much is already
//! buffered before requesting more. Payloads are bytes, so zero, newline, and invalid UTF-8
//! have no special meaning. Empty payloads are valid.
//!
//! Run: `cargo run --example binary_frames`.
//! Output: `3 bytes: 00 ff 0a` followed by `0 bytes:` on the next line.
//! A frame, including its length field, must fit in this example's 8 KiB retention window.
//! EOF between frames is accepted; a partial length or payload is an error. Processing stops
//! on errors without rolling back input or output. This is a small invented format, not a
//! network protocol implementation.

use std::io::{self, BufRead, Cursor, Read, Write};
use strata_reader::{DynamicRead, Reader, constants::CHUNK_SIZE};

fn print_frames(input: impl Read, mut output: impl Write) -> io::Result<()> {
    let mut reader = Reader::builder(input).max_capacity(CHUNK_SIZE).build();
    loop {
        if reader.buffer().is_empty() {
            reader.fill_amount(1)?;
        }
        if reader.buffer().is_empty() {
            return Ok(());
        }

        // Only request missing bytes: fill amounts are additional, not a target buffer length
        let missing = 2usize.saturating_sub(reader.buffer().len());
        reader.fill_amount(missing)?;
        let [high, low] = reader.peek(2) else {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "incomplete frame length",
            ));
        };
        let payload_len = usize::from(u16::from_be_bytes([*high, *low]));
        let frame_len = payload_len.saturating_add(2);
        if frame_len > reader.max_capacity() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "frame exceeds the buffer limit",
            ));
        }
        let missing = frame_len.saturating_sub(reader.buffer().len());
        reader.fill_amount(missing)?;
        if reader.buffer().len() < frame_len {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "incomplete frame payload",
            ));
        }

        // Leave read-ahead for later frames untouched while processing this payload
        reader.consume(2);
        write!(output, "{payload_len} bytes:")?;
        for byte in reader.peek(payload_len) {
            write!(output, " {byte:02x}")?;
        }
        writeln!(output)?;
        reader.consume(payload_len);
        reader.compact();
    }
}

fn main() -> io::Result<()> {
    print_frames(
        Cursor::new(b"\x00\x03\x00\xff\n\x00\x00"),
        io::stdout().lock(),
    )
}

#[cfg(test)]
#[path = "binary_frames/tests.rs"]
mod tests;
