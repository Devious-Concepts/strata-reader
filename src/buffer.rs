//! High-performance buffer with dynamic capacity management.
//!
//! The [`Buffer`] type provides standalone buffered I/O operations with both manual and automatic
//! growth and shrinking of it's capacity. It is used as the internal buffer for
//! [`Reader`](crate::Reader).
//!
//! # Example
//!
//! ```
//! use strata_reader::buffer::{Buffer, FillResult};
//! use strata_reader::constants::CHUNK_SIZE;
//! use std::io::Cursor;
//!
//! let data = vec![0u8; 3 * CHUNK_SIZE];
//! let cur = Cursor::new(data);
//! let mut buffer = Buffer::new();
//!
//! // Read a specific amount
//! let result = buffer.fill_amount(cur, 3 * CHUNK_SIZE).unwrap();
//! assert!(matches!(result, FillResult::Complete(_)));
//!
//! // Consume what we processed
//! buffer.consume(CHUNK_SIZE);
//!
//! // Check remaining data
//! assert_eq!(buffer.len() - buffer.pos(), 2 * CHUNK_SIZE);
//! ```

use crate::constants::{CHUNK_SIZE, PRACTICAL_MAX_SIZE};
use std::cmp;
use std::io::{self, Read};

/// Result type for bounded fill operations.
///
/// Returned by operations that read within a known limit.
///
/// The contained byte count represents the total bytes read from the reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillResult {
    /// The operation completed successfully.
    ///
    /// Contains the byte count.
    Complete(usize),

    /// The operation stopped because end-of-file was reached.
    ///
    /// Contains the number of bytes successfully read before EOF was reached.
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
/// Returned by operations that read without a known limit.
///
/// The contained byte count represents the total bytes read from the reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnboundedFillResult {
    /// The operation completed successfully.
    ///
    /// Contains the byte count.
    Complete(usize),

    /// The operation stopped because end-of-file was reached.
    ///
    /// Contains the number of bytes successfully read before EOF was reached.
    Eof(usize),

    /// The operation stopped because the maximum capacity was reached.
    ///
    /// The buffer cannot grow beyond the specified maximum capacity (or [`PRACTICAL_MAX_SIZE`]
    /// if no maximum was specified).
    ///
    /// Contains the number of bytes successfully read before the capacity limit was reached.
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
/// - Large file processing with dynamic memory needs
/// - Stream buffering with arbitrary peek-ahead requirements
/// - Any scenario requiring efficient buffered reads with automatic capacity adjustment
///
/// # Invariants
///
/// This buffer maintains the invariant `0 <= self.pos <= self.len <= self.cap == self.buf.len() <=
/// self.buf.capacity()` at all times, ensuring memory safety and correctness of all operations.
#[derive(Debug, Clone, PartialEq, Eq)]
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

impl Default for Buffer {
    fn default() -> Self {
        Self::new()
    }
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
    /// The actual capacity will be rounded up to the nearest [`CHUNK_SIZE`] multiple
    /// that can accommodate the requested capacity.
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
    /// // Gets exact multiple, not power-of-2
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
    #[expect(clippy::indexing_slicing, reason = "Safe by invariant")]
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

    /// Discards all data in the buffer without changing capacity.
    ///
    /// Resets both the read position and length to zero, but keeps the current
    /// buffer capacity unchanged.
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

    /// Discards all data in the buffer and shrinks capacity.
    ///
    /// Resets both the read position and length to zero, then shrinks the buffer
    /// to the minimum capacity.
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
    #[expect(clippy::arithmetic_side_effects, reason = "Safe by invariant")]
    #[inline]
    pub fn consume(&mut self, amt: usize) {
        self.pos = cmp::min(self.pos + amt, self.len);
    }

    /// Removes consumed bytes from the buffer by moving unconsumed data to the start.
    ///
    /// Moves unconsumed data to the beginning of the buffer using `copy_within` and resets the
    /// read position to 0. The capacity remains unchanged - use [`shrink()`](Self::shrink) or
    /// [`shrink_targeted()`](Self::shrink_targeted) afterwards if you want to reduce capacity.
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
    #[expect(clippy::arithmetic_side_effects, reason = "Safe by bounds checks")]
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
    /// assert_eq!(Buffer::cap_up(100), CHUNK_SIZE);
    /// ```
    #[inline]
    #[expect(clippy::arithmetic_side_effects, reason = "Safe by bounds checks")]
    pub fn cap_up(capacity: usize) -> usize {
        // The bounds checks prevent both underflow and overflow problems by setting the minimum at
        // `CHUNK_SIZE` and maximum at [`PRACTICAL_MAX_SIZE`]. The early return for large capacities
        // ensures that power-of-two calculations cannot overflow.

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

    /// Rounds capacity up to the nearest [`CHUNK_SIZE`] multiple (linear).
    ///
    /// This method provides linear capacity rounding, useful for precise capacity allocation.
    /// It rounds up the given capacity to the nearest multiple of [`CHUNK_SIZE`], with a minimum
    /// of [`CHUNK_SIZE`] and a maximum of [`PRACTICAL_MAX_SIZE`].
    ///
    /// Unlike [`cap_up`](Self::cap_up) which uses exponential (power-of-2) rounding, this method
    /// provides exact multiples of [`CHUNK_SIZE`].
    ///
    /// # Use Cases
    ///
    /// - Allocating buffers with precise capacity requirements
    /// - Pre-allocating exactly the needed amount without over-allocation
    /// - Creating buffers where you know the exact size needed
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
        // The bounds checks prevent both underflow and overflow problems by setting the minimum at
        // `CHUNK_SIZE` and maximum at [`PRACTICAL_MAX_SIZE`].

        // Max bounds check
        if capacity >= PRACTICAL_MAX_SIZE {
            return PRACTICAL_MAX_SIZE;
        }

        // Min bounds check
        if capacity < CHUNK_SIZE {
            return CHUNK_SIZE;
        }

        // Round up `capacity` to the nearest multiple of `CHUNK_SIZE`
        ((capacity + CHUNK_SIZE - 1) / CHUNK_SIZE) * CHUNK_SIZE
    }

    /// Grows the buffer capacity to at least the specified target.
    ///
    /// If the buffer's current capacity already meets or exceeds `target`, no operation is
    /// performed. Otherwise, grows to the nearest power-of-2 multiple of [`CHUNK_SIZE`] that
    /// accommodates `target`, up to a maximum of [`PRACTICAL_MAX_SIZE`].
    ///
    /// For unconditional single-step growth to the next power-of-2 multiple, use
    /// [`grow()`](Self::grow).
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
    #[expect(clippy::arithmetic_side_effects, reason = "Safe by invariant")]
    #[inline]
    pub fn grow(&mut self) {
        self.grow_targeted(self.cap + CHUNK_SIZE);
    }

    /// Shrinks the buffer capacity to fit the current data.
    ///
    /// Reduces the buffer's capacity to the smallest [`CHUNK_SIZE`] multiple that can hold the
    /// current data. The minimum capacity is always [`CHUNK_SIZE`]. This is useful for reclaiming
    /// memory after operations that left the buffer over-allocated.
    ///
    /// For more control over the target capacity, use [`shrink_targeted()`](Self::shrink_targeted).
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
    /// capacity is always [`CHUNK_SIZE`].
    ///
    /// This is useful when you want to shrink the buffer but maintain a minimum capacity for future
    /// operations, avoiding frequent reallocations.
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
    /// // Shrink to fit data (smaller than target)
    /// buffer.shrink_targeted(CHUNK_SIZE / 2);
    /// assert_eq!(buffer.cap(), CHUNK_SIZE);  // Can't go below data size
    /// ```
    #[inline]
    pub fn shrink_targeted(&mut self, target: usize) {
        // The length maxes out at [`PRACTICAL_MAX_SIZE`] so adding `CHUNK_SIZE` can't overflow.

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
    /// Returns a [`FillResult`] with the number of bytes read and context about how the
    /// operation completed:
    ///
    /// - [`FillResult::Complete`]: The buffer was filled to capacity (or was already full).
    /// - [`FillResult::Eof`]: The reader reached end-of-file before filling the buffer.
    ///
    /// Both variants represent successful operations - the byte count may be 0 if the buffer
    /// is already full or the reader is at EOF.
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
    #[expect(
        clippy::arithmetic_side_effects,
        clippy::indexing_slicing,
        reason = "Safe by invariant"
    )]
    pub fn fill(&mut self, mut reader: impl Read) -> io::Result<FillResult> {
        let mut bytes_read = 0;

        if self.len >= self.cap {
            debug_assert!(self.len == self.cap);

            // Buffer is full, nothing to read.
            return Ok(FillResult::Complete(0));
        }

        // Read to fill the remaining space.
        bytes_read += reader.read(&mut self.buf[self.len..self.cap])?;

        // Increase the length by the number of bytes read.
        self.len += bytes_read;

        if self.len < self.cap {
            // EOF reached.
            return Ok(FillResult::Eof(bytes_read));
        }

        Ok(FillResult::Complete(bytes_read))
    }

    /// Fills the buffer with at least `amt` bytes from a reader, growing as needed.
    ///
    /// Pre-allocates capacity to fit the full requested amount, then reads until the requested
    /// amount is read or EOF is reached. After the operation completes, any excess capacity
    /// beyond the starting capacity is released.
    ///
    /// The total number of bytes read is returned.
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
    #[expect(
        clippy::arithmetic_side_effects,
        clippy::indexing_slicing,
        reason = "Safe by invariant"
    )]
    pub fn fill_amount(&mut self, mut reader: impl Read, amt: usize) -> io::Result<FillResult> {
        // Check if the requested amount exceeds what we can possibly accommodate
        if amt > PRACTICAL_MAX_SIZE - self.len {
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
        self.grow_targeted(target);

        // Track the total bytes read
        let mut total_bytes_read = 0;

        // Loop until we've read enough bytes or EOF
        while total_bytes_read < amt {
            // Fill all available space
            let bytes_read = reader.read(&mut self.buf[self.len..self.cap])?;

            if bytes_read == 0 {
                // We've hit EOF
                self.shrink_targeted(starting_capacity);
                return Ok(FillResult::Eof(total_bytes_read));
            }

            // Increase the length by the number of bytes read
            self.len += bytes_read;

            // Increase the total number of bytes read
            total_bytes_read += bytes_read;
        }

        // Release any excess capacity
        self.shrink_targeted(starting_capacity);

        Ok(FillResult::Complete(total_bytes_read))
    }

    /// Fills the buffer with exactly `amt` bytes from a reader, growing as needed.
    ///
    /// Pre-allocates capacity to fit the requested amount, then reads exactly that many bytes
    /// using [`Read::read_exact`]. After the operation completes, any excess capacity beyond
    /// the starting capacity is released.
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
        if amt > PRACTICAL_MAX_SIZE - self.len {
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
        self.grow_targeted(target);

        // Get exact free slice
        let unfilled = &mut self.buf[self.len..target];
        debug_assert_eq!(unfilled.len(), amt);

        // Read exactly the requested amount of bytes
        reader.read_exact(unfilled)?;

        // Update the length
        self.len += amt;

        // Release any excess capacity
        self.shrink_targeted(starting_capacity);

        Ok(())
    }

    /// Reads from a reader until EOF, growing the buffer as needed.
    ///
    /// Repeatedly reads into available buffer space, growing exponentially when full,
    /// until the reader returns `Ok(0)` (EOF). After the operation completes, any excess
    /// capacity beyond the starting capacity is released.
    ///
    /// Note that this method has no capacity limit — the buffer will grow unboundedly
    /// until EOF is reached or an error occurs. Callers should ensure the input is
    /// reasonably sized or impose their own limits before calling.
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
    #[expect(
        clippy::arithmetic_side_effects,
        clippy::indexing_slicing,
        reason = "Safe by invariant"
    )]
    pub fn fill_to_end(&mut self, mut reader: impl Read) -> io::Result<usize> {
        // Capture starting capacity to use as shrink limit
        let starting_capacity = self.cap;
        // Track the total bytes read
        let mut total_bytes_read = 0;

        // Loop until we hit EOF
        loop {
            if self.len >= self.cap {
                debug_assert!(self.len == self.cap);

                // Buffer is full, so grow.
                self.grow();

                // Note: This will OOM eventually, or stall infinitely if
                // ran with more than 8 EiB of RAM in some super data centre
            }

            // Fill all available space, retrying on interrupt
            let bytes_read = loop {
                match reader.read(&mut self.buf[self.len..self.cap]) {
                    Ok(n) => break n,
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                    Err(e) => return Err(e),
                }
            };

            if bytes_read == 0 {
                // We've hit EOF
                break;
            }

            // Increase the length by the number of bytes read
            self.len += bytes_read;

            // Increase the total number of bytes read
            total_bytes_read += bytes_read;
        }

        // Shrink in case we were overeager with our growth
        self.shrink_targeted(starting_capacity);

        Ok(total_bytes_read)
    }

    /// Reads from a reader while a predicate holds, growing the buffer as needed.
    ///
    /// After each successful read, the predicate is called with the **full unconsumed data**
    /// (`&buf[pos..len]`). The loop continues while the predicate returns `true`. Because
    /// the predicate always receives the full unconsumed slice (not just newly-read bytes),
    /// it can detect patterns that span read boundaries.
    ///
    /// `max_capacity` caps how large the buffer may grow. When the buffer is full and at
    /// the cap, [`UnboundedFillResult::Capped`] is returned. The value is rounded up to
    /// the nearest power-of-2 multiple of [`CHUNK_SIZE`] via [`cap_up`](Self::cap_up).
    /// Pass `None` for no limit.
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
    #[expect(
        clippy::arithmetic_side_effects,
        clippy::indexing_slicing,
        reason = "Safe by invariant"
    )]
    pub fn fill_while(
        &mut self,
        mut reader: impl Read,
        mut predicate: impl FnMut(&[u8]) -> bool,
        max_capacity: Option<usize>,
    ) -> io::Result<UnboundedFillResult> {
        let max_cap = Self::cap_up(max_capacity.unwrap_or(PRACTICAL_MAX_SIZE));

        // Capture starting capacity to use as shrink limit
        let starting_capacity = self.cap;
        // Track the total bytes read
        let mut total_bytes_read = 0;

        loop {
            if self.len >= self.cap {
                debug_assert!(self.len == self.cap);

                if self.cap >= max_cap {
                    // Buffer is full and can't grow
                    return Ok(UnboundedFillResult::Capped(total_bytes_read));
                }

                // Buffer is full, so grow
                self.grow();
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
                // We've hit EOF
                self.shrink_targeted(starting_capacity);
                return Ok(UnboundedFillResult::Eof(total_bytes_read));
            }

            // Increase the length by the number of bytes read
            self.len += bytes_read;

            // Increase the total number of bytes read
            total_bytes_read += bytes_read;

            // Check if the predicate is satisfied
            if !predicate(&self.buf[self.pos..self.len]) {
                break;
            }
        }

        // Shrink in case we were overeager with our growth
        self.shrink_targeted(starting_capacity);

        Ok(UnboundedFillResult::Complete(total_bytes_read))
    }

    /// Aligns a position backward to the start of the current UTF-8 character.
    ///
    /// If `pos` falls in the middle of a UTF-8 multi-byte character, this moves backward to the
    /// start of that character. If already on a character boundary, returns that position.
    /// The result is clamped to the range `self.pos()..=self.len()`.
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
        reason = "Safe by invariant"
    )]
    pub fn as_str_from(&self, pos: usize) -> io::Result<&str> {
        let start = self.align_pos_to_next_char(pos);

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
}

#[cfg(test)]
mod tests;
