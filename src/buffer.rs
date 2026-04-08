//! Buffer with dynamic capacity management.
//!
//! The [`Buffer`] type provides standalone buffered I/O operations with both manual and automatic
//! growth and shrinking of its capacity. It is used as the internal buffer for
//! [`Reader`](crate::Reader).
//!
//! # Example
//!
//! ```
//! use strata_reader::buffer::Buffer;
//! use std::io::Cursor;
//!
//! let mut buffer = Buffer::new();
//! let mut reader = Cursor::new(b"Hello, World!");
//!
//! // Fill the buffer from a reader
//! buffer.fill(&mut reader).unwrap();
//!
//! // Inspect the buffered data
//! let data = buffer.buf();
//! assert_eq!(data, b"Hello, World!");
//!
//! // Find the start of the second word and consume past it
//! let offset = data.iter().position(|&b| b == b' ').unwrap() + 1;
//! buffer.consume(offset);
//! assert_eq!(&buffer.buf()[buffer.pos()..], b"World!");
//! ```

use crate::constants::{CHUNK_SIZE, MAX_EXPONENTIAL_CAPACITY, MAX_SUPPORTED_CAPACITY};
use std::cmp;
use std::io::{self, Read};

/// Result type for bounded fill operations.
///
/// The contained byte count represents the total bytes read from the reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillResult {
    /// The requested fill completed in full without reaching EOF.
    ///
    /// Contains the total bytes read.
    Complete(usize),

    /// The reader reached end-of-file before the fill could complete.
    ///
    /// Contains the total bytes read before EOF.
    Eof(usize),
}

impl FillResult {
    /// Returns the byte count, regardless of completion status.
    ///
    /// # Examples
    ///
    /// ```
    /// # use strata_reader::buffer::FillResult;
    /// assert_eq!(FillResult::Complete(42).count(), 42);
    /// assert_eq!(FillResult::Eof(10).count(), 10);
    /// ```
    pub const fn count(&self) -> usize {
        match self {
            Self::Complete(n) | Self::Eof(n) => *n,
        }
    }
}

/// Result type for unbounded fill operations.
///
/// The contained byte count represents the total bytes read from the reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnboundedFillResult {
    /// The requested fill completed in full without reaching EOF or a growth limit.
    ///
    /// Contains the total bytes read.
    Complete(usize),

    /// The reader reached end-of-file before the fill could complete.
    ///
    /// Contains the total bytes read before EOF.
    Eof(usize),

    /// The operation was capped before it could complete.
    ///
    /// Contains the total bytes read before the limit was reached.
    Capped(usize),
}

impl UnboundedFillResult {
    /// Returns the byte count, regardless of completion status.
    ///
    /// # Examples
    ///
    /// ```
    /// # use strata_reader::buffer::UnboundedFillResult;
    /// assert_eq!(UnboundedFillResult::Complete(42).count(), 42);
    /// assert_eq!(UnboundedFillResult::Eof(10).count(), 10);
    /// assert_eq!(UnboundedFillResult::Capped(100).count(), 100);
    /// ```
    pub const fn count(&self) -> usize {
        match self {
            Self::Complete(n) | Self::Eof(n) | Self::Capped(n) => *n,
        }
    }
}

impl From<FillResult> for UnboundedFillResult {
    fn from(result: FillResult) -> Self {
        match result {
            FillResult::Complete(n) => UnboundedFillResult::Complete(n),
            FillResult::Eof(n) => UnboundedFillResult::Eof(n),
        }
    }
}

/// Result of a single grow-and-read cycle performed by [`Buffer::read_once`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReadOnce {
    /// Successfully read bytes.
    ///
    /// `Buffer::len` and `total_bytes_read` have been updated.
    Read,
    /// The reader returned 0 bytes.
    Eof,
    /// The buffer is full and isn't allowed to grow further.
    Capped,
}

/// A dynamically sized buffer with chunked capacity management.
///
/// Provides buffered I/O with automatic capacity growth. Used as the internal buffer for
/// [`Reader`](crate::Reader).
///
/// # Capacity Management
///
/// All capacities are multiples of [`CHUNK_SIZE`].
/// - **Growth**: Automatic and exponential, rounds up to power-of-two multiples of `CHUNK_SIZE`.
/// - **Shrinking**: Manual and linear, shrinks to the smallest `CHUNK_SIZE` multiple that can
///   hold the retained data.
///
/// # Invariants
///
/// This buffer maintains the invariant `0 <= self.pos <= self.len <= self.cap == self.buf.len() <=
/// self.buf.capacity()` at all times, ensuring memory safety and correctness of all operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Buffer {
    /// Internal buffer storage.
    buf: Vec<u8>,
    /// Logical capacity of the buffer.
    ///
    /// May be slightly less than `buf.capacity()`.
    cap: usize,
    /// Number of bytes currently stored in the buffer.
    len: usize,
    /// Number of bytes that have been consumed (read position).
    pos: usize,
}

impl Default for Buffer {
    fn default() -> Self {
        Self::new()
    }
}

impl Buffer {
    /// Creates a new buffer with the default capacity.
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
    /// The requested capacity is rounded up to the nearest [`CHUNK_SIZE`] multiple that can
    /// accommodate it. Extremely large requests saturate at the largest capacity supported by the
    /// implementation.
    ///
    /// # Examples
    ///
    /// ```
    /// # use strata_reader::buffer::Buffer;
    /// # use strata_reader::constants::CHUNK_SIZE;
    /// let buffer = Buffer::with_capacity(100);
    /// assert!(buffer.cap() >= 100);
    /// assert_eq!(buffer.cap() % CHUNK_SIZE, 0); // Aligned to CHUNK_SIZE
    ///
    /// // Gets exact multiple, not power-of-two
    /// let buffer = Buffer::with_capacity(5 * CHUNK_SIZE);
    /// assert_eq!(buffer.cap(), 5 * CHUNK_SIZE);
    /// ```
    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        // Round up to fit the requested capacity (linear, not exponential)
        let cap = Self::cap_up_linear(capacity);

        Self {
            buf: vec![0; cap],
            cap,
            len: 0,
            pos: 0,
        }
    }

    /// Returns a slice to the content in the internal buffer.
    ///
    /// This slice includes both consumed and unconsumed data, use with [`pos()`](Self::pos) to
    /// access either unconsumed (`pos()..`) or consumed (`..pos()`) data as needed.
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
    #[expect(clippy::indexing_slicing, reason = "Safe by invariant")]
    #[inline]
    pub fn buf(&self) -> &[u8] {
        &self.buf[..self.len]
    }

    /// Returns the current capacity of the buffer in bytes.
    ///
    /// This is always a multiple of [`CHUNK_SIZE`].
    #[inline]
    pub fn cap(&self) -> usize {
        self.cap
    }

    /// Returns the number of bytes currently in the buffer.
    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns `true` if the buffer contains no data.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Returns the current read position (number of consumed bytes).
    #[inline]
    pub fn pos(&self) -> usize {
        self.pos
    }

    /// Clears all data in the buffer.
    ///
    /// Resets both the read position and length to zero, but keeps the current buffer capacity
    /// unchanged.
    ///
    /// # Examples
    ///
    /// ```
    /// # use strata_reader::{buffer::Buffer, constants::CHUNK_SIZE};
    /// # use std::io::Cursor;
    /// let mut buffer = Buffer::with_capacity(8 * CHUNK_SIZE);
    /// buffer.fill(Cursor::new(b"data")).unwrap();
    /// buffer.clear();
    /// assert_eq!(buffer.len(), 0);
    /// assert_eq!(buffer.pos(), 0);
    /// assert_eq!(buffer.cap(), 8 * CHUNK_SIZE);
    /// ```
    #[inline]
    pub fn clear(&mut self) {
        self.pos = 0;
        self.len = 0;
    }

    /// Discards all data and excess capacity.
    ///
    /// Resets both the read position and length to zero, then shrinks the buffer to the minimum
    /// capacity.
    ///
    /// # Examples
    ///
    /// ```
    /// # use strata_reader::{buffer::Buffer, constants::CHUNK_SIZE};
    /// # use std::io::Cursor;
    /// let mut buffer = Buffer::with_capacity(8 * CHUNK_SIZE);
    /// buffer.fill(Cursor::new(b"data")).unwrap();
    /// buffer.discard();
    /// assert_eq!(buffer.len(), 0);
    /// assert_eq!(buffer.pos(), 0);
    /// assert_eq!(buffer.cap(), CHUNK_SIZE);
    /// ```
    #[inline]
    pub fn discard(&mut self) {
        self.clear();
        self.shrink();
    }

    /// Marks `amt` bytes as consumed, moving the read position forwards.
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
    #[expect(clippy::arithmetic_side_effects, reason = "Safe by invariant")]
    #[inline]
    pub fn consume(&mut self, amt: usize) {
        self.pos = cmp::min(self.pos + amt, self.len);
    }

    /// Marks `amt` bytes as unconsumed, moving the read position backwards.
    ///
    /// If `amt` exceeds the available consumed data, the position is clamped to 0.
    ///
    /// # Examples
    ///
    /// ```
    /// # use strata_reader::buffer::Buffer;
    /// # use std::io::Cursor;
    /// let mut buffer = Buffer::new();
    /// buffer.fill(Cursor::new(b"Hello")).unwrap();
    /// buffer.consume(3);
    /// assert_eq!(buffer.pos(), 3);
    /// buffer.unconsume(2);
    /// assert_eq!(buffer.pos(), 1);
    /// ```
    #[inline]
    pub fn unconsume(&mut self, amt: usize) {
        self.pos = self.pos.saturating_sub(amt);
    }

    /// Moves the unconsumed data to the start of the buffer, clearing all consumed data.
    ///
    /// The read position is reset to 0 and the capacity stays the same.
    ///
    /// This operation is useful after consuming a significant amount of data to make room for
    /// more reads without growing the buffer.
    ///
    /// # Examples
    ///
    /// ```
    /// # use strata_reader::buffer::Buffer;
    /// # use std::io::Cursor;
    /// let mut buffer = Buffer::new();
    /// buffer.fill(Cursor::new(b"Hello, World!")).unwrap();
    /// buffer.consume(7);  // Consume "Hello, "
    ///
    /// let cap_before = buffer.cap();
    /// buffer.compact();   // Move "World!" to start
    ///
    /// assert_eq!(buffer.pos(), 0);
    /// assert_eq!(buffer.len(), 6);  // "World!"
    /// assert_eq!(buffer.cap(), cap_before);  // Capacity unchanged
    ///
    /// // Optionally shrink afterwards
    /// buffer.shrink();
    /// ```
    #[expect(clippy::arithmetic_side_effects, reason = "Safe by invariant")]
    #[inline]
    pub fn compact(&mut self) {
        self.buf.copy_within(self.pos..self.len, 0);
        self.len -= self.pos;
        self.pos = 0;
    }

    /// Rounds capacity down to the nearest [`CHUNK_SIZE`] multiple.
    ///
    /// This method implements the linear shrinking strategy used by the buffer. It rounds down
    /// the given capacity to the nearest multiple of [`CHUNK_SIZE`], with a minimum of
    /// [`CHUNK_SIZE`] and a maximum supported capacity.
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
    #[expect(clippy::arithmetic_side_effects, reason = "Safe by bounds checks")]
    pub fn cap_down(capacity: usize) -> usize {
        // Bounds checks clamp the result to the supported capacity range

        // Min bounds check
        if capacity <= CHUNK_SIZE {
            return CHUNK_SIZE;
        }

        // Max bounds check
        if capacity >= MAX_SUPPORTED_CAPACITY {
            return MAX_SUPPORTED_CAPACITY;
        }

        // Round down `capacity` to the nearest multiple of `CHUNK_SIZE`
        (capacity / CHUNK_SIZE) * CHUNK_SIZE
    }

    /// Rounds capacity up to the nearest power-of-two multiple of [`CHUNK_SIZE`].
    ///
    /// This method implements the exponential growth strategy used by the buffer. It rounds up
    /// the given capacity to the nearest power-of-two multiple of [`CHUNK_SIZE`], with a minimum of
    /// [`CHUNK_SIZE`] and saturation at the implementation's supported maximum.
    ///
    /// # Examples
    ///
    /// ```
    /// # use strata_reader::buffer::Buffer;
    /// # use strata_reader::constants::CHUNK_SIZE;
    /// // Rounds up to nearest power-of-two multiple of CHUNK_SIZE
    /// assert_eq!(Buffer::cap_up(CHUNK_SIZE), CHUNK_SIZE);
    /// assert_eq!(Buffer::cap_up(CHUNK_SIZE + 1), 2 * CHUNK_SIZE);
    /// assert_eq!(Buffer::cap_up(2 * CHUNK_SIZE), 2 * CHUNK_SIZE);
    /// assert_eq!(Buffer::cap_up(2 * CHUNK_SIZE + 1), 4 * CHUNK_SIZE);
    ///
    /// // Minimum is CHUNK_SIZE
    /// assert_eq!(Buffer::cap_up(0), CHUNK_SIZE);
    /// assert_eq!(Buffer::cap_up(100), CHUNK_SIZE);
    /// ```
    #[inline]
    #[expect(clippy::arithmetic_side_effects, reason = "Safe by bounds checks")]
    pub fn cap_up(capacity: usize) -> usize {
        // Bounds checks clamp the result to the supported capacity range

        // Max bounds check
        if capacity > MAX_EXPONENTIAL_CAPACITY {
            return MAX_SUPPORTED_CAPACITY;
        }

        // Min bounds check
        if capacity <= CHUNK_SIZE {
            return CHUNK_SIZE;
        }

        // Round up `capacity` to the nearest power-of-two multiple of `CHUNK_SIZE`
        ((capacity + CHUNK_SIZE - 1) / CHUNK_SIZE).next_power_of_two() * CHUNK_SIZE
    }

    /// Rounds capacity up to the nearest [`CHUNK_SIZE`] multiple.
    ///
    /// This method provides linear capacity rounding, for precise capacity allocation. It rounds up
    /// the given capacity to the nearest multiple of [`CHUNK_SIZE`], with a minimum of
    /// [`CHUNK_SIZE`] and a maximum supported capacity.
    ///
    /// # Examples
    ///
    /// ```
    /// # use strata_reader::buffer::Buffer;
    /// # use strata_reader::constants::CHUNK_SIZE;
    /// // Rounds up to nearest CHUNK_SIZE multiple
    /// assert_eq!(Buffer::cap_up_linear(CHUNK_SIZE + 1), 2 * CHUNK_SIZE);
    /// assert_eq!(Buffer::cap_up_linear(2 * CHUNK_SIZE), 2 * CHUNK_SIZE);
    /// assert_eq!(Buffer::cap_up_linear(5 * CHUNK_SIZE - 1), 5 * CHUNK_SIZE);
    ///
    /// // Minimum is CHUNK_SIZE
    /// assert_eq!(Buffer::cap_up_linear(0), CHUNK_SIZE);
    /// assert_eq!(Buffer::cap_up_linear(100), CHUNK_SIZE);
    /// ```
    #[inline]
    #[expect(clippy::arithmetic_side_effects, reason = "Safe by bounds checks")]
    pub fn cap_up_linear(capacity: usize) -> usize {
        // Bounds checks clamp the result to the supported capacity range

        // Max bounds check
        if capacity > MAX_SUPPORTED_CAPACITY - CHUNK_SIZE {
            return MAX_SUPPORTED_CAPACITY;
        }

        // Min bounds check
        if capacity <= CHUNK_SIZE {
            return CHUNK_SIZE;
        }

        // Round up `capacity` to the nearest multiple of `CHUNK_SIZE`
        ((capacity + CHUNK_SIZE - 1) / CHUNK_SIZE) * CHUNK_SIZE
    }

    /// Grows the buffer capacity to the next power-of-two multiple of [`CHUNK_SIZE`].
    ///
    /// The capacity grows exponentially until it reaches the supported maximum.
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
    #[expect(clippy::arithmetic_side_effects, reason = "Safe by invariant")]
    #[inline]
    pub fn grow(&mut self) {
        self.grow_targeted(self.cap + CHUNK_SIZE);
    }

    /// Grows the buffer capacity to at least the specified target.
    ///
    /// If the buffer's current capacity already meets or exceeds `target`, no operation is
    /// performed. Otherwise, grows to the nearest power-of-two multiple of [`CHUNK_SIZE`] that
    /// accommodates `target`, saturating at the supported maximum.
    ///
    /// # Examples
    ///
    /// ```
    /// # use strata_reader::buffer::Buffer;
    /// # use strata_reader::constants::CHUNK_SIZE;
    /// let mut buffer = Buffer::new(); // starts at CHUNK_SIZE
    /// buffer.grow_targeted(4 * CHUNK_SIZE);
    /// assert_eq!(buffer.cap(), 4 * CHUNK_SIZE);
    ///
    /// // No-op when already large enough
    /// buffer.grow_targeted(CHUNK_SIZE);
    /// assert_eq!(buffer.cap(), 4 * CHUNK_SIZE);
    /// ```
    #[inline]
    pub fn grow_targeted(&mut self, target: usize) {
        let next = Self::cap_up(target);

        if next > self.cap {
            self.buf.resize(next, 0);
            self.cap = next;
        }
    }

    /// Grows the buffer capacity to at least the specified target linearly.
    ///
    /// If the buffer's current capacity already meets or exceeds `target`, no operation is
    /// performed. Otherwise, grows to the nearest multiple of [`CHUNK_SIZE`] that accommodates
    /// `target`, saturating at the supported maximum.
    ///
    /// # Examples
    ///
    /// ```
    /// # use strata_reader::buffer::Buffer;
    /// # use strata_reader::constants::CHUNK_SIZE;
    /// let mut buffer = Buffer::new(); // starts at CHUNK_SIZE
    /// buffer.grow_targeted_linear(3 * CHUNK_SIZE);
    /// assert_eq!(buffer.cap(), 3 * CHUNK_SIZE);
    ///
    /// // No-op when already large enough
    /// buffer.grow_targeted_linear(CHUNK_SIZE);
    /// assert_eq!(buffer.cap(), 3 * CHUNK_SIZE);
    /// ```
    #[inline]
    pub fn grow_targeted_linear(&mut self, target: usize) {
        let next = Self::cap_up_linear(target);

        if next > self.cap {
            self.buf.resize(next, 0);
            self.cap = next;
        }
    }

    /// Shrinks the buffer capacity to fit the current data.
    ///
    /// Reduces the buffer's capacity to the smallest multiple of [`CHUNK_SIZE`] that can hold the
    /// current data. The minimum capacity is [`CHUNK_SIZE`].
    ///
    /// # Examples
    ///
    /// ```
    /// # use strata_reader::buffer::Buffer;
    /// # use strata_reader::constants::CHUNK_SIZE;
    /// # use std::io::Cursor;
    /// let mut buffer = Buffer::with_capacity(8 * CHUNK_SIZE);
    /// buffer.fill(Cursor::new(b"Hello")).unwrap();
    ///
    /// buffer.shrink();
    /// assert_eq!(buffer.cap(), CHUNK_SIZE);  // Shrinks to minimum for small data
    /// assert_eq!(buffer.len(), 5);
    /// ```
    #[inline]
    pub fn shrink(&mut self) {
        self.shrink_targeted(CHUNK_SIZE);
    }

    /// Shrinks the buffer capacity to fit the current data or a target capacity, whichever is larger.
    ///
    /// Reduces the buffer's capacity to the smallest [`CHUNK_SIZE`] multiple that can hold either
    /// the current data or the specified target capacity, whichever requires more space. The minimum
    /// capacity is [`CHUNK_SIZE`].
    ///
    /// # Examples
    ///
    /// ```
    /// # use strata_reader::buffer::Buffer;
    /// # use strata_reader::constants::CHUNK_SIZE;
    /// # use std::io::Cursor;
    /// let mut buffer = Buffer::with_capacity(8 * CHUNK_SIZE);
    /// buffer.fill(Cursor::new(b"Hello")).unwrap();
    ///
    /// // Shrink but keep at least 4x CHUNK_SIZE
    /// buffer.shrink_targeted(4 * CHUNK_SIZE);
    /// assert_eq!(buffer.cap(), 4 * CHUNK_SIZE);
    ///
    /// // Shrink with a smaller target
    /// buffer.shrink_targeted(CHUNK_SIZE / 2);
    /// assert_eq!(buffer.cap(), CHUNK_SIZE);  // Minimum capacity is CHUNK_SIZE
    /// ```
    #[inline]
    pub fn shrink_targeted(&mut self, target: usize) {
        // Round `self.len()` up to the next chunk boundary to ensure `self.cap()` >= `self.len()`
        let mut next = Self::cap_up_linear(self.len);

        // Round the target as well to be safe, then use the larger of data size or target
        let next_target = Self::cap_up_linear(target);
        next = next.max(next_target);

        self.buf.truncate(next);
        self.buf.shrink_to(next);
        self.cap = next;
    }

    /// Fills the available space in the buffer from a reader.
    ///
    /// Reads data to fill the buffer up to its current capacity without growing the buffer.
    ///
    /// This method keeps reading until the buffer has no free space left, the reader reaches EOF,
    /// or an error occurs. It may therefore perform multiple underlying `read` calls in a single
    /// invocation.
    ///
    /// Returns a [`FillResult`] with the number of bytes read and context about how the operation
    /// completed:
    ///
    /// - [`FillResult::Complete`]: The buffer was filled to capacity (or was already full).
    /// - [`FillResult::Eof`]: The reader reached end-of-file before filling the buffer.
    ///
    /// Both variants represent successful operations, the byte count may be 0 if the buffer is
    /// already full or the reader is at EOF.
    ///
    /// # Examples
    ///
    /// ```
    /// # use strata_reader::buffer::{Buffer, FillResult};
    /// # use std::io::Cursor;
    /// let mut buffer = Buffer::new();
    /// let mut reader = Cursor::new(b"Hello, World!");
    /// let result = buffer.fill(&mut reader).unwrap();
    /// assert!(matches!(result, FillResult::Eof(13)));
    /// assert_eq!(buffer.len(), 13);
    ///
    /// // Reading from EOF returns Eof(0)
    /// let result = buffer.fill(&mut reader).unwrap();
    /// assert!(matches!(result, FillResult::Eof(0)));
    /// ```
    ///
    /// # Errors
    ///
    /// Returns any I/O errors encountered while reading from the reader.
    pub fn fill(&mut self, mut reader: impl Read) -> io::Result<FillResult> {
        let mut total_bytes_read = 0;

        loop {
            // This is a non-growing method so hitting the current cap means `Complete`
            match self.read_once(&mut reader, &mut total_bytes_read, Some(self.cap))? {
                ReadOnce::Capped => return Ok(FillResult::Complete(total_bytes_read)),
                ReadOnce::Eof => return Ok(FillResult::Eof(total_bytes_read)),
                ReadOnce::Read => {}
            }
        }
    }

    /// Tries to fill the buffer with at least `amt` bytes from a reader, growing as needed.
    ///
    /// Pre-allocates capacity to fit the full requested amount, then reads until the requested
    /// amount is read or EOF is reached. On EOF, any excess capacity beyond the starting capacity
    /// is released.
    ///
    /// Returns a [`FillResult`] with the number of bytes read and context about how the operation
    /// completed:
    ///
    /// - [`FillResult::Complete`]: At least the requested amount of bytes were read.
    /// - [`FillResult::Eof`]: The reader reached end-of-file before reading the requested amount of
    ///   bytes.
    ///
    /// Both variants represent successful operations, the byte count may be 0 if the reader is at
    /// EOF.
    ///
    /// # Examples
    ///
    /// ```
    /// # use strata_reader::buffer::{Buffer, FillResult};
    /// # use strata_reader::constants::CHUNK_SIZE;
    /// # use std::io::Cursor;
    /// let mut buffer = Buffer::new();
    /// let data = vec![0u8; 3 * CHUNK_SIZE];
    /// let mut reader = Cursor::new(data);
    /// let result = buffer.fill_amount(&mut reader, 3 * CHUNK_SIZE).unwrap();
    /// assert!(matches!(result, FillResult::Complete(_)));
    /// assert!(result.count() >= 3 * CHUNK_SIZE);
    /// ```
    ///
    /// # Errors
    ///
    /// Returns any I/O errors encountered while reading from the reader.
    #[expect(clippy::arithmetic_side_effects, reason = "Safe by overflow check")]
    pub fn fill_amount(&mut self, mut reader: impl Read, amt: usize) -> io::Result<FillResult> {
        // Check if the requested amount exceeds what we can possibly accommodate
        if amt > MAX_SUPPORTED_CAPACITY - self.len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "requested amount exceeds maximum buffer capacity",
            ));
        }

        // Capture starting capacity to use as a shrink target
        let starting_capacity = self.cap;

        // Find the target length (safe: checked above)
        let target = self.len + amt;

        // Grow to accommodate amt more bytes of data
        self.grow_targeted_linear(target);

        let mut total_bytes_read = 0;

        while total_bytes_read < amt {
            match self.read_once(&mut reader, &mut total_bytes_read, Some(self.cap))? {
                ReadOnce::Read => {}
                ReadOnce::Eof => {
                    self.shrink_targeted(starting_capacity);
                    return Ok(FillResult::Eof(total_bytes_read));
                }
                ReadOnce::Capped => unreachable!(),
            }
        }

        Ok(FillResult::Complete(total_bytes_read))
    }

    /// Fills the buffer with exactly `amt` bytes from a reader, growing as needed.
    ///
    /// Pre-allocates capacity to fit the requested amount, then reads exactly that many bytes
    /// using [`Read::read_exact`].
    ///
    /// Unlike [`fill_amount`](Self::fill_amount), this method requires the full amount to be
    /// available and returns an error if EOF is reached early.
    ///
    /// # Examples
    ///
    /// ```
    /// # use strata_reader::buffer::Buffer;
    /// # use strata_reader::constants::CHUNK_SIZE;
    /// # use std::io::Cursor;
    /// let mut buffer = Buffer::new();
    /// let data = vec![0u8; 100];
    /// let mut reader = Cursor::new(data);
    /// buffer.fill_exact(&mut reader, 100).unwrap();
    /// assert_eq!(buffer.len(), 100);
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`io::ErrorKind::UnexpectedEof`] if the reader cannot provide the full amount.
    /// On error, the buffer contents are unspecified (some bytes may have been read).
    #[expect(
        clippy::arithmetic_side_effects,
        clippy::indexing_slicing,
        reason = "Safe by invariant"
    )]
    pub fn fill_exact(&mut self, mut reader: impl Read, amt: usize) -> io::Result<()> {
        // Check if the requested amount exceeds what we can possibly accommodate
        if amt > MAX_SUPPORTED_CAPACITY - self.len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "requested amount exceeds maximum buffer capacity",
            ));
        }

        // Find the target length (safe: checked above)
        let target = self.len + amt;

        // Grow to accommodate amt more bytes of data
        self.grow_targeted_linear(target);

        // Get exact free slice
        let unfilled = &mut self.buf[self.len..target];
        debug_assert_eq!(unfilled.len(), amt);

        // Read exactly the requested amount of bytes
        reader.read_exact(unfilled)?;

        // Update the length
        self.len += amt;

        Ok(())
    }

    /// Grows (unless capped), reads once from `reader`, and updates bookkeeping.
    ///
    /// When the buffer is full, `growth_limit` is consulted first. If `Some(limit)` and
    /// `self.cap >= limit`, the read is skipped and [`ReadOnce::Capped`] is returned. Otherwise
    /// the buffer grows before reading, using exponential growth when the next step fits within
    /// the limit and linear growth toward the limit when it would overshoot.
    ///
    /// On success the read bytes are appended (`self.len` is advanced) and `*total_bytes_read` is
    /// incremented.
    #[expect(
        clippy::arithmetic_side_effects,
        clippy::indexing_slicing,
        reason = "Safe by invariant"
    )]
    fn read_once(
        &mut self,
        reader: &mut impl Read,
        total_bytes_read: &mut usize,
        growth_limit: Option<usize>,
    ) -> io::Result<ReadOnce> {
        if self.len >= self.cap {
            debug_assert!(self.len == self.cap);

            if let Some(limit) = growth_limit.map(Self::cap_up_linear) {
                if self.cap >= limit {
                    return Ok(ReadOnce::Capped);
                }

                let next_exponential = Self::cap_up(self.cap + CHUNK_SIZE);
                if next_exponential > limit {
                    // Fail closed if a future caller stops normalizing `growth_limit`.
                    if Self::cap_up_linear(limit) > limit {
                        return Ok(ReadOnce::Capped);
                    }

                    self.grow_targeted_linear(limit);
                } else {
                    self.grow();
                }
            } else {
                self.grow();
            }
        }

        // Fill all available space, retrying on interrupt
        let bytes_read = loop {
            match reader.read(&mut self.buf[self.len..self.cap]) {
                Ok(n) => break n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        };

        if bytes_read == 0 {
            return Ok(ReadOnce::Eof);
        }

        self.len += bytes_read;
        *total_bytes_read += bytes_read;

        Ok(ReadOnce::Read)
    }

    /// Reads from a reader until EOF, growing the buffer as needed.
    ///
    /// Repeatedly reads into available buffer space, growing as needed until the reader returns
    /// `Ok(0)` (EOF). After the operation completes, any excess capacity is shrunk back toward the
    /// starting capacity while still fitting the buffered data.
    ///
    /// This method does not impose a caller-visible growth limit of its own; it keeps reading and
    /// growing until EOF or an error occurs. In practice, available memory is the meaningful
    /// bound.
    ///
    /// # Examples
    ///
    /// ```
    /// # use strata_reader::buffer::Buffer;
    /// # use std::io::Cursor;
    /// let mut buffer = Buffer::new();
    /// let data = vec![42u8; 100];
    /// let mut reader = Cursor::new(data);
    /// let bytes_read = buffer.fill_to_end(&mut reader).unwrap();
    /// assert_eq!(bytes_read, 100);
    /// assert_eq!(buffer.len(), 100);
    /// ```
    ///
    /// # Errors
    ///
    /// Returns any I/O errors encountered while reading. On error, bytes read before
    /// the failure remain in the buffer but the returned count is not available.
    pub fn fill_to_end(&mut self, mut reader: impl Read) -> io::Result<usize> {
        // Capture starting capacity to use as shrink limit
        let starting_capacity = self.cap;
        // Track the total bytes read
        let mut total_bytes_read = 0;

        // Loop until we hit EOF
        loop {
            match self.read_once(&mut reader, &mut total_bytes_read, None)? {
                ReadOnce::Read => {}
                ReadOnce::Eof => break,
                ReadOnce::Capped => unreachable!(),
            }
        }

        // Shrink in case we were overeager with our growth
        self.shrink_targeted(starting_capacity);

        Ok(total_bytes_read)
    }

    /// Reads from a reader while a predicate holds, growing the buffer as needed.
    ///
    /// After each successful read, the predicate is called with the full unconsumed data
    /// (`&self.buf()[self.pos()..self.len()]`). Reading continues while the predicate returns
    /// `true`.
    ///
    /// Pass a `growth_limit` to cap how large the buffer may grow, or `None` to leave growth
    /// uncapped by the caller. Non-aligned limits are rounded up to the next [`CHUNK_SIZE`]
    /// multiple. Returns [`UnboundedFillResult::Capped`] if the limit is reached before the
    /// predicate returns `false`.
    ///
    /// # Examples
    ///
    /// Read until a delimiter is found, tracking its position:
    ///
    /// ```
    /// # use strata_reader::buffer::{Buffer, UnboundedFillResult};
    /// # use std::io::Cursor;
    /// let mut buffer = Buffer::new();
    /// let mut reader = Cursor::new(b"key=value\nother");
    ///
    /// let mut delim_pos = None;
    /// let result = buffer.fill_while(
    ///     &mut reader,
    ///     |data| {
    ///         match data.iter().position(|&b| b == b'\n') {
    ///             Some(pos) => { delim_pos = Some(pos); false }
    ///             None => true,
    ///         }
    ///     },
    ///     None,
    /// ).unwrap();
    ///
    /// assert!(matches!(result, UnboundedFillResult::Complete(_)));
    /// assert_eq!(delim_pos, Some(9));
    /// ```
    ///
    /// # Errors
    ///
    /// Returns any I/O errors encountered while reading. On error, bytes read before
    /// the failure remain in the buffer but the returned count is not available.
    #[expect(clippy::indexing_slicing, reason = "Safe by invariant")]
    pub fn fill_while(
        &mut self,
        mut reader: impl Read,
        mut predicate: impl FnMut(&[u8]) -> bool,
        growth_limit: Option<usize>,
    ) -> io::Result<UnboundedFillResult> {
        // Capture starting capacity to use as shrink limit
        let starting_capacity = self.cap;
        // Track the total bytes read
        let mut total_bytes_read = 0;

        loop {
            // Check the predicate on current unconsumed data before reading more
            if !predicate(&self.buf[self.pos..self.len]) {
                break;
            }

            match self.read_once(&mut reader, &mut total_bytes_read, growth_limit)? {
                ReadOnce::Capped => return Ok(UnboundedFillResult::Capped(total_bytes_read)),
                ReadOnce::Eof => {
                    self.shrink_targeted(starting_capacity);
                    return Ok(UnboundedFillResult::Eof(total_bytes_read));
                }
                ReadOnce::Read => {}
            }
        }

        // Shrink in case we were overeager with our growth
        self.shrink_targeted(starting_capacity);

        Ok(UnboundedFillResult::Complete(total_bytes_read))
    }

    /// Reads from a reader until a byte delimiter is found, growing the buffer as needed.
    ///
    /// Existing unconsumed data is checked first, and the search continues from where the previous
    /// check left off.
    ///
    /// Pass a `growth_limit` to cap how large the buffer may grow, or `None` to leave growth
    /// uncapped by the caller. Non-aligned limits are rounded up to the next [`CHUNK_SIZE`]
    /// multiple. Returns [`UnboundedFillResult::Capped`] if the limit is reached before the
    /// delimiter is found.
    ///
    /// # Examples
    ///
    /// ```
    /// # use strata_reader::buffer::{Buffer, UnboundedFillResult};
    /// # use std::io::Cursor;
    /// let mut buffer = Buffer::new();
    /// let mut reader = Cursor::new(b"key=value\nother");
    ///
    /// let result = buffer.fill_until(&mut reader, b'\n', None).unwrap();
    ///
    /// assert!(matches!(result, UnboundedFillResult::Complete(_)));
    /// assert!(buffer.buf().contains(&b'\n'));
    /// ```
    ///
    /// # Errors
    ///
    /// Returns any I/O errors encountered while reading.
    #[expect(clippy::indexing_slicing, reason = "Safe by invariant")]
    pub fn fill_until(
        &mut self,
        mut reader: impl Read,
        byte: u8,
        growth_limit: Option<usize>,
    ) -> io::Result<UnboundedFillResult> {
        // Capture starting capacity to use as shrink limit
        let starting_capacity = self.cap;
        // Track the total bytes read
        let mut total_bytes_read = 0;
        // Start checking from the unconsumed region
        let mut check_pos = self.pos;

        loop {
            // Check the unchecked portion for the delimiter before reading more
            if self.buf[check_pos..self.len].contains(&byte) {
                break;
            }

            // Advance past what we've already checked
            check_pos = self.len;

            match self.read_once(&mut reader, &mut total_bytes_read, growth_limit)? {
                ReadOnce::Capped => return Ok(UnboundedFillResult::Capped(total_bytes_read)),
                ReadOnce::Eof => {
                    self.shrink_targeted(starting_capacity);
                    return Ok(UnboundedFillResult::Eof(total_bytes_read));
                }
                ReadOnce::Read => {}
            }
        }

        self.shrink_targeted(starting_capacity);

        Ok(UnboundedFillResult::Complete(total_bytes_read))
    }

    /// Aligns a position backward to the start of the current UTF-8 character.
    ///
    /// If `offset` falls in the middle of a UTF-8 multi-byte character, this moves backward to the
    /// start of that character. If already on a character boundary, returns that position. The
    /// offset is clamped to the range `self.pos()..=self.len()`.
    ///
    /// When `offset` is at (or clamps to) `self.len()`, the function checks whether the trailing
    /// bytes form a complete UTF-8 sequence. If they do, `self.len()` is returned as a valid
    /// boundary. If the buffer ends with an incomplete multi-byte character, the position is moved
    /// back to the start of that incomplete sequence.
    ///
    /// Note that if `self.pos()` itself is not on a UTF-8 character boundary (e.g., if positioned
    /// within a multi-byte character), the returned position may still not be on a valid character
    /// boundary, as it cannot move before `self.pos()`.
    ///
    /// This is useful when working with UTF-8 text to ensure operations don't split characters.
    ///
    /// # Examples
    ///
    /// ```
    /// # use strata_reader::buffer::Buffer;
    /// # use std::io::Cursor;
    /// let mut buffer = Buffer::new();
    /// buffer.fill(Cursor::new("Hello, 世界!")).unwrap(); // 世界 is World in Japanese
    /// // "Hello, " = 7 bytes, '世' starts at byte 7 and is 3 bytes long, '界' starts at byte 10
    /// // Position 8 is in the middle of the '世' character
    /// let aligned = buffer.align_pos_to_char(8);
    /// assert_eq!(aligned, 7); // Aligned to start of '世'
    /// ```
    #[expect(
        clippy::arithmetic_side_effects,
        clippy::indexing_slicing,
        reason = "Safe by invariant"
    )]
    #[inline]
    pub fn align_pos_to_char(&self, offset: usize) -> usize {
        // Clamp the requested offset to the readable range: self.pos <= offset <= self.len.
        let clamped = cmp::max(offset, self.pos).min(self.len);

        if clamped == self.len {
            // Can't use an inclusive range here: buf[len] is outside the logical content, and
            // panics when len == cap. Instead, scan the valid bytes to check whether self.len
            // falls on a complete character boundary.
            if self.pos == self.len {
                return self.len;
            }

            // Find the start of the last byte sequence.
            let start = self.buf[self.pos..self.len]
                .iter()
                .rev()
                .position(|&b| b & 0b1100_0000 != 0b1000_0000)
                .map_or(self.pos, |i| self.len - 1 - i);

            // Determine the expected sequence length from the leading byte.
            let leading = self.buf[start];
            let expected = if leading & 0b1000_0000 == 0 {
                1
            } else if leading & 0b1110_0000 == 0b1100_0000 {
                2
            } else if leading & 0b1111_0000 == 0b1110_0000 {
                3
            } else if leading & 0b1111_1000 == 0b1111_0000 {
                4
            } else {
                // Invalid leading byte (bare continuation or 0xFF/0xFE), treat as single byte.
                1
            };

            if start + expected <= self.len {
                self.len // Trailing character is complete; self.len is a valid boundary.
            } else {
                start // Incomplete trailing sequence; back up to its start.
            }
        } else {
            // Find the position of first byte that is not a UTF-8 continuation byte
            self.buf[self.pos..=clamped]
                .iter()
                .rev()
                // If the top two bits are 10 then it's a continuation byte, this bitmask checks that
                .position(|&b| b & 0b1100_0000 != 0b1000_0000)
                .map_or(self.pos, |i| clamped - i)
        }
    }

    /// Aligns a position forward to the start of the next UTF-8 character.
    ///
    /// If `offset` falls in the middle of a UTF-8 multi-byte character, this moves forward to the
    /// start of the following character. If already on a character boundary, returns that
    /// position. The offset is clamped to the range `self.pos()..=self.len()`.
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
    /// buffer.fill(Cursor::new("Hello, 世界!")).unwrap(); // 世界 is World in Japanese
    /// // "Hello, " = 7 bytes, '世' starts at byte 7 and is 3 bytes long, '界' starts at byte 10
    /// // Position 8 is in the middle of the '世' character
    /// let aligned = buffer.align_pos_to_next_char(8);
    /// assert_eq!(aligned, 10); // Aligned to start of '界'
    /// ```
    #[expect(
        clippy::arithmetic_side_effects,
        clippy::indexing_slicing,
        reason = "Safe by invariant"
    )]
    #[inline]
    pub fn align_pos_to_next_char(&self, offset: usize) -> usize {
        // Clamp the requested offset to the readable range: self.pos <= offset <= self.len.
        let clamped = cmp::max(offset, self.pos).min(self.len);

        // Find the position of first byte that is not a UTF-8 continuation byte
        self.buf[clamped..self.len]
            .iter()
            // If the top two bits are 10 then it's a continuation byte, this bitmask checks that
            .position(|&b| b & 0b1100_0000 != 0b1000_0000)
            .map_or(self.len, |i| i + clamped)
    }

    /// Returns the unconsumed buffer data as a UTF-8 string slice.
    ///
    /// Automatically handles partial UTF-8 sequences by:
    /// - Skipping incomplete sequences at the start (e.g., if `pos()` is not on a character
    ///   boundary)
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

    /// Returns buffer data as a UTF-8 string slice starting from a specific offset.
    ///
    /// Like [`as_str()`](Self::as_str), but starts from an offset in the buffer clamped to the
    /// range `self.pos()..=self.len()`. Automatically handles partial UTF-8 sequences at both
    /// boundaries.
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
        reason = "Safe by invariant"
    )]
    pub fn as_str_from(&self, offset: usize) -> io::Result<&str> {
        let start = self.align_pos_to_next_char(offset);

        let mut end = self.len;
        while end > start {
            match str::from_utf8(&self.buf[start..end]) {
                Ok(s) => return Ok(s),
                Err(e) => {
                    // If we have an invalid UTF-8 sequence in the middle
                    if e.error_len().is_some() {
                        return Err(io::Error::new(io::ErrorKind::InvalidData, e));
                    }

                    // We have an incomplete sequence at the end, so trim it
                    end = start + e.valid_up_to();
                }
            }
        }

        Ok("")
    }

    /// Reads from a reader until a character delimiter is found, growing the buffer as needed.
    ///
    /// Existing unconsumed data is checked first, and character matches are handled correctly even
    /// when their UTF-8 bytes span read boundaries.
    ///
    /// Pass a `growth_limit` to cap how large the buffer may grow, or `None` to leave growth
    /// uncapped by the caller. Non-aligned limits are rounded up to the next [`CHUNK_SIZE`]
    /// multiple.
    /// Returns [`UnboundedFillResult::Capped`] if the limit is reached before `ch` is found.
    ///
    /// # Examples
    ///
    /// ```
    /// # use strata_reader::buffer::{Buffer, UnboundedFillResult};
    /// # use std::io::Cursor;
    /// let mut buffer = Buffer::new();
    /// let mut reader = Cursor::new("Hello, 世界!");
    ///
    /// let result = buffer.fill_until_char(&mut reader, '界', None).unwrap();
    ///
    /// assert!(matches!(result, UnboundedFillResult::Complete(_)));
    /// ```
    ///
    /// # Errors
    ///
    /// Returns any I/O errors encountered while reading, or an
    /// [`InvalidData`](io::ErrorKind::InvalidData) error if the buffer contains invalid UTF-8.
    pub fn fill_until_char(
        &mut self,
        reader: impl Read,
        ch: char,
        growth_limit: Option<usize>,
    ) -> io::Result<UnboundedFillResult> {
        self.fill_until_str(reader, ch.encode_utf8(&mut [0; 4]), growth_limit)
    }

    /// Reads from a reader until a string delimiter is found, growing the buffer as needed.
    ///
    /// Existing unconsumed data is checked first. A small overlap may be revisited between
    /// iterations so matches spanning read boundaries are not missed.
    ///
    /// Pass a `growth_limit` to cap how large the buffer may grow, or `None` to leave growth
    /// uncapped by the caller. Non-aligned limits are rounded up to the next [`CHUNK_SIZE`]
    /// multiple. Returns [`UnboundedFillResult::Capped`] if the limit is reached before `needle`
    /// is found, or [`UnboundedFillResult::Complete`] immediately if `needle` is empty.
    ///
    /// # Examples
    ///
    /// ```
    /// # use strata_reader::buffer::{Buffer, UnboundedFillResult};
    /// # use std::io::Cursor;
    /// let mut buffer = Buffer::new();
    /// let mut reader = Cursor::new(b"Hello, World!END more data");
    ///
    /// let result = buffer.fill_until_str(&mut reader, "END", None).unwrap();
    ///
    /// assert!(matches!(result, UnboundedFillResult::Complete(_)));
    /// ```
    ///
    /// # Errors
    ///
    /// Returns any I/O errors encountered while reading, or an
    /// [`InvalidData`](io::ErrorKind::InvalidData) error if the buffer contains invalid UTF-8.
    #[expect(clippy::arithmetic_side_effects, reason = "Safe by invariant")]
    pub fn fill_until_str(
        &mut self,
        mut reader: impl Read,
        needle: &str,
        growth_limit: Option<usize>,
    ) -> io::Result<UnboundedFillResult> {
        if needle.is_empty() {
            return Ok(UnboundedFillResult::Complete(0));
        }

        // Capture starting capacity to use as shrink limit
        let starting_capacity = self.cap;
        let mut total_bytes_read = 0;
        // Start checking from the first valid char at or after pos
        let mut check_pos = self.align_pos_to_next_char(self.pos);

        loop {
            /* Decode only the unchecked portion as UTF-8 and search for the needle. as_str_from
            trims incomplete chars at the trailing edge, so they'll be re-examined next iteration
            when more bytes arrive */
            let unchecked = self.as_str_from(check_pos)?;
            if unchecked.contains(needle) {
                break;
            }

            /* Advance past checked data, but back up by `needle.len() - 1` to catch matches that
            span the boundary between this read and the next. align_pos_to_char ensures we land on a
            char boundary (backward) so that as_str_from won't skip past a valid needle start */
            let end = check_pos + unchecked.len();
            check_pos = self.align_pos_to_char((end + 1).saturating_sub(needle.len()));

            match self.read_once(&mut reader, &mut total_bytes_read, growth_limit)? {
                ReadOnce::Capped => return Ok(UnboundedFillResult::Capped(total_bytes_read)),
                ReadOnce::Eof => {
                    self.shrink_targeted(starting_capacity);
                    return Ok(UnboundedFillResult::Eof(total_bytes_read));
                }
                ReadOnce::Read => {}
            }
        }

        self.shrink_targeted(starting_capacity);

        Ok(UnboundedFillResult::Complete(total_bytes_read))
    }
}

#[cfg(test)]
pub(crate) mod tests;
