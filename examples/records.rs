//! Inspect LF-terminated byte records while keeping each record available after consumption.
//! `fill_until` can grow across reads and retain read-ahead for the next record. `peek_behind`
//! lets the processing step use the consumed record before `compact` releases that lookbehind.
//!
//! Run: `cargo run --example records -- [file]` (omit the optional file for built-in input).
//! The built-in input prints `12 bytes: name=Strata` and `15 bytes: mode=read-only` on two lines.
//! Records may contain arbitrary bytes, must end in LF, and must fit in 32 KiB including LF.
//! These are this program's format choices. The library retries interrupted reads; other I/O
//! errors stop processing, and output already written remains written. For ordinary text lines
//! without retention, `BufRead::lines` is a simpler starting point.

use std::io::{self, BufRead, Cursor, Read, Write};
use strata_reader::{DynamicRead, Reader, constants::CHUNK_SIZE};

const RECORD_CAPACITY: usize = 4 * CHUNK_SIZE;

fn print_records(input: impl Read, mut output: impl Write) -> io::Result<()> {
    let mut reader = Reader::builder(input).max_capacity(RECORD_CAPACITY).build();

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
