use crate::DynamicRead;
use crate::constants::DEFAULT_MAX_SIZE;
use crate::buffer::Buffer;
use std::io::{self, BufRead, Read};

pub struct Reader<R: ?Sized> {
    buffer: Buffer,
    max_capacity: usize,
    reader: R,
}

impl<R: Read> Reader<R> {
    pub fn new(reader: R) -> Reader<R> {
        Reader::with_max_capacity(DEFAULT_MAX_SIZE, reader)
    }

    pub fn with_max_capacity(max_capacity: usize, reader: R) -> Reader<R> {
        Reader {
            buffer: Buffer::new(),
            max_capacity: Buffer::cap_up(max_capacity),
            reader,
        }
    }
}

impl<R: ?Sized> Reader<R> {
    // TODO: stuff

    pub fn grow(&mut self) {
        if self.buffer.cap() < self.max_capacity {
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
            // We've consumed all the data we have

            // Cap the fill amount to respect max_size
            let max_allowed = self.max_capacity.saturating_sub(self.buffer.len());
            let capped_amt = buf.len().min(max_allowed);

            // Read at least the requested amount of data (up to max_size)
            let _ = self.buffer.fill_amount(&mut self.reader, capped_amt)?;
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

impl<R: Read + ?Sized> BufRead for Reader<R> {
    #[expect(clippy::indexing_slicing, reason = "Buffer invariant makes it safe")]
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.buffer.pos() >= self.buffer.len() {
            debug_assert!(self.buffer.pos() == self.buffer.len());
            // We've consumed all the data we have

            if self.buffer.len() >= self.buffer.cap() {
                debug_assert!(self.buffer.len() == self.buffer.cap());
                // The buffer is full

                // Try to grow the buffer
                self.grow();
            }

            // Read to fill the internal buffer
            let _ = self.buffer.fill(&mut self.reader)?;
        }

        // Return buffered data
        Ok(&self.buffer.buf()[self.buffer.pos()..])
    }

    fn consume(&mut self, amt: usize) {
        self.consume(amt);
    }
}

impl<R: Read + ?Sized> DynamicRead for Reader<R> {
    // TODO
}

#[cfg(test)]
mod tests {
    use super::*;
}
