use std::cmp;
use std::io::{self, Read};

use crate::constants::{CHUNK_SIZE, PRACTICAL_MAX_SIZE};

/// A dynamically sized buffer with chunked capacity management.
///
/// # Invariants
///
/// This buffer maintains the invariant `0 <= pos <= len <= buf.len() <= cap <= buf.capacity()` at
/// all times.
#[derive(Debug)]
pub struct Buffer {
    /// Internal buffer.
    buf: Vec<u8>,
    /// Capacity of the buffer, as set, not in practice. It may be slightly larger in reality.
    cap: usize,
    /// Length of the data in the buffer that we care about.
    len: usize,
    /// Position that data has been consumed up to.
    pos: usize,
}

impl Buffer {
    /// Create a new buffer
    #[inline]
    pub fn new() -> Self {
        Self {
            buf: vec![0; CHUNK_SIZE],
            cap: CHUNK_SIZE,
            len: 0,
            pos: 0,
        }
    }

    /// Create a new buffer with at least the given capacity
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

    /// Return a reference slice to the buffer
    #[expect(clippy::indexing_slicing, reason = "The invariant makes it safe")]
    #[inline]
    pub fn buf(&self) -> &[u8] {
        // We need to slice here in case the internal buffer is a bit larger.
        &self.buf[..self.cap]
    }

    /// The capacity of the buffer
    #[inline]
    pub fn cap(&self) -> usize {
        self.cap
    }

    /// The length of the data in the buffer
    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    /// The amount of data that has been consumed
    #[inline]
    pub fn pos(&self) -> usize {
        self.pos
    }

    /// Discard the current buffer.
    #[inline]
    pub fn discard(&mut self) {
        self.pos = 0;
        self.len = 0;

        self.shrink();
    }

    /// Mark an amount of bytes as consumed
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "The invariant makes it safe"
    )]
    #[inline]
    pub fn consume(&mut self, amt: usize) {
        self.pos = cmp::min(self.pos + amt, self.len);
    }

    /// Remove bytes that have been consumed
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "The invariant makes it safe"
    )]
    pub fn compact(&mut self) {
        self.buf.copy_within(self.pos..self.len, 0);
        self.len -= self.pos;
        self.pos = 0;

        self.shrink();
    }

    /// Round down linearly
    ///
    /// # Safety
    ///
    /// The bounds checks prevent both underflow and overflow problems by setting the minimum at
    /// [`CHUNK_SIZE`] and maximum at [`PRACTICAL_MAX_SIZE`]. The smallest value the calculation can
    /// reach is `CHUNK_SIZE` when `capacity` is less than `2 * CHUNK_SIZE`.
    #[inline]
    #[expect(clippy::arithmetic_side_effects, reason = "Bounds checks make it safe")]
    fn cap_down(capacity: usize) -> usize {
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

    /// Round up in exponential steps
    ///
    /// # Safety
    ///
    /// The bounds checks prevent both underflow and overflow problems by setting the minium at
    /// [`CHUNK_SIZE`] and maximum at [`PRACTICAL_MAX_SIZE`]. The max value that the power of two
    /// calculation can reach is `PRACTICAL_MAX_SIZE >> 1` when `capacity` is greater than
    /// `PRACTICAL_MAX_SIZE >> 2`.
    #[inline]
    #[expect(clippy::arithmetic_side_effects, reason = "Bounds checks make it safe")]
    fn cap_up(capacity: usize) -> usize {
        // Max bounds checks
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

    /// Grows the capacity to the next power of 2 multiple of [`CHUNK_SIZE`].
    ///
    /// # Safety
    ///
    /// The capacity maxes out at [`PRACTICAL_MAX_SIZE`] so adding `CHUNK_SIZE` can't overflow.
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "The invariant makes it safe"
    )]
    pub fn grow(&mut self) {
        // Round `self.cap()` up to ensure we advance to the next power of two multiple of
        // `CHUNK_SIZE` even when already at a power of two.
        let next = Self::cap_up(self.cap + CHUNK_SIZE - 1);

        self.buf.resize(next, 0);
        self.cap = next;
    }

    /// Shrink the capacity to the smallest multiple of [`CHUNK_SIZE`] that fits `self.len()`.
    ///
    /// # Safety
    ///
    /// The length maxes out at [`PRACTICAL_MAX_SIZE`] so adding `CHUNK_SIZE` can't overflow.
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "The invariant makes it safe"
    )]
    pub fn shrink(&mut self) {
        // Round `self.len()` up to the next chunk boundary to ensure `self.cap()` >= `self.len()`
        let next = Self::cap_down(self.len + CHUNK_SIZE - 1);

        self.buf.truncate(next);
        self.cap = next;
    }

    /// Fill available space in the buffer and return the number of bytes read.
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
            bytes_read = reader.read(&mut self.buf[self.len..self.cap])?;

            // Increase the length by the number of bytes read.
            self.len += bytes_read;
        }

        Ok(bytes_read)
    }

    /// Fill buffer and grow to fit the requested amount
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

            if available < CHUNK_SIZE / 2 && remaining >= available {
                // We've hit a point where growing would be more optimal than just filling the
                // available space and the available space is too small.
                if self.cap < PRACTICAL_MAX_SIZE {
                    // There is no more space available, grow to the next size.
                    self.grow();
                } else {
                    // Can't grow anymore.
                    break;
                }
            }

            // Get potentially updated available space.
            let available = self.cap - self.len;

            let bytes_read = if available >= remaining {
                // We have enough space, so just read the requested amount.
                reader.read(&mut self.buf[self.len..self.len + remaining])?
            } else {
                // We don't have enough space, so just fill the space we have.
                reader.read(&mut self.buf[self.len..self.cap])?
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

        // Shrink in case we where overeager with our growth.
        self.shrink();

        Ok(total_bytes_read)
    }

    /// Fill buffer while a predicate is true and grow to fit
    pub fn fill_while<P: Fn(char) -> bool>(
        &mut self,
        mut reader: impl Read,
        predicate: P,
    ) -> io::Result<usize> {
        todo!();
    }

    /// Fill buffer until a delimiter is encountered and grow to fit
    pub fn fill_until(&mut self, mut reader: impl Read, delimiter: char) -> io::Result<usize> {
        todo!();
    }
}

impl Default for Buffer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "Unwrap is okay in tests")]
mod tests {
    use std::{io::Cursor, usize};

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
        let buffer = Buffer::new();

        assert_eq!(buffer.buf(), &[0; CHUNK_SIZE]);
    }

    #[test]
    fn test_cap() {
        let buffer = Buffer::new();

        assert_eq!(buffer.cap(), CHUNK_SIZE);
    }

    #[test]
    fn test_len() {
        let buffer = Buffer::new();

        assert_eq!(buffer.len(), 0);
    }

    #[test]
    fn test_pos() {
        let buffer = Buffer::new();

        assert_eq!(buffer.pos(), 0);
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

        // Simulate there being data.
        buffer.len = 100;

        // Consume some bytes
        buffer.consume(10);

        // Buffer state should match
        assert_eq!(buffer.len, 100);
        assert_eq!(buffer.pos, 10);
    }

    #[test]
    fn test_compact() {
        let mut buffer = Buffer::new();

        // Simulate there being data.
        buffer.pos = 10;
        buffer.len = 100;

        // Compact the data.
        buffer.compact();

        // Buffer state should match
        assert_eq!(buffer.len, 90);
        assert_eq!(buffer.pos, 0);
    }

    #[test]
    fn test_cap_rounding_down() {
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
    fn test_cap_rounding_up() {
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

        buffer.grow();
        assert_eq!(buffer.cap(), 4 * CHUNK_SIZE);

        buffer.grow();
        assert_eq!(buffer.cap(), 8 * CHUNK_SIZE);

        buffer.grow();
        assert_eq!(buffer.cap(), 16 * CHUNK_SIZE);

        buffer.grow();
        assert_eq!(buffer.cap(), 32 * CHUNK_SIZE);
    }

    #[test]
    fn test_shrink() {
        let mut buffer = Buffer::with_capacity(32 * CHUNK_SIZE);

        buffer.len = 15 * CHUNK_SIZE + 1; // simulate unconsumed content
        buffer.shrink();
        assert_eq!(buffer.cap(), 16 * CHUNK_SIZE);

        buffer.len = 7 * CHUNK_SIZE + 1;
        buffer.shrink();
        assert_eq!(buffer.cap(), 8 * CHUNK_SIZE);

        buffer.len = 6 * CHUNK_SIZE - 1;
        buffer.shrink();
        assert_eq!(buffer.cap(), 6 * CHUNK_SIZE);

        buffer.len = 3 * CHUNK_SIZE + 1;
        buffer.shrink();
        assert_eq!(buffer.cap(), 4 * CHUNK_SIZE);

        buffer.len = 2 * CHUNK_SIZE + 1;
        buffer.shrink();
        assert_eq!(buffer.cap(), 3 * CHUNK_SIZE);

        buffer.len = 2 * CHUNK_SIZE;
        buffer.shrink();
        assert_eq!(buffer.cap(), 2 * CHUNK_SIZE);

        buffer.len = CHUNK_SIZE;
        buffer.shrink();
        assert_eq!(buffer.cap(), CHUNK_SIZE);

        buffer.len = 0;
        buffer.shrink();
        assert_eq!(buffer.cap(), CHUNK_SIZE);
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

        assert_eq!(total_read, amount);
        assert_eq!(buffer.len(), amount);
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
    fn test_fill_while() {
        let mut buffer = Buffer::new();

        // TODO
    }

    #[test]
    fn test_fill_until() {
        let mut buffer = Buffer::new();

        // TODO
    }
}
