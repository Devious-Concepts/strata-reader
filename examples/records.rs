//! Print LF-terminated records, retaining each record until it has been processed.
//! Run with no arguments for sample input, or pass a file path.

use std::io::{self, BufRead, Cursor, Read, Write};
use strata_reader::{DynamicRead, Reader, constants::CHUNK_SIZE};

fn print_records(input: impl Read, mut output: impl Write) -> io::Result<()> {
    let mut reader = Reader::builder(input).max_capacity(CHUNK_SIZE).build();

    loop {
        reader.fill_until(b'\n')?;
        let available = reader.peek(usize::MAX);
        if available.is_empty() {
            return Ok(());
        }

        // A byte count alone cannot distinguish a found delimiter, EOF, or a capacity stop.
        let Some(newline) = available.iter().position(|&byte| byte == b'\n') else {
            let message = if reader.buffer().len() == reader.max_capacity() {
                "record reached the buffer limit without a newline"
            } else {
                "final record has no terminating newline"
            };
            return Err(io::Error::new(io::ErrorKind::InvalidData, message));
        };
        let record_len = newline.saturating_add(1);
        reader.consume(record_len);

        // Consumed bytes remain available until we explicitly reclaim them.
        let record = reader.peek_behind(record_len);
        write!(output, "{} bytes: ", record.len())?;
        output.write_all(record)?;
        reader.compact();
    }
}

fn main() -> io::Result<()> {
    let input: Box<dyn Read> = match std::env::args_os().nth(1) {
        Some(path) => Box::new(std::fs::File::open(path)?),
        None => Box::new(Cursor::new(b"name=Strata\nmode=read-only\n")),
    };
    print_records(input, io::stdout().lock())
}

#[cfg(test)]
#[path = "records/tests.rs"]
mod tests;
