use crate::DynamicRead;
use crate::constants::{CHUNK_SIZE, DEFAULT_MAX_SIZE, PRACTICAL_MAX_SIZE};
use crate::buffer::Buffer;
use std::cmp;
use std::io::{self, Read};

pub struct Reader<R: ?Sized> {
    buffer: Buffer,
    max_size: usize,
    reader: R,
}

impl<R: Read> Reader<R> {
    pub fn new(reader: R) -> Reader<R> {
        Reader::with_max_size(DEFAULT_MAX_SIZE, reader)
    }

    pub fn with_max_size(max_size: usize, reader: R) -> Reader<R> {
        Reader {
            buffer: Buffer::new(),
            max_size: Buffer::cap_up(max_size),
            reader,
        }
    }
}

impl<R: ?Sized> Reader<R> {
    // TODO: stuff

    pub fn grow(&mut self) {
        if self.buffer.cap() < self.max_size {
            self.buffer.grow();
        }
    }

    pub fn shrink(&mut self) {
        self.buffer.shrink();
    }

    pub fn consume(&mut self, amt: usize) {
        self.buffer.consume(amt);
    }

    pub fn discard(&mut self) {
        self.buffer.discard();
    }

    pub fn compact(&mut self) {
        self.buffer.compact();
    }
}

impl<R: Read + ?Sized> Reader<R> {
    // TODO: Peek stuff.
}

impl<R: Read + ?Sized> Read for Reader<R> {
    #[expect(clippy::indexing_slicing, reason = "Buffer invariant makes it safe")]
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.buffer.pos() >= self.buffer.len() {
            debug_assert!(self.buffer.pos() == self.buffer.len());
            // We've consumed all the data we have.

            // Read at least the requested amount of data.
            let _ = self.buffer.fill_amount(&mut self.reader, buf.len())?;
        }

        // Get the range of data to give
        let start = self.buffer.pos();
        let end = self
            .buffer
            .pos()
            .saturating_add(buf.len())
            .min(self.buffer.len());

        // Get the slice of the data
        let data = &self.buffer.buf()[start..end];

        // Get count of read bytes
        let bytes_read = data.len();

        // Copy the data into the target buffer
        buf[..bytes_read].copy_from_slice(data);

        // Mark the now read bytes as consumed
        self.consume(bytes_read);

        // Return the number of read bytes
        Ok(bytes_read)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
}
