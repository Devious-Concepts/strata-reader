use crate::buffer::Buffer;
use crate::constants::DEFAULT_MAX_CAPACITY;
use crate::read::DynamicRead;
use std::io::{self, BufRead, Read, Seek, SeekFrom};

/// A builder for constructing a [`Reader`] with custom capacity settings.
///
/// Both capacities are rounded up to implementation-specific alignment boundaries.
/// If `max_capacity` is less than `initial_capacity`, it is raised to match.
#[derive(Debug, Clone)]
#[must_use]
pub struct ReaderBuilder<R> {
    reader: R,
    initial_capacity: Option<usize>,
    max_capacity: Option<usize>,
}

impl<R: Read> ReaderBuilder<R> {
    /// Sets the initial buffer capacity.
    pub fn initial_capacity(mut self, cap: usize) -> Self {
        self.initial_capacity = Some(cap);
        self
    }

    /// Sets the maximum buffer capacity.
    pub fn max_capacity(mut self, cap: usize) -> Self {
        self.max_capacity = Some(cap);
        self
    }

    /// Builds the [`Reader`] with the configured settings.
    pub fn build(self) -> Reader<R> {
        let buffer = match self.initial_capacity {
            Some(cap) => Buffer::with_capacity(cap),
            None => Buffer::new(),
        };
        let max_capacity = self
            .max_capacity
            .map_or(DEFAULT_MAX_CAPACITY, Buffer::cap_up)
            .max(buffer.cap());

        Reader {
            buffer,
            max_capacity,
            reader: self.reader,
        }
    }
}

#[derive(Debug)]
pub struct Reader<R: ?Sized> {
    buffer: Buffer,
    max_capacity: usize,
    reader: R,
}

impl<R: Read + ?Sized> Read for Reader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.buffer.pos() >= self.buffer.len() && buffer.len() >= self.buffer.cap() {
            debug_assert!(self.buffer.pos() == self.buffer.len());
            // Buffer is exhausted and the target is at least as large as the current capacity, so
            // buffering would just add a copy without holding any leftover data.

            // Clear the buffer to invalidate it's data before delegating to the inner reader
            self.buffer.clear();

            // Let the inner reader take things from here. Reading into the target buffer directly
            return self.reader.read(buffer);
        }

        // Get a slice of data to put in the buffer
        let mut data = self.fill_buf()?;

        // Read from the slice into the buffer
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
            // Buffer is exhausted and the target is at least as large as the current capacity, so
            // buffering would just add a copy without holding any leftover data.

            // Clear the buffer to invalidate it's data before delegating to the inner reader
            self.buffer.clear();

            // Let the inner reader take things from here. Reading into the target buffers directly
            return self.reader.read_vectored(buffers);
        }

        // Get a slice of data to put in the buffers
        let mut data = self.fill_buf()?;

        // Read from the slice into the buffers
        let bytes_read = data.read_vectored(buffers)?;

        // Consume the read bytes
        self.consume(bytes_read);

        Ok(bytes_read)
    }

    // Like BufReader, clear our buffer and delegate if the inner reader optimizes `read_to_end`
    #[expect(clippy::indexing_slicing, reason = "pos ≤ len by Buffer invariant")]
    #[expect(clippy::arithmetic_side_effects, reason = "would OOM before overflow")]
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
            // Optimized path for empty string.

            // Here be dragons, don't poke them!
            #[expect(unsafe_code, reason = "Exactly what BufReader does")]
            {
                // RAII guard to ensure panic-safety and automatic rollback
                struct Guard<'a> {
                    buf: &'a mut Vec<u8>,
                    len: usize,
                }

                impl Drop for Guard<'_> {
                    fn drop(&mut self) {
                        // Truncates the string to rollback invalid UTF-8
                        unsafe {
                            self.buf.set_len(self.len);
                        }
                    }
                }

                // We take mutable ownership of the strings raw bytes with the guard as safety
                let mut g = Guard {
                    len: buf.len(),
                    buf: unsafe { buf.as_mut_vec() },
                };

                // Read directly into the raw buffer
                let ret = self.read_to_end(g.buf);

                // read_to_end only appends so we can skip bounds checks
                let appended = unsafe { g.buf.get_unchecked(g.len..) };

                // validate the appended bytes as valid UTF-8
                if str::from_utf8(appended).is_err() {
                    // Validation failed, return an error. The guard will rollback the string
                    ret.and_then(|_| {
                        Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "stream did not contain valid UTF-8",
                        ))
                    })
                } else {
                    // Validation succeeded, update the string length
                    g.len = g.buf.len();
                    ret
                }
            }
        } else {
            // Fallback path with intermediate vector
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

    #[expect(clippy::arithmetic_side_effects, reason = "would OOM before overflow")]
    #[expect(clippy::indexing_slicing, reason = "pos ≤ buf.len() by loop guard")]
    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        if let Some(slice) = self
            .buffer
            .buf()
            .get(self.buffer.pos()..(self.buffer.pos() + buf.len()))
        {
            // We have enough data in the internal buffer

            // Copy the data to the target buffer
            buf.copy_from_slice(slice);

            // Mark the data as consumed in the internal buffer
            self.consume(buf.len());

            return Ok(());
        }

        // We don't have enough data, so read repeatedly until we do
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
    #[expect(clippy::indexing_slicing, reason = "pos ≤ len by Buffer invariant")]
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

    fn buffer(&self) -> &[u8] {
        self.buffer.buf()
    }

    fn pos(&self) -> usize {
        self.buffer.pos()
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

    fn fill(&mut self) -> io::Result<usize> {
        self.buffer
            .fill(&mut self.reader)
            .map(|reader| reader.count())
    }

    /// Reads from the underlying reader while `predicate` returns `true`.
    ///
    /// See [`DynamicRead::fill_while_dyn`] for the general contract.
    ///
    /// This implementation returns `0` without reading in three cases:
    ///
    /// - The predicate returned `false` on the existing unconsumed data.
    /// - The underlying reader reached EOF while the predicate was still unsatisfied.
    /// - The buffer reached [`max_capacity`](ReaderBuilder::max_capacity)
    ///   while the predicate was still unsatisfied.
    fn fill_while_dyn(&mut self, predicate: &mut dyn FnMut(&[u8]) -> bool) -> io::Result<usize> {
        self.buffer
            .fill_while(&mut self.reader, predicate, Some(self.max_capacity))
            .map(|reader| reader.count())
    }
}

impl<R: Read> Reader<R> {
    /// Creates a new `Reader` with default configuration.
    ///
    /// The buffer starts at the default capacity and can grow up to [`DEFAULT_MAX_CAPACITY`].
    pub fn new(reader: R) -> Reader<R> {
        Reader::builder(reader).build()
    }

    /// Returns a [`ReaderBuilder`] for configuring a new `Reader`.
    pub fn builder(reader: R) -> ReaderBuilder<R> {
        ReaderBuilder {
            reader,
            initial_capacity: None,
            max_capacity: None,
        }
    }
}

impl<R> Reader<R> {
    pub fn into_parts(self) -> (R, Buffer) {
        (self.reader, self.buffer)
    }
}

impl<R: ?Sized> Reader<R> {
    /// Returns a reference to the underlying reader.
    pub fn get_ref(&self) -> &R {
        &self.reader
    }

    /// Returns a mutable reference to the underlying reader.
    ///
    /// It is inadvisable to directly read from the underlying reader, as data that has already been
    /// buffered will be lost.
    pub fn get_mut(&mut self) -> &mut R {
        &mut self.reader
    }

    /// Returns the maximum buffer capacity configured for this reader.
    pub fn max_capacity(&self) -> usize {
        self.max_capacity
    }

    /// Returns up to `n` unconsumed bytes without advancing the read position.
    ///
    /// If fewer than `n` unconsumed bytes are available, the returned slice contains only what is
    /// available. Returns an empty slice when there is no unconsumed data.
    #[expect(clippy::indexing_slicing, reason = "Clamped to buffer bounds")]
    pub fn peek(&self, n: usize) -> &[u8] {
        let start = self.buffer.pos();
        let end = self.buffer.len().min(start.saturating_add(n));
        &self.buffer.buf()[start..end]
    }

    /// Returns up to `n` consumed bytes immediately before the read position.
    ///
    /// The standard [`Read`]/[`BufRead`]/[`Seek`] methods don't know retained consumed bytes exist,
    /// so any of them that fetches from the inner reader, or seeks, may drop the retained prefix.
    ///
    /// If fewer than `n` consumed bytes are retained, the returned slice contains only what is
    /// available. Returns an empty slice when no consumed data is retained.
    #[expect(clippy::indexing_slicing, reason = "Clamped to buffer bounds")]
    pub fn peek_behind(&self, n: usize) -> &[u8] {
        let end = self.buffer.pos();
        let start = end.saturating_sub(n);
        &self.buffer.buf()[start..end]
    }
}

impl<R: Read + ?Sized> Reader<R> {
    /// Fills the buffer with at least `amt` bytes from the underlying reader, growing as needed.
    ///
    /// Returns the total number of bytes read. If the reader reaches EOF before `amt` bytes are
    /// read, the partial count is returned (without error).
    ///
    /// Returns an error if the request would cause the buffer to exceed `max_capacity`.
    pub fn fill_amount(&mut self, amt: usize) -> io::Result<usize> {
        if amt > self.max_capacity.saturating_sub(self.buffer.len()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "requested amount exceeds maximum buffer capacity",
            ));
        }

        self.buffer
            .fill_amount(&mut self.reader, amt)
            .map(|reader| reader.count())
    }

    /// Fills the buffer with exactly `amt` bytes from the underlying reader, growing as needed.
    ///
    /// Returns an error if the reader reaches EOF before `amt` bytes are read
    /// ([`UnexpectedEof`](io::ErrorKind::UnexpectedEof)), or if the request would cause the buffer
    /// to exceed `max_capacity` ([`InvalidInput`](io::ErrorKind::InvalidInput)).
    pub fn fill_exact(&mut self, amt: usize) -> io::Result<()> {
        if amt > self.max_capacity.saturating_sub(self.buffer.len()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "requested amount exceeds maximum buffer capacity",
            ));
        }

        self.buffer.fill_exact(&mut self.reader, amt)
    }

    /// Reads from the underlying reader until EOF or `max_capacity` is reached.
    ///
    /// Returns the total number of bytes read.
    pub fn fill_to_end(&mut self) -> io::Result<usize> {
        // Can't use Buffer::fill_to_end since it doesn't take a growth limit.
        self.fill_while_dyn(&mut |_| true)
    }

    /// Reads from the underlying reader until a byte delimiter is found, EOF, or `max_capacity`
    /// is reached. Only newly-read data is scanned for `byte` each iteration.
    ///
    /// Returns the total number of bytes read.
    pub fn fill_until(&mut self, byte: u8) -> io::Result<usize> {
        self.buffer
            .fill_until(&mut self.reader, byte, Some(self.max_capacity))
            .map(|reader| reader.count())
    }

    /// Reads from the underlying reader until a character delimiter is found, EOF, or
    /// `max_capacity` is reached. Multi-byte characters that span read boundaries are handled
    /// correctly.
    ///
    /// Returns the total number of bytes read.
    pub fn fill_until_char(&mut self, ch: char) -> io::Result<usize> {
        self.buffer
            .fill_until_char(&mut self.reader, ch, Some(self.max_capacity))
            .map(|reader| reader.count())
    }

    /// Reads from the underlying reader until a string delimiter is found, EOF, or `max_capacity`
    /// is reached. Matches that span read boundaries are handled correctly.
    ///
    /// Returns the total number of bytes read.
    pub fn fill_until_str(&mut self, needle: &str) -> io::Result<usize> {
        self.buffer
            .fill_until_str(&mut self.reader, needle, Some(self.max_capacity))
            .map(|reader| reader.count())
    }
}

// TODO: Fully replace the Seek impl and concrete seek_relative
#[expect(
    clippy::arithmetic_side_effects,
    reason = "pos ≤ len by Buffer invariant"
)]
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_wrap,
    reason = "buffer size ≤ max_capacity ≪ i64::MAX"
)]
impl<R: Read + Seek + ?Sized> Seek for Reader<R> {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let result = match pos {
            SeekFrom::Current(n) => {
                let unconsumed = (self.buffer.len() - self.buffer.pos()) as i64;
                let adjusted = n.checked_sub(unconsumed).ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "seek offset overflow")
                })?;
                self.reader.seek(SeekFrom::Current(adjusted))?
            }
            _ => self.reader.seek(pos)?,
        };
        self.buffer.clear();
        Ok(result)
    }
}

impl<R: Read + Seek + ?Sized> Reader<R> {
    /// Seeks relative to the current position, with an in-buffer fast path.
    ///
    /// If the target position falls within the currently buffered data, either
    /// forward into unconsumed bytes or backward into retained consumed bytes,
    /// the seek is performed without any I/O. Otherwise, the seek is delegated
    /// to the underlying reader and the buffer is invalidated.
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "pos ≤ len by Buffer invariant"
    )]
    #[expect(clippy::as_conversions, reason = "casts are lossless or range-checked")]
    #[expect(clippy::cast_sign_loss, reason = "non-negative i64 fits in u64")]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "values bounded by buffer positions, which fit in usize"
    )]
    pub fn seek_relative(&mut self, offset: i64) -> io::Result<()> {
        let pos = self.buffer.pos();
        let len = self.buffer.len();

        if offset >= 0 {
            let offset_u64 = offset as u64;
            if offset_u64 <= (len - pos) as u64 {
                self.buffer.consume(offset_u64 as usize);
                return Ok(());
            }
        } else {
            let back_u64 = offset.unsigned_abs();
            if back_u64 <= pos as u64 {
                self.buffer.unconsume(back_u64 as usize);
                return Ok(());
            }
        }

        self.seek(SeekFrom::Current(offset))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;

