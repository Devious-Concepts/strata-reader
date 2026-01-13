use crate::DynamicRead;
use crate::buffer::Buffer;
use crate::constants::DEFAULT_MAX_SIZE;
use std::io::{self, BufRead, Read};

pub struct Reader<R: ?Sized> {
    buffer: Buffer,
    max_capacity: usize,
    reader: R,
}

impl<R: Read> Reader<R> {
    /// Creates a new `Reader` with default configuration.
    ///
    /// The buffer starts at the default capacity and can grow up to [`DEFAULT_MAX_SIZE`].
    pub fn new(reader: R) -> Reader<R> {
        Reader::with_config(reader, None, None)
    }

    /// Creates a new `Reader` with custom capacity configuration.
    ///
    /// # Arguments
    ///
    /// * `reader` - The underlying reader to buffer
    /// * `initial_capacity` - Initial buffer capacity, or `None` for default
    /// * `max_capacity` - Maximum buffer capacity, or `None` for [`DEFAULT_MAX_SIZE`]
    ///
    /// Both capacities are rounded up to implementation-specific alignment boundaries.
    /// If `max_capacity` is less than `initial_capacity`, it is raised to match.
    pub fn with_config(
        reader: R,
        initial_capacity: Option<usize>,
        max_capacity: Option<usize>,
    ) -> Reader<R> {
        let buffer = match initial_capacity {
            Some(cap) => Buffer::with_capacity(cap),
            None => Buffer::new(),
        };
        let max_capacity = max_capacity
            .map_or(DEFAULT_MAX_SIZE, Buffer::cap_up)
            .max(buffer.cap());

        Reader {
            buffer,
            max_capacity,
            reader,
        }
    }
}

impl<R: ?Sized> Reader<R> {
    // TODO: stuff

    /*
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
        self.buffer.clear();
    }

    pub fn compact(&mut self) {
        self.buffer.compact();
    }
    */
}

impl<R: Read + ?Sized> Reader<R> {
    // Might want a non-consuming version of fill_buf, though might want to
    // rename it to something else as well...
    /*
    #[expect(clippy::indexing_slicing, reason = "Safe by invariant")]
    fn my_fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.buffer.pos() >= self.buffer.len() {
            debug_assert!(self.buffer.pos() == self.buffer.len());
            // We've consumed all the data we have

            // Read to fill the internal buffer
            let _ = self.buffer.fill(&mut self.reader)?;
        }

        // Return buffered data
        Ok(&self.buffer.buf()[self.buffer.pos()..])
    }
    */
}

impl<R: Read + ?Sized> Read for Reader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.buffer.pos() >= self.buffer.len() {
            debug_assert!(self.buffer.pos() == self.buffer.len());
            // We've consumed all the data we have

            // Clear all data in the internal buffer
            self.buffer.clear();

            // Let the inner reader take things from here
            // Reading into the target buffer directly
            return self.reader.read(buffer);
        }

        // Get a slice of data to put in the buffer.
        let mut data = self.fill_buf()?;

        // Read from the slice into the buffers
        let bytes_read = data.read(buffer)?;

        // Consume the read bytes
        self.consume(bytes_read);

        Ok(bytes_read)
    }

    fn read_vectored(&mut self, buffers: &mut [io::IoSliceMut<'_>]) -> io::Result<usize> {
        // Get the total length of all the buffers
        let total_length = buffers.iter().map(|b| b.len()).sum::<usize>();

        if self.buffer.pos() >= self.buffer.len() && total_length >= self.buffer.cap() {
            debug_assert!(self.buffer.pos() == self.buffer.len());
            // If we've consumed the entire buffer and the total length of the target buffers is
            // larger than the current buffer size, then defer to the inner reader

            // Discard all data in the internal buffer
            self.buffer.clear();

            // Let the inner reader take things from here
            return self.reader.read_vectored(buffers);
        }

        // Get a slice of data to put in the buffers.
        let mut data = self.fill_buf()?;

        // Read from the slice into the buffers
        let bytes_read = data.read_vectored(buffers)?;

        // Consume the read bytes
        self.consume(bytes_read);

        Ok(bytes_read)
    }

    // Like BufReader, we can also delegate to the internal reader after clearing our buffer as the
    // internal buffer might have a more efficient method to `read_to_end`
    #[expect(clippy::indexing_slicing, reason = "Safe by invariant")]
    #[expect(clippy::arithmetic_side_effects, reason = "Would OOM before overflow")]
    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        // Get unconsumed data from the internal buffer
        let unconsumed = &self.buffer.buf()[self.buffer.pos()..];
        let unconsumed_bytes = unconsumed.len();

        // Add all we have to the buffer
        buf.try_reserve(unconsumed_bytes)?;
        buf.extend_from_slice(unconsumed);

        // Discard all data in the internal buffer
        self.buffer.clear();

        // Let the inner reader take things from here
        let bytes_read = self.reader.read_to_end(buf)?;

        Ok(unconsumed_bytes + bytes_read)
    }

    fn read_to_string(&mut self, buf: &mut String) -> io::Result<usize> {
        if buf.is_empty() {
            // Here be dragons, don't poke them
            #[expect(unsafe_code, reason = "Exactly what BufReader does")]
            {
                struct Guard<'a> {
                    buf: &'a mut Vec<u8>,
                    len: usize,
                }

                impl Drop for Guard<'_> {
                    fn drop(&mut self) {
                        unsafe {
                            self.buf.set_len(self.len);
                        }
                    }
                }

                let mut g = Guard {
                    len: buf.len(),
                    buf: unsafe { buf.as_mut_vec() },
                };
                let ret = self.read_to_end(g.buf);

                // SAFETY: `read_to_end` only appends data to `g.buf`
                let appended = unsafe { g.buf.get_unchecked(g.len..) };
                if str::from_utf8(appended).is_err() {
                    ret.and_then(|_| {
                        Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "stream did not contain valid UTF-8",
                        ))
                    })
                } else {
                    g.len = g.buf.len();
                    ret
                }
            }
        } else {
            let mut bytes = Vec::new();
            self.read_to_end(&mut bytes)?;
            let string = str::from_utf8(&bytes).map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "stream did not contain valid UTF-8",
                )
            })?;
            *buf += string;
            Ok(string.len())
        }
    }

    #[expect(
        clippy::arithmetic_side_effects,
        clippy::indexing_slicing,
        reason = "Safe by invariant or bounds checks"
    )]
    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        if let Some(slice) = self
            .buffer
            .buf()
            .get(self.buffer.pos()..(self.buffer.pos() + buf.len()))
        {
            // If we have enough data in the buffer

            // Copy the data to the buffer
            buf.copy_from_slice(slice);

            // Mark the data as consumed
            self.consume(buf.len());

            return Ok(());
        }

        let mut pos = 0;
        while pos < buf.len() {
            match self.read(buf[pos..].as_mut()) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "failed to fill whole buffer",
                    ));
                }
                Ok(n) => pos += n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }

        Ok(())
    }
}

impl<R: Read + ?Sized> BufRead for Reader<R> {
    #[expect(clippy::indexing_slicing, reason = "Buffer invariant makes it safe")]
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.buffer.pos() >= self.buffer.len() {
            debug_assert!(self.buffer.pos() == self.buffer.len());
            // We've consumed all the data we have

            // Clear the buffer
            self.buffer.clear();

            // Fill the buffer again
            let _ = self.buffer.fill(&mut self.reader)?;
        }

        // Return the unconsumed data we have
        Ok(&self.buffer.buf()[self.buffer.pos()..])
    }

    fn consume(&mut self, amt: usize) {
        self.buffer.consume(amt);
    }
}

impl<R: Read + ?Sized> DynamicRead for Reader<R> {
    fn capacity(&self) -> usize {
        self.buffer.cap()
    }

    fn shrink(&mut self) {
        self.buffer.shrink();
    }

    fn compact(&mut self) {
        self.buffer.compact();
    }

    fn clear(&mut self) {
        self.buffer.clear();
    }

    fn discard(&mut self) {
        self.buffer.discard();
    }
}

#[cfg(test)]
mod tests;
