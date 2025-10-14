//! High-performance buffer with dynamic capacity management.
//!
//! The [`Buffer`] type provides standalone buffered I/O operations with automatic growth
//! and shrinking. It is also used as the internal buffer for [`Reader`](crate::Reader).
//!
//! # Example
//!
//! ```
//! use strata_reader::buffer::Buffer;
//! use std::io::Cursor;
//!
//! let mut buffer = Buffer::new();
//! let mut reader = Cursor::new("aaaa:bbbb");
//!
//! // Read until delimiter
//! let count = buffer.fill_until(&mut reader, ':').unwrap();
//! assert_eq!(count, 5); // "aaaa:" (includes delimiter)
//!
//! // Access the data
//! assert_eq!(buffer.as_str().unwrap(), "aaaa:bbbb");
//!
//! // Consume what we processed
//! buffer.consume(count);
//!
//! // Process remaining data
//! assert_eq!(buffer.as_str().unwrap(), "bbbb");
//! ```

use std::cmp;
use std::io::{self, Read};

use crate::constants::{CHUNK_SIZE, PRACTICAL_MAX_SIZE};

/// A dynamically sized buffer with chunked capacity management.
///
/// `Buffer` provides high-performance buffered I/O operations with automatic capacity adjustment.
/// It grows and shrinks its capacity based on usage patterns, making it ideal for scenarios with
/// varying data sizes. This type is also used as the internal buffer for
/// [`Reader`](crate::Reader).
///
/// # Capacity Management
///
/// The buffer uses a chunk-based growth and shrinking strategy:
/// - **Growth**: Expands in exponential steps (powers of 2) aligned to [`CHUNK_SIZE`] boundaries,
///   allowing efficient memory allocation for large reads.
/// - **Shrinking**: Contracts linearly to `CHUNK_SIZE` multiples, freeing unused memory while
///   maintaining alignment with typical page sizes for optimal I/O performance.
/// - **Alignment**: All capacities are multiples of `CHUNK_SIZE`, which is tuned for common
///   system page sizes.
///
/// # Use Cases
///
/// - Tokenization and parsing (e.g., JSON decoding with [`fill_while`](Self::fill_while) and
///   [`fill_until`](Self::fill_until))
/// - Large file processing with dynamic memory needs
/// - Stream buffering with arbitrary peek-ahead requirements
/// - Any scenario requiring efficient buffered reads with automatic capacity adjustment
///
/// # Invariants
///
/// This buffer maintains the invariant `0 <= self.pos <= self.len <= self.buf.len() <= self.cap <=
/// self.buf.capacity()` at all times, ensuring memory safety and correctness of all operations.
#[derive(Debug)]
pub struct Buffer {
    /// Internal buffer storage.
    buf: Vec<u8>,
    /// Logical capacity of the buffer (may be slightly less than `buf.capacity()`).
    cap: usize,
    /// Number of bytes currently stored in the buffer.
    len: usize,
    /// Number of bytes that have been consumed (read position).
    pos: usize,
}

impl Buffer {
    /// Creates a new buffer with default capacity.
    ///
    /// The buffer is initialized with a capacity of [`CHUNK_SIZE`].
    ///
    /// # Examples
    ///
    /// ```
    /// # use strata_reader::buffer::Buffer;
    /// let buffer = Buffer::new();
    /// assert_eq!(buffer.len(), 0);
    /// ```
    #[inline]
    pub fn new() -> Self {
        Self {
            buf: vec![0; CHUNK_SIZE],
            cap: CHUNK_SIZE,
            len: 0,
            pos: 0,
        }
    }

    /// Creates a new buffer with at least the specified capacity.
    ///
    /// The actual capacity will be rounded up to the nearest power-of-2 multiple of
    /// [`CHUNK_SIZE`] that can accommodate the requested capacity.
    ///
    /// # Examples
    ///
    /// ```
    /// # use strata_reader::buffer::Buffer;
    /// # use strata_reader::constants::CHUNK_SIZE;
    /// let buffer = Buffer::with_capacity(100);
    /// assert!(buffer.cap() >= 100);
    /// assert_eq!(buffer.cap() % CHUNK_SIZE, 0); // Aligned to CHUNK_SIZE
    /// ```
    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        // Round up to fit the requested capacity
        let cap = Self::cap_up(capacity);

        Self {
            buf: vec![0; cap],
            cap,
            len: 0,
            pos: 0,
        }
    }

    /// Returns a reference to the buffer contents from the beginning up to the current length.
    ///
    /// This returns a slice of the internal buffer from index 0 to [`len()`](Self::len),
    /// which includes both consumed and unconsumed data. Use with [`pos()`](Self::pos) to access
    /// either unconsumed `pos()..` or consumed `..pos()` data as needed.
    ///
    /// # Examples
    ///
    /// ```
    /// # use strata_reader::buffer::Buffer;
    /// # use std::io::Cursor;
    /// let mut buffer = Buffer::new();
    /// let mut reader = Cursor::new(b"Hello, World!");
    /// buffer.fill(&mut reader).unwrap();
    ///
    /// let slice = buffer.buf();
    /// assert_eq!(slice.len(), 13); // Full data length
    /// // Access unconsumed data: &slice[buffer.pos()..]
    /// ```
    #[expect(clippy::indexing_slicing, reason = "The invariant makes it safe")]
    #[inline]
    pub fn buf(&self) -> &[u8] {
        &self.buf[..self.len]
    }

    /// Returns the current capacity of the buffer in bytes.
    ///
    /// This is always a multiple of [`CHUNK_SIZE`] and represents the maximum number of bytes
    /// the buffer can hold before needing to grow.
    #[inline]
    pub fn cap(&self) -> usize {
        self.cap
    }

    /// Returns the number of bytes currently in the buffer.
    ///
    /// This includes both consumed and unconsumed data. The unconsumed portion is
    /// `len() - pos()` bytes.
    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns `true` if the buffer contains no data.
    ///
    /// Equivalent to `self.len() == 0`.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Returns the current read position (number of consumed bytes).
    ///
    /// Data from index 0 to `pos()` has been consumed. Unconsumed data starts at `pos()`.
    #[inline]
    pub fn pos(&self) -> usize {
        self.pos
    }

    /// Discards all data in the buffer and shrinks capacity.
    ///
    /// Resets both the read position and length to zero, then shrinks the buffer to
    /// [`CHUNK_SIZE`].
    ///
    /// # Examples
    ///
    /// ```
    /// # use strata_reader::buffer::Buffer;
    /// # use std::io::Cursor;
    /// let mut buffer = Buffer::new();
    /// buffer.fill(Cursor::new(b"data")).unwrap();
    /// buffer.discard();
    /// assert_eq!(buffer.len(), 0);
    /// assert_eq!(buffer.pos(), 0);
    /// ```
    #[inline]
    pub fn discard(&mut self) {
        self.pos = 0;
        self.len = 0;

        self.shrink();
    }

    /// Marks `amt` bytes as consumed, advancing the read position.
    ///
    /// If `amt` exceeds the available unconsumed data, the position is clamped to
    /// [`len()`](Self::len).
    ///
    /// # Examples
    ///
    /// ```
    /// # use strata_reader::buffer::Buffer;
    /// # use std::io::Cursor;
    /// let mut buffer = Buffer::new();
    /// buffer.fill(Cursor::new(b"Hello")).unwrap();
    /// buffer.consume(2);
    /// assert_eq!(buffer.pos(), 2);
    /// ```
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "The invariant makes it safe"
    )]
    #[inline]
    pub fn consume(&mut self, amt: usize) {
        self.pos = cmp::min(self.pos + amt, self.len);
    }

    /// Removes consumed bytes from the buffer and shrinks capacity.
    ///
    /// Moves unconsumed data to the beginning of the buffer using `copy_within`, resets the
    /// read position to 0, and shrinks the capacity to fit the remaining data. This operation
    /// is useful after consuming a significant amount of data to reclaim space.
    ///
    /// # Examples
    ///
    /// ```
    /// # use strata_reader::buffer::Buffer;
    /// # use std::io::Cursor;
    /// let mut buffer = Buffer::new();
    /// buffer.fill(Cursor::new(b"Hello, World!")).unwrap();
    /// buffer.consume(7);  // Consume "Hello, "
    /// buffer.compact();   // Move "World!" to start
    /// assert_eq!(buffer.pos(), 0);
    /// assert_eq!(buffer.len(), 6);  // "World!"
    /// ```
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "The invariant makes it safe"
    )]
    #[inline]
    pub fn compact(&mut self) {
        self.buf.copy_within(self.pos..self.len, 0);
        self.len -= self.pos;
        self.pos = 0;

        self.shrink();
    }

    /// Rounds capacity down to the nearest [`CHUNK_SIZE`] multiple.
    ///
    /// This method implements the linear shrinking strategy used by the buffer. It rounds down
    /// the given capacity to the nearest multiple of [`CHUNK_SIZE`], with a minimum of
    /// [`CHUNK_SIZE`] and a maximum of [`PRACTICAL_MAX_SIZE`].
    ///
    /// # Use Cases
    ///
    /// - Calculating optimal buffer capacity when downsizing
    /// - Pre-calculating shrink targets before performing operations
    /// - Understanding the buffer's shrinking behavior
    ///
    /// # Examples
    ///
    /// ```
    /// # use strata_reader::buffer::Buffer;
    /// # use strata_reader::constants::CHUNK_SIZE;
    /// // Rounds down to nearest CHUNK_SIZE multiple
    /// assert_eq!(Buffer::cap_down(CHUNK_SIZE + 1), CHUNK_SIZE);
    /// assert_eq!(Buffer::cap_down(2 * CHUNK_SIZE + 100), 2 * CHUNK_SIZE);
    ///
    /// // Minimum is CHUNK_SIZE
    /// assert_eq!(Buffer::cap_down(0), CHUNK_SIZE);
    /// assert_eq!(Buffer::cap_down(CHUNK_SIZE / 2), CHUNK_SIZE);
    /// ```
    #[inline]
    #[expect(clippy::arithmetic_side_effects, reason = "Bounds checks make it safe")]
    pub fn cap_down(capacity: usize) -> usize {
        // The bounds checks prevent both underflow and overflow problems by setting the minimum at
        // `CHUNK_SIZE` and maximum at [`PRACTICAL_MAX_SIZE`]. The smallest value the calculation
        // can reach is `CHUNK_SIZE` when `capacity` is less than `2 * CHUNK_SIZE`.

        // Min bounds check
        if capacity <= CHUNK_SIZE {
            return CHUNK_SIZE;
        }

        // Max bounds check
        if capacity >= PRACTICAL_MAX_SIZE {
            return PRACTICAL_MAX_SIZE;
        }

        // Round down `capacity` to the nearest multiple of `CHUNK_SIZE`
        (capacity / CHUNK_SIZE) * CHUNK_SIZE
    }

    /// Rounds capacity up to the nearest power-of-2 multiple of [`CHUNK_SIZE`].
    ///
    /// This method implements the exponential growth strategy used by the buffer. It rounds up
    /// the given capacity to the nearest power-of-2 multiple of [`CHUNK_SIZE`] (e.g.,
    /// `CHUNK_SIZE`, `2 * CHUNK_SIZE`, `4 * CHUNK_SIZE`, etc.), with a minimum of [`CHUNK_SIZE`]
    /// and a maximum of [`PRACTICAL_MAX_SIZE`].
    ///
    /// # Use Cases
    ///
    /// - Pre-allocating buffers with optimal capacity before operations
    /// - Calculating growth targets to minimize reallocations
    /// - Understanding the buffer's growth behavior
    ///
    /// # Examples
    ///
    /// ```
    /// # use strata_reader::buffer::Buffer;
    /// # use strata_reader::constants::CHUNK_SIZE;
    /// // Rounds up to nearest power-of-2 multiple of CHUNK_SIZE
    /// assert_eq!(Buffer::cap_up(CHUNK_SIZE), CHUNK_SIZE);
    /// assert_eq!(Buffer::cap_up(CHUNK_SIZE + 1), 2 * CHUNK_SIZE);
    /// assert_eq!(Buffer::cap_up(2 * CHUNK_SIZE), 2 * CHUNK_SIZE);
    /// assert_eq!(Buffer::cap_up(2 * CHUNK_SIZE + 1), 4 * CHUNK_SIZE);
    ///
    /// // Minimum is CHUNK_SIZE
    /// assert_eq!(Buffer::cap_up(0), CHUNK_SIZE);
    /// assert_eq!(Buffer::cap_up(100), CHUNK_SIZE); // if CHUNK_SIZE > 100
    /// ```
    #[inline]
    #[expect(clippy::arithmetic_side_effects, reason = "Bounds checks make it safe")]
    pub fn cap_up(capacity: usize) -> usize {
        // The bounds checks prevent both underflow and overflow problems by setting the minimum at
        // `CHUNK_SIZE` and maximum at [`PRACTICAL_MAX_SIZE`]. The max value that the power of two
        // calculation can reach is `PRACTICAL_MAX_SIZE / 2` when `capacity` is greater than
        // `PRACTICAL_MAX_SIZE / 4`.

        // Max bounds check
        if capacity >= PRACTICAL_MAX_SIZE >> 1 {
            return PRACTICAL_MAX_SIZE;
        }

        // Min bounds check
        if capacity < CHUNK_SIZE {
            return CHUNK_SIZE;
        }

        // Round up `capacity` to the nearest power of two multiple of `CHUNK_SIZE`
        ((capacity + CHUNK_SIZE - 1) / CHUNK_SIZE).next_power_of_two() * CHUNK_SIZE
    }

    /// Grows the buffer capacity to the next power-of-2 multiple of [`CHUNK_SIZE`].
    ///
    /// The capacity grows exponentially (e.g., `CHUNK_SIZE` → `2 * CHUNK_SIZE` → `4 * CHUNK_SIZE`)
    /// up to [`PRACTICAL_MAX_SIZE`].
    ///
    /// # Examples
    ///
    /// ```
    /// # use strata_reader::buffer::Buffer;
    /// # use strata_reader::constants::CHUNK_SIZE;
    /// let mut buffer = Buffer::new();
    /// let initial_cap = buffer.cap();
    /// buffer.grow();
    /// assert_eq!(buffer.cap(), 2 * initial_cap);
    /// ```
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "The invariant makes it safe"
    )]
    #[inline]
    pub fn grow(&mut self) {
        // The capacity maxes out at [`PRACTICAL_MAX_SIZE`] so adding `CHUNK_SIZE` can't overflow.

        // Round `self.cap()` up to ensure we advance to the next power of two multiple of
        // `CHUNK_SIZE` even when already at a power of two.
        let next = Self::cap_up(self.cap + CHUNK_SIZE - 1);

        self.buf.resize(next, 0);
        self.cap = next;
    }

    /// Shrinks the buffer capacity to the smallest [`CHUNK_SIZE`] multiple that fits the current data.
    ///
    /// The capacity shrinks linearly to reduce memory usage while maintaining alignment with
    /// `CHUNK_SIZE` boundaries. Minimum capacity is always `CHUNK_SIZE`.
    ///
    /// This method is called automatically by [`compact()`](Self::compact) and
    /// [`discard()`](Self::discard).
    ///
    /// # Examples
    ///
    /// ```
    /// # use strata_reader::buffer::Buffer;
    /// # use strata_reader::constants::CHUNK_SIZE;
    /// let mut buffer = Buffer::with_capacity(8 * CHUNK_SIZE);
    /// buffer.shrink();
    /// assert_eq!(buffer.cap(), CHUNK_SIZE);  // Shrinks to minimum when empty
    /// ```
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "The invariant makes it safe"
    )]
    #[inline]
    pub fn shrink(&mut self) {
        // The length maxes out at [`PRACTICAL_MAX_SIZE`] so adding `CHUNK_SIZE` can't overflow.

        // Round `self.len()` up to the next chunk boundary to ensure `self.cap()` >= `self.len()`
        let next = Self::cap_down(self.len + CHUNK_SIZE - 1);

        self.buf.truncate(next);
        self.buf.shrink_to(next);
        self.cap = next;
    }

    /// Fills the available space in the buffer from a reader.
    ///
    /// Reads data to fill the buffer up to its current capacity. Does not grow the buffer.
    /// Returns the number of bytes read, which may be 0 if the buffer is already full or if
    /// the reader reaches EOF.
    ///
    /// # Examples
    ///
    /// ```
    /// # use strata_reader::buffer::Buffer;
    /// # use std::io::Cursor;
    /// let mut buffer = Buffer::new();
    /// let mut reader = Cursor::new(b"Hello, World!");
    /// let bytes_read = buffer.fill(&mut reader).unwrap();
    /// assert_eq!(bytes_read, 13);
    /// assert_eq!(buffer.len(), 13);
    /// ```
    ///
    /// # Errors
    ///
    /// Returns any I/O errors encountered while reading from the reader.
    #[expect(
        clippy::arithmetic_side_effects,
        clippy::indexing_slicing,
        reason = "The invariant makes it safe"
    )]
    pub fn fill(&mut self, mut reader: impl Read) -> io::Result<usize> {
        let mut bytes_read = 0;

        // Check if there is space available to fill
        if self.len < self.cap {
            // Read to fill the remaining space.
            bytes_read += reader.read(&mut self.buf[self.len..self.cap])?;

            // Increase the length by the number of bytes read.
            self.len += bytes_read;
        }

        Ok(bytes_read)
    }

    /// Fills the buffer with at least `amt` bytes from a reader, growing as needed.
    ///
    /// Automatically grows the buffer capacity to accommodate the requested amount of data.
    /// Stops reading when at least `amt` bytes have been read, EOF is reached, or the maximum
    /// capacity ([`PRACTICAL_MAX_SIZE`]) is reached.
    ///
    /// After reading, the buffer is shrunk to fit the actual data read.
    ///
    /// # Examples
    ///
    /// ```
    /// # use strata_reader::buffer::Buffer;
    /// # use strata_reader::constants::CHUNK_SIZE;
    /// # use std::io::Cursor;
    /// let mut buffer = Buffer::new();
    /// let data = vec![0u8; 3 * CHUNK_SIZE];
    /// let mut reader = Cursor::new(data);
    /// let bytes_read = buffer.fill_amount(&mut reader, 3 * CHUNK_SIZE).unwrap();
    /// assert!(bytes_read >= 3 * CHUNK_SIZE);
    /// ```
    ///
    /// # Errors
    ///
    /// Returns any I/O errors encountered while reading from the reader.
    #[expect(
        clippy::arithmetic_side_effects,
        clippy::indexing_slicing,
        reason = "The invariant makes it safe"
    )]
    pub fn fill_amount(&mut self, mut reader: impl Read, amt: usize) -> io::Result<usize> {
        let mut total_bytes_read = 0;

        // Loop until we've read enough bytes or break
        while total_bytes_read < amt {
            // Get available space.
            let available = self.cap - self.len;

            // Get remaining amount to read.
            let remaining = amt - total_bytes_read;

            // Note regarding `CHUNK_SIZE / 2`: This should align with a common page size by the
            // very definition of how `CHUNK_SIZE` is set and what values are valid for it.
            if available < CHUNK_SIZE / 2 && remaining >= available {
                // We've hit a point where growing would be more optimal than just filling the
                // available space and the available space is too small.
                if self.cap < PRACTICAL_MAX_SIZE {
                    // Available space is insufficient, grow to the next size.
                    self.grow();
                } else {
                    // Can't grow anymore.
                    break;
                }
            }

            // Fill all available space
            let bytes_read = match reader.read(&mut self.buf[self.len..self.cap]) {
                Err(e) => {
                    self.shrink();
                    return Err(e);
                }
                Ok(r) => r,
            };

            // Increase the length by the number of bytes read.
            self.len += bytes_read;

            // Increase the total number of bytes read.
            total_bytes_read += bytes_read;

            if bytes_read == 0 {
                // We've hit EOF.
                break;
            }
        }

        // Shrink in case we were overeager with our growth.
        self.shrink();

        Ok(total_bytes_read)
    }

    /// Aligns a position backward to the start of the current UTF-8 character.
    ///
    /// If `pos` falls in the middle of a UTF-8 multi-byte character, this moves backward to the
    /// start of that character. If already on a character boundary, returns that position.
    /// The result is clamped to the range `self.pos()..=self.len()`.
    ///
    /// Note that if `self.pos()` itself is in the middle of a multi-byte character (e.g., after
    /// [`consume()`](Self::consume) was called mid-character), the returned position may still
    /// not be on a valid character boundary, as it cannot move before `self.pos()`.
    ///
    /// This is useful when working with UTF-8 text to ensure operations don't split characters.
    ///
    /// # Examples
    ///
    /// ```
    /// # use strata_reader::buffer::Buffer;
    /// # use std::io::Cursor;
    /// let mut buffer = Buffer::new();
    /// buffer.fill(Cursor::new("Hello, 世界")).unwrap();
    /// // "Hello, " = 7 bytes, '世' starts at byte 7 (3 bytes), '界' starts at byte 10
    /// // Position 8 is in the middle of the '世' character
    /// let aligned = buffer.align_pos_to_char(8);
    /// assert_eq!(aligned, 7); // Aligned to start of '世'
    /// ```
    #[expect(
        clippy::arithmetic_side_effects,
        clippy::indexing_slicing,
        reason = "The invariant makes it safe"
    )]
    #[inline]
    pub fn align_pos_to_char(&self, offset: usize) -> usize {
        // Get the offset position clamped by: self.pos <= pos <= self.len
        let offset = cmp::max(offset, self.pos).min(self.len);

        // Find the position of first byte that is not a UTF-8 continuation byte
        self.buf[self.pos..=offset]
            .iter()
            .rev()
            // If the top two bits are 10 then it's a continuation byte, this bitmask checks that
            .position(|&b| b & 0b1100_0000 != 0b1000_0000)
            .map_or(self.pos, |i| offset - i)
    }

    /// Aligns a position forward to the start of the next UTF-8 character.
    ///
    /// If `pos` falls in the middle of a UTF-8 multi-byte character, this moves forward to the
    /// start of the following character. If already on a character boundary, returns that position.
    /// The result is clamped to the range `self.pos()..=self.len()`.
    ///
    /// Note that if `self.len()` is in the middle of a multi-byte character (e.g., an incomplete
    /// UTF-8 sequence at the end of the buffer), the returned position may still not be on a valid
    /// character boundary, as it cannot move beyond `self.len()`.
    ///
    /// This is useful when working with UTF-8 text to ensure operations don't split characters.
    ///
    /// # Examples
    ///
    /// ```
    /// # use strata_reader::buffer::Buffer;
    /// # use std::io::Cursor;
    /// let mut buffer = Buffer::new();
    /// buffer.fill(Cursor::new("Hello, 世界")).unwrap();
    /// // "Hello, " = 7 bytes, '世' starts at byte 7 (3 bytes), '界' starts at byte 10
    /// // Position 8 is in the middle of the '世' character
    /// let aligned = buffer.align_pos_to_next_char(8);
    /// assert_eq!(aligned, 10); // Aligned to start of '界'
    /// ```
    #[expect(
        clippy::arithmetic_side_effects,
        clippy::indexing_slicing,
        reason = "The invariant makes it safe"
    )]
    #[inline]
    pub fn align_pos_to_next_char(&self, offset: usize) -> usize {
        // Get the offset position clamped by: self.pos <= pos <= self.len
        let offset = cmp::max(offset, self.pos).min(self.len);

        // Find the position of first byte that is not a UTF-8 continuation byte
        self.buf[offset..self.len]
            .iter()
            // If the top two bits are 10 then it's a continuation byte, this bitmask checks that
            .position(|&b| b & 0b1100_0000 != 0b1000_0000)
            .map_or(self.len, |i| i + offset)
    }

    /// Returns the unconsumed buffer data as a UTF-8 string slice.
    ///
    /// Automatically handles partial UTF-8 sequences by:
    /// - Skipping incomplete sequences at the start (e.g., if [`consume()`](Self::consume) was
    ///   called mid-character)
    /// - Trimming incomplete sequences at the end
    ///
    /// This is a convenience method for working with text data without manual UTF-8 handling.
    ///
    /// # Examples
    ///
    /// ```
    /// # use strata_reader::buffer::Buffer;
    /// # use std::io::Cursor;
    /// let mut buffer = Buffer::new();
    /// buffer.fill(Cursor::new("Hello, 世界!")).unwrap();
    /// let text = buffer.as_str().unwrap();
    /// assert_eq!(text, "Hello, 世界!");
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an [`io::Error`] with kind [`InvalidData`](io::ErrorKind::InvalidData) if the
    /// buffer contains invalid UTF-8 sequences (not at boundaries).
    pub fn as_str(&self) -> io::Result<&str> {
        self.as_str_from(self.pos)
    }

    /// Returns buffer data as a UTF-8 string slice starting from a specific position.
    ///
    /// Like [`as_str()`](Self::as_str), but starts from a position in the buffer clamped to the
    /// range `self.pos()..=self.len()`. Automatically handles partial UTF-8 sequences at
    /// both boundaries.
    ///
    /// # Examples
    ///
    /// ```
    /// # use strata_reader::buffer::Buffer;
    /// # use std::io::Cursor;
    /// let mut buffer = Buffer::new();
    /// buffer.fill(Cursor::new("Hello, World!")).unwrap();
    /// let text = buffer.as_str_from(7).unwrap();
    /// assert_eq!(text, "World!");
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an [`io::Error`] with kind [`InvalidData`](io::ErrorKind::InvalidData) if the
    /// buffer contains invalid UTF-8 sequences (not at boundaries).
    #[expect(
        clippy::arithmetic_side_effects,
        clippy::indexing_slicing,
        reason = "The invariant makes it safe"
    )]
    pub fn as_str_from(&self, pos: usize) -> io::Result<&str> {
        let start = self.align_pos_to_next_char(pos);

        let mut end = self.len;
        while end > start {
            match str::from_utf8(&self.buf[start..end]) {
                Ok(s) => return Ok(s),
                Err(e) => {
                    if e.error_len().is_none() {
                        // If we have an incomplete sequence at the end, then trim it
                        end = start + e.valid_up_to();
                    } else {
                        // We have an invalid UTF-8 sequence in the middle
                        return Err(io::Error::new(io::ErrorKind::InvalidData, e));
                    }
                }
            }
        }

        Ok("")
    }

    /// Fills the buffer while a predicate matches characters, growing as needed.
    ///
    /// Reads from the reader and decodes UTF-8 characters, continuing as long as the predicate
    /// returns `true`. Stops when a character fails the predicate, EOF is reached, or the
    /// maximum capacity ([`PRACTICAL_MAX_SIZE`]) is reached.
    ///
    /// The buffer automatically grows to accommodate matching data.
    ///
    /// # Return Value
    ///
    /// Returns the number of **valid UTF-8 bytes** that matched the predicate. Note that the
    /// buffer may contain significantly more data than this count, since reads occur in chunks
    /// and may include data beyond where the predicate first fails.
    ///
    /// # Examples
    ///
    /// ```
    /// # use strata_reader::buffer::Buffer;
    /// # use std::io::Cursor;
    /// let mut buffer = Buffer::new();
    /// let mut reader = Cursor::new("aaaaaHello");
    /// let count = buffer.fill_while(&mut reader, |c| c == 'a').unwrap();
    /// assert_eq!(count, 5);  // Five 'a' characters matched
    /// assert_eq!(buffer.len(), 10);  // But entire string was read
    /// ```
    ///
    /// # Errors
    ///
    /// Returns any I/O errors encountered while reading, or UTF-8 validation errors.
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "The invariant makes it safe"
    )]
    pub fn fill_while<P: Fn(char) -> bool>(
        &mut self,
        mut reader: impl Read,
        predicate: P,
    ) -> io::Result<usize> {
        // Get pos aligned to the next char, this is where we start checking against the predicate
        let mut check_pos = self.align_pos_to_next_char(self.pos);
        // The number of valid bytes read
        let mut total_valid_read = 0;

        loop {
            // If the buffer is currently full then start by growing it
            if self.len == self.cap {
                self.grow();
            }

            let read = match self.fill(&mut reader) {
                Ok(r) => r,
                Err(e) => {
                    self.shrink();
                    return Err(e);
                }
            };

            // EOF detection
            if read == 0 {
                break;
            }

            // Get the new part
            let string = &self.as_str_from(check_pos)?;

            // Look for a non-matching char
            if let Some((byte_index, _)) = string.char_indices().find(|(_, c)| !predicate(*c)) {
                // Add the number of valid bytes until the predicate breaks to the total valid bytes
                // read so far and return it.
                return Ok(total_valid_read + byte_index);
            }

            // All read characters are valid
            // Some boundary bytes may slip into the next iteration and be counted there
            total_valid_read += string.len();
            // Likewise, update the check position to the last valid full char
            // This way any partial chars at the end of the buffer are parsed in the next iteration
            check_pos += string.len();
        }

        // Shrink in case we were overeager with our growth.
        self.shrink();

        Ok(total_valid_read)
    }

    /// Fills the buffer until a delimiter character is found, growing as needed.
    ///
    /// Reads from the reader until the specified delimiter character is encountered. Stops when
    /// the delimiter is found, EOF is reached, or the maximum capacity ([`PRACTICAL_MAX_SIZE`])
    /// is reached.
    ///
    /// The buffer automatically grows to accommodate the data.
    ///
    /// # Return Value
    ///
    /// Returns the number of **valid UTF-8 bytes** **including the delimiter** if found. Note
    /// that the buffer may contain significantly more data than this count, since reads occur in
    /// chunks and may include data beyond the delimiter.
    ///
    /// # Examples
    ///
    /// ```
    /// # use strata_reader::buffer::Buffer;
    /// # use std::io::Cursor;
    /// let mut buffer = Buffer::new();
    /// let mut reader = Cursor::new("Hello\nWorld");
    /// let count = buffer.fill_until(&mut reader, '\n').unwrap();
    /// assert_eq!(count, 6);  // "Hello\n" (includes delimiter)
    /// assert_eq!(buffer.as_str().unwrap(), "Hello\nWorld");
    /// ```
    ///
    /// # Errors
    ///
    /// Returns any I/O errors encountered while reading, or UTF-8 validation errors.
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "The invariant makes it safe"
    )]
    pub fn fill_until(&mut self, reader: impl Read, delimiter: char) -> io::Result<usize> {
        let valid_count = self.fill_while(reader, |c| c != delimiter)?;

        // Check if we stopped at the delimiter (not EOF)
        // Align to the char boundary first
        let check_start = self.align_pos_to_next_char(self.pos);
        let string = &self.as_str_from(check_start + valid_count)?;
        if let Some(ch) = string.chars().next() {
            if ch == delimiter {
                // Include the delimiter
                return Ok(valid_count + ch.len_utf8());
            }
        }

        // Hit EOF before finding delimiter
        Ok(valid_count)
    }
}

impl Default for Buffer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "Okay in tests"
)]
mod tests {
    use std::io::Cursor;

    use crate::constants::CHUNK_SIZE;

    use super::*;

    #[test]
    fn test_create_new() {
        let buffer = Buffer::new();

        assert_eq!(buffer.cap(), CHUNK_SIZE);
        assert_eq!(buffer.len(), 0);
        assert_eq!(buffer.pos(), 0);
    }

    #[test]
    fn test_create_default() {
        let buffer = Buffer::default();

        assert_eq!(buffer.cap(), CHUNK_SIZE);
        assert_eq!(buffer.len(), 0);
        assert_eq!(buffer.pos(), 0);
    }

    #[test]
    fn test_create_with_capacity() {
        let buffer = Buffer::with_capacity(2 * CHUNK_SIZE - 1);

        assert_eq!(buffer.cap(), 2 * CHUNK_SIZE);
        assert_eq!(buffer.len(), 0);
        assert_eq!(buffer.pos(), 0);
    }

    #[test]
    fn test_buf() {
        let mut buffer = Buffer::new();

        // Empty buffer returns empty slice
        assert_eq!(buffer.buf(), &[]);

        // Inject some data
        buffer.buf[0..5].copy_from_slice(b"Hello");
        buffer.len = 5;

        // Now buf() returns the data
        assert_eq!(buffer.buf(), b"Hello");
    }

    #[test]
    fn test_cap() {
        let mut buffer = Buffer::new();

        assert_eq!(buffer.cap(), CHUNK_SIZE);

        // Simulate a bigger capacity
        buffer.cap = 2 * CHUNK_SIZE;

        assert_eq!(buffer.cap(), 2 * CHUNK_SIZE);
    }

    #[test]
    fn test_len() {
        let mut buffer = Buffer::new();

        assert_eq!(buffer.len(), 0);

        // Simulate existing data.
        buffer.len = 50;

        assert_eq!(buffer.len(), 50);
    }

    #[test]
    fn test_pos() {
        let mut buffer = Buffer::new();

        assert_eq!(buffer.pos(), 0);

        // Simulate consumed data.
        buffer.pos = 50;

        // technically the invariant is now broken, but it doesn't matter for the test
        assert_eq!(buffer.pos(), 50);
    }

    #[test]
    fn test_is_empty() {
        let mut buffer = Buffer::new();

        assert!(buffer.is_empty());

        // Simulate existing data.
        buffer.len = 50;

        assert!(!buffer.is_empty());
    }

    #[test]
    fn test_discard() {
        let mut buffer = Buffer::new();

        // Simulate there being data.
        buffer.pos = 10;
        buffer.len = 100;

        // Discard current data.
        buffer.discard();

        // Buffer should be blank now.
        assert_eq!(buffer.len, 0);
        assert_eq!(buffer.pos, 0);
    }

    #[test]
    fn test_consume() {
        let mut buffer = Buffer::new();

        // Consume nothing or try to consume on empty
        buffer.consume(0);
        buffer.consume(10);

        // Should still be empty
        assert_eq!(buffer.len, 0);
        assert_eq!(buffer.pos, 0);

        // Simulate there being data
        buffer.len = 100;

        // Consume some bytes
        buffer.consume(10);

        // Buffer state should match
        assert_eq!(buffer.len, 100);
        assert_eq!(buffer.pos, 10);

        // Consume the remaining bytes
        buffer.consume(90);

        // Buffer state should match
        assert_eq!(buffer.len, 100);
        assert_eq!(buffer.pos, 100);

        // Simulate there being more data
        buffer.len = 150;

        // Consume more bytes than there are
        buffer.consume(100);

        // Buffer state should match
        assert_eq!(buffer.len, 150);
        assert_eq!(buffer.pos, 150);
    }

    #[test]
    fn test_compact() {
        let mut buffer = Buffer::new();

        // Compact should be a no-op
        buffer.compact();

        assert_eq!(buffer.len, 0);
        assert_eq!(buffer.pos, 0);

        // Simulate there being data.
        buffer.pos = 10;
        buffer.len = 100;

        // Compact the data.
        buffer.compact();

        // Buffer state should match
        assert_eq!(buffer.len, 90);
        assert_eq!(buffer.pos, 0);

        // Simulate all data being consumed (pos == len)
        buffer.pos = 90;

        // Compact the data
        buffer.compact();

        // Should result in empty buffer
        assert_eq!(buffer.len, 0);
        assert_eq!(buffer.pos, 0);
    }

    #[test]
    fn test_cap_down() {
        assert_eq!(Buffer::cap_down(0), CHUNK_SIZE);
        assert_eq!(Buffer::cap_down(CHUNK_SIZE - 1), CHUNK_SIZE);
        assert_eq!(Buffer::cap_down(CHUNK_SIZE), CHUNK_SIZE);
        assert_eq!(Buffer::cap_down(CHUNK_SIZE + 1), CHUNK_SIZE);
        assert_eq!(Buffer::cap_down(2 * CHUNK_SIZE + 1), 2 * CHUNK_SIZE);
        assert_eq!(Buffer::cap_down(2 * CHUNK_SIZE), 2 * CHUNK_SIZE);
        assert_eq!(Buffer::cap_down(8 * CHUNK_SIZE + 1), 8 * CHUNK_SIZE);
        assert_eq!(Buffer::cap_down(32 * CHUNK_SIZE + 1), 32 * CHUNK_SIZE);
        assert_eq!(Buffer::cap_down(128 * CHUNK_SIZE + 1), 128 * CHUNK_SIZE);
        assert_eq!(Buffer::cap_down(usize::MAX), PRACTICAL_MAX_SIZE);
    }

    #[test]
    fn test_cap_up() {
        assert_eq!(Buffer::cap_up(0), CHUNK_SIZE);
        assert_eq!(Buffer::cap_up(CHUNK_SIZE - 1), CHUNK_SIZE);
        assert_eq!(Buffer::cap_up(CHUNK_SIZE), CHUNK_SIZE);
        assert_eq!(Buffer::cap_up(CHUNK_SIZE + 1), 2 * CHUNK_SIZE);
        assert_eq!(Buffer::cap_up(2 * CHUNK_SIZE - 1), 2 * CHUNK_SIZE);
        assert_eq!(Buffer::cap_up(2 * CHUNK_SIZE), 2 * CHUNK_SIZE);
        assert_eq!(Buffer::cap_up(8 * CHUNK_SIZE - 1), 8 * CHUNK_SIZE);
        assert_eq!(Buffer::cap_up(32 * CHUNK_SIZE - 1), 32 * CHUNK_SIZE);
        assert_eq!(Buffer::cap_up(128 * CHUNK_SIZE - 1), 128 * CHUNK_SIZE);
        assert_eq!(Buffer::cap_up(usize::MAX), PRACTICAL_MAX_SIZE);
    }

    #[test]
    fn test_grow() {
        let mut buffer = Buffer::new();

        buffer.grow();
        assert_eq!(buffer.cap(), 2 * CHUNK_SIZE);
        assert!(buffer.buf.capacity() >= buffer.cap());
        assert!(buffer.buf.capacity() <= buffer.cap() + CHUNK_SIZE);

        buffer.grow();
        assert_eq!(buffer.cap(), 4 * CHUNK_SIZE);
        assert!(buffer.buf.capacity() >= buffer.cap());
        assert!(buffer.buf.capacity() <= buffer.cap() + CHUNK_SIZE);

        buffer.grow();
        assert_eq!(buffer.cap(), 8 * CHUNK_SIZE);
        assert!(buffer.buf.capacity() >= buffer.cap());
        assert!(buffer.buf.capacity() <= buffer.cap() + CHUNK_SIZE);

        buffer.grow();
        assert_eq!(buffer.cap(), 16 * CHUNK_SIZE);
        assert!(buffer.buf.capacity() >= buffer.cap());
        assert!(buffer.buf.capacity() <= buffer.cap() + CHUNK_SIZE);

        buffer.grow();
        assert_eq!(buffer.cap(), 32 * CHUNK_SIZE);
        assert!(buffer.buf.capacity() >= buffer.cap());
        assert!(buffer.buf.capacity() <= buffer.cap() + CHUNK_SIZE);
    }

    #[test]
    fn test_shrink() {
        let mut buffer = Buffer::with_capacity(32 * CHUNK_SIZE);

        buffer.len = 15 * CHUNK_SIZE + 1;
        buffer.shrink();
        assert_eq!(buffer.cap(), 16 * CHUNK_SIZE);
        assert!(buffer.buf.capacity() >= buffer.cap());
        assert!(buffer.buf.capacity() <= buffer.cap() + CHUNK_SIZE);

        buffer.len = 7 * CHUNK_SIZE + 1;
        buffer.shrink();
        assert_eq!(buffer.cap(), 8 * CHUNK_SIZE);
        assert!(buffer.buf.capacity() >= buffer.cap());
        assert!(buffer.buf.capacity() <= buffer.cap() + CHUNK_SIZE);

        buffer.len = 6 * CHUNK_SIZE - 1;
        buffer.shrink();
        assert_eq!(buffer.cap(), 6 * CHUNK_SIZE);
        assert!(buffer.buf.capacity() >= buffer.cap());
        assert!(buffer.buf.capacity() <= buffer.cap() + CHUNK_SIZE);

        buffer.len = 3 * CHUNK_SIZE + 1;
        buffer.shrink();
        assert_eq!(buffer.cap(), 4 * CHUNK_SIZE);
        assert!(buffer.buf.capacity() >= buffer.cap());
        assert!(buffer.buf.capacity() <= buffer.cap() + CHUNK_SIZE);

        buffer.len = 2 * CHUNK_SIZE + 1;
        buffer.shrink();
        assert_eq!(buffer.cap(), 3 * CHUNK_SIZE);
        assert!(buffer.buf.capacity() >= buffer.cap());
        assert!(buffer.buf.capacity() <= buffer.cap() + CHUNK_SIZE);

        buffer.len = 2 * CHUNK_SIZE;
        buffer.shrink();
        assert_eq!(buffer.cap(), 2 * CHUNK_SIZE);
        assert!(buffer.buf.capacity() >= buffer.cap());
        assert!(buffer.buf.capacity() <= buffer.cap() + CHUNK_SIZE);

        buffer.len = CHUNK_SIZE;
        buffer.shrink();
        assert_eq!(buffer.cap(), CHUNK_SIZE);
        assert!(buffer.buf.capacity() >= buffer.cap());
        assert!(buffer.buf.capacity() <= buffer.cap() + CHUNK_SIZE);

        buffer.len = 0;
        buffer.shrink();
        assert_eq!(buffer.cap(), CHUNK_SIZE);
        assert!(buffer.buf.capacity() >= buffer.cap());
        assert!(buffer.buf.capacity() <= buffer.cap() + CHUNK_SIZE);
    }

    #[test]
    fn test_fill() {
        let mut buffer = Buffer::new();
        let string = "Hello, World!";

        let remainder = CHUNK_SIZE % string.len();
        let fills = CHUNK_SIZE / string.len();

        assert!(remainder > 0);

        // Repeatedly fill the buffer until it's almost full
        for n in 1..=fills {
            let cur = Cursor::new(string);
            let read = buffer.fill(cur).unwrap();
            let pos = buffer.pos();
            let data = buffer.buf().get(pos..pos + read).unwrap();

            assert_eq!(data, string.as_bytes());
            assert_eq!(pos + read, n * string.len());

            buffer.consume(read);
        }

        // Fill the last bit of space
        let cur = Cursor::new(string);
        let read = buffer.fill(cur).unwrap();
        let pos = buffer.pos();
        let data = buffer.buf().get(pos..pos + read).unwrap();
        let fin = string.as_bytes().get(..remainder).unwrap();

        assert_eq!(data, fin);
        assert_eq!(buffer.len(), CHUNK_SIZE);
    }

    #[test]
    fn test_fill_amount() {
        let mut buffer = Buffer::new();
        let string = "Hello, World!";

        let amount = 3 * CHUNK_SIZE;
        let remainder = amount % string.len();
        let fills = amount / string.len();

        assert!(remainder > 0);

        let repeated = string.repeat(fills + 1);
        let cur = Cursor::new(repeated);

        // Fill the requested amount
        let total_read = buffer.fill_amount(cur, amount).unwrap();

        assert!(total_read >= amount);
        assert!(buffer.len() >= amount);
        assert!(buffer.cap() >= amount);

        // Verify the data is correct
        for _ in 0..fills {
            let pos = buffer.pos();
            let data = buffer.buf().get(pos..pos + string.len()).unwrap();

            assert_eq!(data, string.as_bytes());

            buffer.consume(string.len());
        }

        // Verify the last bit of data
        let pos = buffer.pos();
        let data = buffer.buf().get(pos..pos + remainder).unwrap();
        let fin = string.as_bytes().get(..remainder).unwrap();
        assert_eq!(data, fin);
    }

    #[test]
    fn test_as_str_valid_ascii() {
        let mut buffer = Buffer::new();
        let text = "Hello, World!";
        let cur = Cursor::new(text);
        buffer.fill(cur).unwrap();
        assert_eq!(buffer.as_str().unwrap(), text);
    }

    #[test]
    fn test_as_str_valid_multibyte() {
        let mut buffer = Buffer::new();
        let text = "Hello, 世界!";
        let cur = Cursor::new(text);
        buffer.fill(cur).unwrap();
        assert_eq!(buffer.as_str().unwrap(), text);
    }

    #[test]
    fn test_as_str_partial_utf8_at_start() {
        let mut buffer = Buffer::new();
        let text = "Hello, 世界!";
        let cur = Cursor::new(text);
        buffer.fill(cur).unwrap();
        // Use as_str_from with offset in the middle of "世" (3-byte UTF-8 character)
        let result = buffer.as_str_from(8).unwrap();
        assert_eq!(result, "界!");
    }

    #[test]
    fn test_as_str_partial_utf8_at_end() {
        let mut buffer = Buffer::new();
        let text = "Hello, 世界";
        let bytes = text.as_bytes();
        let truncated = bytes.len() - 1;
        let cur = Cursor::new(&bytes[0..truncated]);
        buffer.fill(cur).unwrap();
        let result = buffer.as_str().unwrap();
        assert_eq!(result, "Hello, 世");
    }

    #[test]
    fn test_as_str_invalid_utf8_middle() {
        let mut buffer = Buffer::new();
        buffer.buf[0..5].copy_from_slice(b"Hello");
        // Continuation byte without start
        buffer.buf[5] = 0b1000_0000;
        buffer.buf[6..12].copy_from_slice(b"World!");
        buffer.len = 12;
        assert!(buffer.as_str().is_err());
    }

    #[test]
    fn test_as_str_empty() {
        let buffer = Buffer::new();
        assert_eq!(buffer.as_str().unwrap(), "");
    }

    #[test]
    fn test_as_str_all_consumed() {
        let mut buffer = Buffer::new();
        let text = "Hello";
        let cur = Cursor::new(text);
        buffer.fill(cur).unwrap();
        // Use as_str_from with offset at the end
        let result = buffer.as_str_from(5).unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_align_pos_to_char_on_char_boundary() {
        let mut buffer = Buffer::new();
        let text = "Hello, 世界!";
        let cur = Cursor::new(text);
        buffer.fill(cur).unwrap();
        // Position 7 is on a character boundary (start of '世')
        let aligned = buffer.align_pos_to_char(7);
        assert_eq!(aligned, 7);
    }

    #[test]
    fn test_align_pos_to_char_middle_of_multibyte() {
        let mut buffer = Buffer::new();
        let text = "Hello, 世界!";
        let cur = Cursor::new(text);
        buffer.fill(cur).unwrap();
        // "Hello, " = 7 bytes, '世' is 3 bytes (7-9), '界' is 3 bytes (10-12)
        // Position 8 is in the middle of '世', should align to start of '世' at 7
        let aligned = buffer.align_pos_to_char(8);
        assert_eq!(aligned, 7);
    }

    #[test]
    fn test_align_pos_to_char_beyond_len() {
        let mut buffer = Buffer::new();
        let text = "Hello";
        let cur = Cursor::new(text);
        buffer.fill(cur).unwrap();
        // Position beyond len should clamp to len
        let aligned = buffer.align_pos_to_char(100);
        assert_eq!(aligned, 5);
    }

    #[test]
    fn test_align_pos_to_char_before_pos() {
        let mut buffer = Buffer::new();
        let text = "Hello, World!";
        let cur = Cursor::new(text);
        buffer.fill(cur).unwrap();
        buffer.consume(5);
        // Trying to align before current pos should clamp to pos
        let aligned = buffer.align_pos_to_char(2);
        assert_eq!(aligned, 5);
    }

    #[test]
    fn test_align_pos_to_next_char_on_char_boundary() {
        let mut buffer = Buffer::new();
        let text = "Hello, 世界!";
        let cur = Cursor::new(text);
        buffer.fill(cur).unwrap();
        // Position 7 is on a character boundary (start of '世')
        let aligned = buffer.align_pos_to_next_char(7);
        assert_eq!(aligned, 7);
    }

    #[test]
    fn test_align_pos_to_next_char_middle_of_multibyte() {
        let mut buffer = Buffer::new();
        let text = "Hello, 世界!";
        let cur = Cursor::new(text);
        buffer.fill(cur).unwrap();
        // "Hello, " = 7 bytes, '世' is 3 bytes (7-9), '界' is 3 bytes (10-12)
        // Position 8 is in the middle of '世', should align to start of '界' at 10
        let aligned = buffer.align_pos_to_next_char(8);
        assert_eq!(aligned, 10);
    }

    #[test]
    fn test_align_pos_to_next_char_beyond_len() {
        let mut buffer = Buffer::new();
        let text = "Hello";
        let cur = Cursor::new(text);
        buffer.fill(cur).unwrap();
        // Position beyond len should clamp to len
        let aligned = buffer.align_pos_to_next_char(100);
        assert_eq!(aligned, 5);
    }

    #[test]
    fn test_align_pos_to_next_char_before_pos() {
        let mut buffer = Buffer::new();
        let text = "Hello, World!";
        let cur = Cursor::new(text);
        buffer.fill(cur).unwrap();
        buffer.consume(5);
        // Trying to align before current pos should clamp to pos
        let aligned = buffer.align_pos_to_next_char(2);
        assert_eq!(aligned, 5);
    }

    #[test]
    fn test_fill_while_basic() {
        let mut buffer = Buffer::new();
        let text = "aaaaaHello";
        let cur = Cursor::new(text);

        let read = buffer.fill_while(cur, |c| c == 'a').unwrap();

        assert_eq!(read, 5);
        assert_eq!(buffer.len(), text.len());
        assert_eq!(buffer.cap(), CHUNK_SIZE);
        assert_eq!(buffer.as_str().unwrap(), text);
    }

    #[test]
    fn test_fill_while_all_match() {
        let mut buffer = Buffer::new();
        let text = "aaaaaaaaa";
        let cur = Cursor::new(text);

        let read = buffer.fill_while(cur, |c| c == 'a').unwrap();

        assert_eq!(read, text.len());
        assert_eq!(buffer.len(), text.len());
        assert_eq!(buffer.cap(), CHUNK_SIZE);
        assert_eq!(buffer.as_str().unwrap(), text);
    }

    #[test]
    fn test_fill_while_multibyte_chars() {
        let mut buffer = Buffer::new();
        let text = "世界世界ABC";
        let cur = Cursor::new(text);

        let read = buffer.fill_while(cur, |c| c == '世' || c == '界').unwrap();

        // Should read 4 chars * 3 bytes each = 12 bytes
        assert_eq!(read, 12);
        assert_eq!(buffer.len(), text.len());
        assert_eq!(buffer.cap(), CHUNK_SIZE);
        assert_eq!(buffer.as_str().unwrap(), text);
    }

    #[test]
    fn test_fill_while_chunk_boundary() {
        let mut buffer = Buffer::new();

        // Create text that breaks exactly at CHUNK_SIZE
        let prefix = "a".repeat(CHUNK_SIZE);
        let text = format!("{prefix}b");
        let cur = Cursor::new(text);

        let read = buffer.fill_while(cur, |c| c == 'a').unwrap();

        assert_eq!(read, CHUNK_SIZE);
        assert_eq!(buffer.len(), CHUNK_SIZE + 1);
        assert_eq!(buffer.cap(), 2 * CHUNK_SIZE);
    }

    #[test]
    fn test_fill_while_chunk_boundary_plus_one() {
        let mut buffer = Buffer::new();

        // Create text that breaks at CHUNK_SIZE + 1
        let prefix = "a".repeat(CHUNK_SIZE + 1);
        let text = format!("{prefix}b");
        let cur = Cursor::new(text);

        let read = buffer.fill_while(cur, |c| c == 'a').unwrap();

        assert_eq!(read, CHUNK_SIZE + 1);
        assert_eq!(buffer.len(), CHUNK_SIZE + 2);
        assert_eq!(buffer.cap(), 2 * CHUNK_SIZE);
    }

    #[test]
    fn test_fill_while_multibyte_char_at_chunk_boundary() {
        let mut buffer = Buffer::new();

        // Create text where a multibyte char starts right at CHUNK_SIZE
        let prefix = "a".repeat(CHUNK_SIZE);
        let text = format!("{prefix}世b");
        let cur = Cursor::new(text);

        let read = buffer.fill_while(cur, |c| c == 'a' || c == '世').unwrap();

        // Should read CHUNK_SIZE 'a's + 1 '世' (3 bytes)
        assert_eq!(read, CHUNK_SIZE + 3);
        assert_eq!(buffer.len(), CHUNK_SIZE + 4); // + 'b'
        assert_eq!(buffer.cap(), 2 * CHUNK_SIZE);
    }

    #[test]
    fn test_fill_while_multibyte_char_breaks_at_chunk_boundary() {
        let mut buffer = Buffer::new();

        // Create text where a multibyte char that breaks the predicate is split at CHUNK_SIZE
        // The first byte of '世' will be at CHUNK_SIZE - 1, causing it to be split across reads
        let prefix = "a".repeat(CHUNK_SIZE - 1);
        let text = format!("{prefix}世");
        let cur = Cursor::new(text);

        let read = buffer.fill_while(cur, |c| c == 'a').unwrap();

        assert_eq!(read, CHUNK_SIZE - 1);
        assert_eq!(buffer.len(), CHUNK_SIZE + 2); // CHUNK_SIZE - 1 + 3 bytes for '世'
        assert_eq!(buffer.cap(), 2 * CHUNK_SIZE);
    }

    #[test]
    fn test_fill_while_with_existing_data() {
        let mut buffer = Buffer::new();
        let text1 = "aaaaa";
        let cur1 = Cursor::new(text1);
        buffer.fill(cur1).unwrap();
        buffer.consume(2); // Consume 2 'a's

        let text2 = "aaHello";
        let cur2 = Cursor::new(text2);

        // Should read from existing position
        let read = buffer.fill_while(cur2, |c| c == 'a').unwrap();

        // Should count the 3 remaining 'a's from first fill + 2 'a's from second fill = 5
        assert_eq!(read, 5);
        assert_eq!(buffer.as_str().unwrap(), "aaaaaHello");
    }

    #[test]
    fn test_fill_until_basic() {
        let mut buffer = Buffer::new();
        let text = "Hello\nWorld";
        let cur = Cursor::new(text);

        let read = buffer.fill_until(cur, '\n').unwrap();

        assert_eq!(read, 6); // "Hello\n"
        assert_eq!(buffer.len(), text.len());
        assert_eq!(buffer.cap(), CHUNK_SIZE);
        assert_eq!(buffer.as_str().unwrap(), text);
    }

    #[test]
    fn test_fill_until_not_found() {
        let mut buffer = Buffer::new();
        let text = "Hello World";
        let cur = Cursor::new(text);

        let read = buffer.fill_until(cur, '\n').unwrap();

        assert_eq!(read, text.len());
        assert_eq!(buffer.len(), text.len());
        assert_eq!(buffer.cap(), CHUNK_SIZE);
        assert_eq!(buffer.as_str().unwrap(), text);
    }

    #[test]
    fn test_fill_until_multibyte_delimiter() {
        let mut buffer = Buffer::new();
        let text = "Hello世World";
        let cur = Cursor::new(text);

        let read = buffer.fill_until(cur, '世').unwrap();

        // "Hello" (5 bytes) + "世" (3 bytes)
        assert_eq!(read, 8);
        assert_eq!(buffer.len(), text.len());
        assert_eq!(buffer.cap(), CHUNK_SIZE);
        assert_eq!(buffer.as_str().unwrap(), text);
    }

    #[test]
    fn test_fill_until_chunk_boundary() {
        let mut buffer = Buffer::new();

        // Create text where delimiter is exactly at CHUNK_SIZE
        let prefix = "a".repeat(CHUNK_SIZE);
        let text = format!("{prefix}\n");
        let cur = Cursor::new(text);

        let read = buffer.fill_until(cur, '\n').unwrap();

        assert_eq!(read, CHUNK_SIZE + 1); // includes the newline
        assert_eq!(buffer.len(), CHUNK_SIZE + 1);
        assert_eq!(buffer.cap(), 2 * CHUNK_SIZE);
    }

    #[test]
    fn test_fill_until_chunk_boundary_plus_one() {
        let mut buffer = Buffer::new();

        // Create text where delimiter is at CHUNK_SIZE + 1
        let prefix = "a".repeat(CHUNK_SIZE + 1);
        let text = format!("{prefix}\n");
        let cur = Cursor::new(text);

        let read = buffer.fill_until(cur, '\n').unwrap();

        assert_eq!(read, CHUNK_SIZE + 2); // includes the newline
        assert_eq!(buffer.len(), CHUNK_SIZE + 2);
        assert_eq!(buffer.cap(), 2 * CHUNK_SIZE);
    }

    #[test]
    fn test_fill_until_multibyte_delimiter_at_chunk_boundary() {
        let mut buffer = Buffer::new();

        // Create text where multibyte delimiter starts at CHUNK_SIZE
        let prefix = "a".repeat(CHUNK_SIZE);
        let text = format!("{prefix}世");
        let cur = Cursor::new(text);

        let read = buffer.fill_until(cur, '世').unwrap();

        // CHUNK_SIZE 'a's + '世' (3 bytes)
        assert_eq!(read, CHUNK_SIZE + 3);
        assert_eq!(buffer.len(), CHUNK_SIZE + 3);
        assert_eq!(buffer.cap(), 2 * CHUNK_SIZE);
    }

    #[test]
    fn test_fill_until_multibyte_delimiter_breaks_at_chunk_boundary() {
        let mut buffer = Buffer::new();

        // Create text where a multibyte delimiter is split at CHUNK_SIZE
        // The first byte of '世' will be at CHUNK_SIZE - 1, causing it to be split across reads
        let prefix = "a".repeat(CHUNK_SIZE - 1);
        let text = format!("{prefix}世");
        let cur = Cursor::new(text);

        let read = buffer.fill_until(cur, '世').unwrap();

        // CHUNK_SIZE - 1 'a's + '世' (3 bytes)
        assert_eq!(read, CHUNK_SIZE - 1 + 3);
        assert_eq!(buffer.len(), CHUNK_SIZE + 2); // CHUNK_SIZE - 1 + 3 bytes for '世'
        assert_eq!(buffer.cap(), 2 * CHUNK_SIZE);
    }

    #[test]
    fn test_fill_until_with_existing_data() {
        let mut buffer = Buffer::new();
        let text1 = "Hello";
        let cur1 = Cursor::new(text1);
        buffer.fill(cur1).unwrap();
        buffer.consume(2); // Consume "He"

        let text2 = "llo\nWorld";
        let cur2 = Cursor::new(text2);

        // Should read from existing position and find delimiter
        let read = buffer.fill_until(cur2, '\n').unwrap();

        // Should count remaining "llo" (3) + new "llo\n" (4) = 7
        assert_eq!(read, 7);
        assert_eq!(buffer.as_str().unwrap(), "llollo\nWorld"); // spellchecker:ignore llollo
    }

    #[test]
    fn test_fill_until_immediate_delimiter() {
        let mut buffer = Buffer::new();
        let text = "\nWorld";
        let cur = Cursor::new(text);

        let read = buffer.fill_until(cur, '\n').unwrap();

        // Should find delimiter immediately
        assert_eq!(read, 1);
        assert_eq!(buffer.as_str().unwrap(), "\nWorld");
    }

    // Mock reader that returns an error after reading a specified number of bytes
    struct ErrorReader {
        data: Vec<u8>,
        pos: usize,
        error_at: usize,
    }

    impl ErrorReader {
        fn new(data: Vec<u8>, error_at: usize) -> Self {
            Self {
                data,
                pos: 0,
                error_at,
            }
        }
    }

    impl Read for ErrorReader {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.pos >= self.error_at {
                return Err(io::Error::other("simulated read error"));
            }

            let remaining = self.data.len() - self.pos;
            let to_read = cmp::min(buf.len(), remaining);
            let to_read = cmp::min(to_read, self.error_at - self.pos);

            if to_read == 0 {
                return Ok(0);
            }

            buf[..to_read].copy_from_slice(&self.data[self.pos..self.pos + to_read]);
            self.pos += to_read;
            Ok(to_read)
        }
    }

    #[test]
    fn test_fill_error() {
        let mut buffer = Buffer::new();
        let data = b"Hello, World!".to_vec();
        let mut reader = ErrorReader::new(data.clone(), 5);

        // First read succeeds with 5 bytes
        let result = buffer.fill(&mut reader);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 5);
        assert_eq!(buffer.len(), 5);
        assert_eq!(&buffer.buf()[..5], b"Hello");

        // Second read should error
        let result = buffer.fill(&mut reader);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Other);
        assert_eq!(buffer.len(), 5); // Length unchanged after error
    }

    #[test]
    fn test_fill_error_immediate() {
        let mut buffer = Buffer::new();
        let data = b"Hello, World!".to_vec();
        let mut reader = ErrorReader::new(data, 0);

        // Should error immediately
        let result = buffer.fill(&mut reader);

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Other);
        assert_eq!(buffer.len(), 0);
    }

    #[test]
    fn test_fill_amount_error() {
        let mut buffer = Buffer::new();
        let data = b"Hello, World!".to_vec();
        let mut reader = ErrorReader::new(data.clone(), 5);

        // Request more than what we can read before error
        let result = buffer.fill_amount(&mut reader, 100);

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Other);
        assert_eq!(buffer.len(), 5);
        assert_eq!(&buffer.buf()[..5], b"Hello");
    }

    #[test]
    fn test_fill_amount_error_immediate() {
        let mut buffer = Buffer::new();
        let data = b"Hello, World!".to_vec();
        let mut reader = ErrorReader::new(data, 0);

        // Should error immediately
        let result = buffer.fill_amount(&mut reader, 100);

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Other);
        assert_eq!(buffer.len(), 0);
    }

    #[test]
    fn test_fill_amount_error_after_growth() {
        let mut buffer = Buffer::new();
        let data = "a".repeat(2 * CHUNK_SIZE);
        let error_at = CHUNK_SIZE + 10;
        let mut reader = ErrorReader::new(data.into_bytes(), error_at);

        // Request enough to trigger growth, then error
        let result = buffer.fill_amount(&mut reader, 2 * CHUNK_SIZE);

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Other);
        assert_eq!(buffer.len(), error_at);
    }

    #[test]
    fn test_fill_while_error() {
        let mut buffer = Buffer::new();
        let data = "aaaaaHello".to_string();
        let mut reader = ErrorReader::new(data.into_bytes(), 3);

        // Should read 3 bytes then error
        let result = buffer.fill_while(&mut reader, |c| c == 'a');

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Other);
        assert_eq!(buffer.len(), 3);
        assert_eq!(buffer.as_str().unwrap(), "aaa");
    }

    #[test]
    fn test_fill_while_error_immediate() {
        let mut buffer = Buffer::new();
        let data = "aaaaaHello".to_string();
        let mut reader = ErrorReader::new(data.into_bytes(), 0);

        // Should error immediately
        let result = buffer.fill_while(&mut reader, |c| c == 'a');

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Other);
        assert_eq!(buffer.len(), 0);
    }

    #[test]
    fn test_fill_while_error_across_chunk_boundary() {
        let mut buffer = Buffer::new();
        let prefix = "a".repeat(CHUNK_SIZE + 10);
        let text = format!("{prefix}b");
        let error_at = CHUNK_SIZE + 5;
        let mut reader = ErrorReader::new(text.into_bytes(), error_at);

        // Should read past chunk boundary then error
        let result = buffer.fill_while(&mut reader, |c| c == 'a');

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Other);
        assert_eq!(buffer.len(), error_at);
        assert!(buffer.cap() >= 2 * CHUNK_SIZE);
    }

    #[test]
    fn test_fill_while_error_before_predicate_breaks() {
        let mut buffer = Buffer::new();
        let data = "aaaaaHello".to_string();
        let mut reader = ErrorReader::new(data.into_bytes(), 5);

        // Should read 5 'a's (matching predicate), then error on next read
        let result = buffer.fill_while(&mut reader, |c| c == 'a');

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Other);
        assert_eq!(buffer.len(), 5);
        assert_eq!(buffer.as_str().unwrap(), "aaaaa");
    }

    #[test]
    fn test_fill_until_error() {
        let mut buffer = Buffer::new();
        let data = "Hello\nWorld".to_string();
        let mut reader = ErrorReader::new(data.into_bytes(), 3);

        // Should read 3 bytes then error
        let result = buffer.fill_until(&mut reader, '\n');

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Other);
        assert_eq!(buffer.len(), 3);
        assert_eq!(buffer.as_str().unwrap(), "Hel");
    }

    #[test]
    fn test_fill_until_error_immediate() {
        let mut buffer = Buffer::new();
        let data = "Hello\nWorld".to_string();
        let mut reader = ErrorReader::new(data.into_bytes(), 0);

        // Should error immediately
        let result = buffer.fill_until(&mut reader, '\n');

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Other);
        assert_eq!(buffer.len(), 0);
    }

    #[test]
    fn test_fill_until_error_before_delimiter() {
        let mut buffer = Buffer::new();
        let data = "Hello\nWorld".to_string();
        let mut reader = ErrorReader::new(data.into_bytes(), 5);

        // Should read 5 bytes (before '\n'), then error
        let result = buffer.fill_until(&mut reader, '\n');

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Other);
        assert_eq!(buffer.len(), 5);
        assert_eq!(buffer.as_str().unwrap(), "Hello");
    }

    #[test]
    fn test_fill_until_error_across_chunk_boundary() {
        let mut buffer = Buffer::new();
        let prefix = "a".repeat(CHUNK_SIZE + 10);
        let text = format!("{prefix}\n");
        let error_at = CHUNK_SIZE + 5;
        let mut reader = ErrorReader::new(text.into_bytes(), error_at);

        // Should read past chunk boundary then error
        let result = buffer.fill_until(&mut reader, '\n');

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Other);
        assert_eq!(buffer.len(), error_at);
        assert!(buffer.cap() >= 2 * CHUNK_SIZE);
    }

    #[test]
    fn test_fill_until_error_with_multibyte_delimiter() {
        let mut buffer = Buffer::new();
        let data = "Hello世World".to_string();
        let mut reader = ErrorReader::new(data.into_bytes(), 7);

        // Should read 7 bytes (before '世'), then error
        let result = buffer.fill_until(&mut reader, '世');

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Other);
        assert_eq!(buffer.len(), 7);
        assert_eq!(buffer.as_str().unwrap(), "Hello");
    }
}
