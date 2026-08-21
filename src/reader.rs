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
    inner: R,
    initial_capacity: Option<usize>,
    max_capacity: Option<usize>,
}

impl<R: Read> ReaderBuilder<R> {
    /// Sets the initial buffer capacity.
    ///
    /// The requested capacity is rounded up to the crate's chunk alignment when the reader is
    /// built.
    #[inline]
    pub fn initial_capacity(mut self, cap: usize) -> Self {
        self.initial_capacity = Some(cap);
        self
    }

    /// Sets the maximum buffer capacity.
    ///
    /// The requested capacity is rounded up to the buffer's exponential growth alignment when the
    /// reader is built. If the result is smaller than the initial capacity, it is raised to match
    /// the initial capacity.
    #[inline]
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
            inner: self.inner,
        }
    }
}

/// A buffered reader with dynamically managed capacity and retained consumed data.
///
/// # Trait semantics
///
/// A `Reader` can retain consumed bytes for inspection through methods like
/// [`peek_behind`](Self::peek_behind). Methods inherited from [`Read`], [`BufRead`], and [`Seek`]
/// are still allowed to invalidate retained data when they need to synchronize with the inner
/// reader.
///
/// Use [`DynamicRead`] and the inherent methods on `Reader` when retained buffer contents
/// matter across calls.
///
/// [`Seek::seek`] clears the buffer after a successful seek. [`Seek::stream_position`] reports the
/// logical position without clearing it. For buffer-preserving relative movement, use
/// [`Reader::seek_relative`].
#[derive(Debug)]
pub struct Reader<R: ?Sized> {
    buffer: Buffer,
    max_capacity: usize,
    inner: R,
}

impl<R: Read + ?Sized> Read for Reader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.buffer.pos() >= self.buffer.len() && buffer.len() >= self.buffer.cap() {
            debug_assert!(self.buffer.pos() == self.buffer.len());
            /* Buffer is exhausted and the target is at least as large as the current capacity, so
            buffering would just add a copy without holding any leftover data. */

            // Clear the buffer to invalidate its data before delegating to the inner reader
            self.buffer.clear();

            // Let the inner reader take things from here. Reading into the target buffer directly
            return self.inner.read(buffer);
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
            /* Buffer is exhausted and the target is at least as large as the current capacity, so
            buffering would just add a copy without holding any leftover data. */

            // Clear the buffer to invalidate its data before delegating to the inner reader
            self.buffer.clear();

            // Let the inner reader take things from here. Reading into the target buffers directly
            return self.inner.read_vectored(buffers);
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
        let bytes_read = self.inner.read_to_end(buf)?;

        Ok(unconsumed_bytes + bytes_read)
    }

    fn read_to_string(&mut self, buf: &mut String) -> io::Result<usize> {
        if buf.is_empty() {
            // Optimized path for empty string

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
    #[inline]
    #[expect(clippy::indexing_slicing, reason = "pos ≤ len by Buffer invariant")]
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.buffer.pos() >= self.buffer.len() {
            debug_assert!(self.buffer.pos() == self.buffer.len());
            // We've consumed all the data we have

            // Clear the buffer
            self.buffer.clear();

            // Fill the buffer again
            let _ = self.buffer.fill(&mut self.inner)?;
        }

        // Return the unconsumed data we have
        Ok(&self.buffer.buf()[self.buffer.pos()..])
    }

    #[inline]
    fn consume(&mut self, amt: usize) {
        self.buffer.consume(amt);
    }
}

impl<R: Seek + ?Sized> Seek for Reader<R> {
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "pos ≤ len by Buffer invariant"
    )]
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let result = if let SeekFrom::Current(offset) = pos {
            let unconsumed =
                i64::try_from(self.buffer.len() - self.buffer.pos()).map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "buffered data exceeds seek offset range",
                    )
                })?;

            if let Some(inner_offset) = offset.checked_sub(unconsumed) {
                self.inner.seek(SeekFrom::Current(inner_offset))?
            } else {
                /* `offset - unconsumed` cannot fit in one `i64` seek. Rewind by the buffered
                tail first so the inner and logical positions match, then retry the caller's
                original offset without any buffer adjustment. */
                self.inner
                    .seek(SeekFrom::Current(unconsumed.saturating_neg()))?;
                self.buffer.clear();
                return self.inner.seek(SeekFrom::Current(offset));
            }
        } else {
            self.inner.seek(pos)?
        };

        self.buffer.clear();
        Ok(result)
    }

    #[expect(
        clippy::arithmetic_side_effects,
        reason = "pos ≤ len by Buffer invariant"
    )]
    fn stream_position(&mut self) -> io::Result<u64> {
        let unconsumed = u64::try_from(self.buffer.len() - self.buffer.pos()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "buffered data exceeds stream position range",
            )
        })?;

        self.inner
            .stream_position()?
            .checked_sub(unconsumed)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "inner reader position is before unread buffered data",
                )
            })
    }
}

impl<R: Seek + ?Sized> Reader<R> {
    /// Seeks relative to the current logical position.
    ///
    /// If the target remains inside the retained buffer, either forward through unconsumed bytes
    /// or backward through consumed lookbehind, only the buffer cursor moves and the inner reader
    /// is left untouched.
    ///
    /// If the target is outside the retained window, this falls back to [`Seek::seek_relative`].
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "pos ≤ len by Buffer invariant"
    )]
    pub fn seek_relative(&mut self, offset: i64) -> io::Result<()> {
        let pos = self.buffer.pos();
        let len = self.buffer.len();

        if offset >= 0 {
            if let Ok(forward) = usize::try_from(offset) {
                if forward <= len - pos {
                    self.buffer.consume(forward);
                    return Ok(());
                }
            }
        } else if let Ok(backward) = usize::try_from(offset.unsigned_abs()) {
            if backward <= pos {
                self.buffer.unconsume(backward);
                return Ok(());
            }
        }

        Seek::seek_relative(self, offset)
    }
}

impl<R: Read + ?Sized> DynamicRead for Reader<R> {
    #[inline]
    fn capacity(&self) -> usize {
        self.buffer.cap()
    }

    #[inline]
    fn buffer(&self) -> &[u8] {
        self.buffer.buf()
    }

    #[inline]
    fn pos(&self) -> usize {
        self.buffer.pos()
    }

    #[inline]
    fn shrink(&mut self) {
        self.buffer.shrink();
    }

    #[inline]
    fn compact(&mut self) {
        self.buffer.compact();
    }

    #[inline]
    fn clear(&mut self) {
        self.buffer.clear();
    }

    #[inline]
    fn discard(&mut self) {
        self.buffer.discard();
    }

    #[inline]
    fn fill(&mut self) -> io::Result<usize> {
        self.buffer
            .fill(&mut self.inner)
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
            .fill_while(&mut self.inner, predicate, Some(self.max_capacity))
            .map(|reader| reader.count())
    }
}

impl<R: Read> Reader<R> {
    /// Creates a new `Reader` with default configuration.
    ///
    /// The buffer starts at the default capacity and can grow up to [`DEFAULT_MAX_CAPACITY`].
    #[inline]
    pub fn new(inner: R) -> Reader<R> {
        Reader::builder(inner).build()
    }

    /// Returns a [`ReaderBuilder`] for configuring a new `Reader`.
    #[inline]
    pub fn builder(inner: R) -> ReaderBuilder<R> {
        ReaderBuilder {
            inner,
            initial_capacity: None,
            max_capacity: None,
        }
    }
}

impl<R> Reader<R> {
    /// Decomposes the reader into its inner reader and retained buffer.
    ///
    /// The returned [`Buffer`] preserves retained bytes and the current buffer position. Any
    /// unconsumed bytes in that buffer have already been read from the inner reader, so callers
    /// that continue using the inner reader directly must account for them.
    #[inline]
    pub fn into_parts(self) -> (R, Buffer) {
        (self.inner, self.buffer)
    }
}

impl<R: ?Sized> Reader<R> {
    /// Returns a reference to the underlying reader.
    #[inline]
    pub fn get_ref(&self) -> &R {
        &self.inner
    }

    /// Returns a mutable reference to the underlying reader.
    ///
    /// It is inadvisable to directly read from the underlying reader, as data that has already been
    /// buffered will be bypassed by those direct reads.
    #[inline]
    pub fn get_mut(&mut self) -> &mut R {
        &mut self.inner
    }

    /// Replaces the internal buffer with a fresh default-capacity buffer and returns the old one.
    ///
    /// Useful when retained slices into the buffer must outlive the reader's continued use, for
    /// example when handing parsed bytes to another component while the reader keeps reading.
    /// The returned buffer can be parked alongside those slices and dropped once they are no
    /// longer needed.
    ///
    /// The replacement starts at the default capacity; it will grow as subsequent reads require.
    #[inline]
    pub fn take_buffer(&mut self) -> Buffer {
        std::mem::take(&mut self.buffer)
    }

    /// Splits off the consumed portion of the internal buffer and returns it, retaining the
    /// unconsumed portion.
    ///
    /// Like [`take_buffer`](Self::take_buffer), but only the consumed lookbehind leaves the
    /// reader: the returned [`Buffer`] is shortened and shrunk to exactly the consumed bytes,
    /// with its read position at the end. The unconsumed bytes stay in the reader at the start of
    /// a fresh replacement buffer, so subsequent reads continue where they left off.
    ///
    /// The replacement starts at the smallest capacity that fits the unconsumed data; it will
    /// grow as subsequent reads require.
    #[inline]
    pub fn take_consumed(&mut self) -> Buffer {
        self.buffer.take_consumed()
    }

    /// Returns the maximum buffer capacity configured for this reader.
    #[inline]
    pub fn max_capacity(&self) -> usize {
        self.max_capacity
    }

    /// Returns up to `n` unconsumed bytes without advancing the read position.
    ///
    /// If fewer than `n` unconsumed bytes are available, the returned slice contains only what is
    /// available. Returns an empty slice when there is no unconsumed data.
    #[inline]
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
    #[inline]
    #[expect(clippy::indexing_slicing, reason = "Clamped to buffer bounds")]
    pub fn peek_behind(&self, n: usize) -> &[u8] {
        let end = self.buffer.pos();
        let start = end.saturating_sub(n);
        &self.buffer.buf()[start..end]
    }
}

impl<R: Read + ?Sized> Reader<R> {
    /// Ensures a fill request cannot grow the buffer beyond `max_capacity`.
    ///
    /// `max_capacity` is normalized to a chunk boundary, so accepting only requests where
    /// `buffer.len() + amt <= max_capacity` also keeps any chunk-rounded growth within the limit.
    #[inline]
    fn ensure_fill_within_max_capacity(&self, amt: usize) -> io::Result<()> {
        if amt > self.max_capacity.saturating_sub(self.buffer.len()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "requested amount exceeds maximum buffer capacity",
            ));
        }

        Ok(())
    }

    /// Fills the buffer with at least `amt` additional bytes from the underlying reader, growing as
    /// needed.
    ///
    /// Returns the total number of newly read bytes. If the reader reaches EOF before `amt` bytes
    /// are read, the partial count is returned without error.
    ///
    /// Returns an error if the request would cause the buffer to exceed `max_capacity`.
    pub fn fill_amount(&mut self, amt: usize) -> io::Result<usize> {
        self.ensure_fill_within_max_capacity(amt)?;

        self.buffer
            .fill_amount(&mut self.inner, amt)
            .map(|reader| reader.count())
    }

    /// Fills the buffer with exactly `amt` additional bytes from the underlying reader, growing as
    /// needed.
    ///
    /// Returns an error if the reader reaches EOF before `amt` bytes are read
    /// ([`UnexpectedEof`](io::ErrorKind::UnexpectedEof)), or if the request would cause the buffer
    /// to exceed `max_capacity` ([`InvalidInput`](io::ErrorKind::InvalidInput)).
    pub fn fill_exact(&mut self, amt: usize) -> io::Result<()> {
        self.ensure_fill_within_max_capacity(amt)?;

        self.buffer.fill_exact(&mut self.inner, amt)
    }

    /// Reads from the underlying reader until EOF or `max_capacity` is reached.
    ///
    /// Returns the total number of bytes read.
    pub fn fill_to_end(&mut self) -> io::Result<usize> {
        // Can't use Buffer::fill_to_end since it doesn't take a growth limit
        self.fill_while_dyn(&mut |_| true)
    }

    /// Reads from the underlying reader until a byte delimiter is found, EOF, or `max_capacity`
    /// is reached.
    ///
    /// Existing unconsumed data is checked first, and newly read data is appended to the retained
    /// buffer. The delimiter remains in the buffer when found.
    ///
    /// Returns the total number of bytes read.
    pub fn fill_until(&mut self, byte: u8) -> io::Result<usize> {
        self.buffer
            .fill_until(&mut self.inner, byte, Some(self.max_capacity))
            .map(|reader| reader.count())
    }

    /// Reads from the underlying reader until a character delimiter is found, EOF, or
    /// `max_capacity` is reached.
    ///
    /// Existing unconsumed data is checked first. Multi-byte characters that span read boundaries
    /// are handled correctly, and the delimiter remains in the buffer when found.
    ///
    /// Returns the total number of bytes read.
    pub fn fill_until_char(&mut self, ch: char) -> io::Result<usize> {
        self.buffer
            .fill_until_char(&mut self.inner, ch, Some(self.max_capacity))
            .map(|reader| reader.count())
    }

    /// Reads from the underlying reader until a string delimiter is found, EOF, or `max_capacity`
    /// is reached.
    ///
    /// Existing unconsumed data is checked first. Matches that span read boundaries are handled
    /// correctly, and the delimiter remains in the buffer when found.
    ///
    /// Returns the total number of bytes read.
    pub fn fill_until_str(&mut self, needle: &str) -> io::Result<usize> {
        self.buffer
            .fill_until_str(&mut self.inner, needle, Some(self.max_capacity))
            .map(|reader| reader.count())
    }
}

#[cfg(test)]
mod tests;
