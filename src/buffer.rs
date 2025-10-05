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
    #[inline]
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
    #[inline]
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
    #[inline]
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

    /// Aligns the given pos to the next char boundary.
    ///
    /// Clamped according to the following constraints: self.pos <= pos <= self.len
    #[expect(
        clippy::arithmetic_side_effects,
        clippy::indexing_slicing,
        reason = "The invariant makes it safe"
    )]
    #[inline]
    pub fn align_pos_to_char(&self, pos: usize) -> usize {
        // Get the start position clamped by: self.pos <= pos <= self.len
        let pos = cmp::max(pos, self.pos).min(self.len);

        // Find the position of first byte that is not a UTF-8 continuation byte
        self.buf[pos..self.len]
            .iter()
            // If the top two bit's are 10 then it's a continuation byte, this bitmask checks that
            .position(|&b| b & 0b1100_0000 != 0b1000_0000)
            .map_or(self.len, |i| i + pos)
    }

    /// Returns a view of the unconsumed buffer data as a UTF-8 string.
    ///
    /// This method automatically skips any partial UTF-8 sequences at the start
    /// (e.g., if `consume()` was called mid-character) and end of the buffer.
    ///
    /// # Errors
    ///
    /// Returns an error if the buffer contains invalid UTF-8 sequences that are
    /// not at the boundaries.
    pub fn as_str(&self) -> io::Result<&str> {
        self.as_str_from(None)
    }

    /// Returns a view of the unconsumed buffer data as a UTF-8 string.
    ///
    /// This method automatically skips any partial UTF-8 sequences at the start
    /// (e.g., if `consume()` was called mid-character) and end of the buffer.
    ///
    /// Takes an optional offset from the start of the buffer to make the str from.
    ///
    /// # Errors
    ///
    /// Returns an error if the buffer contains invalid UTF-8 sequences that are
    /// not at the boundaries.
    #[expect(
        clippy::arithmetic_side_effects,
        clippy::indexing_slicing,
        reason = "The invariant makes it safe"
    )]
    pub fn as_str_from(&self, pos: Option<usize>) -> io::Result<&str> {
        let start = self.align_pos_to_char(pos.unwrap_or(self.pos));

        let mut end = self.len;
        while end > start {
            match str::from_utf8(&self.buf[start..end]) {
                Ok(s) => return Ok(s),
                Err(e) => {
                    if e.error_len().is_none() {
                        // If we have an incomplete sequence at the end, then trim it
                        end = start + e.valid_up_to();
                    } else {
                        // We have a invalid UTF-8 sequence in the middle
                        return Err(io::Error::new(io::ErrorKind::InvalidData, e));
                    }
                }
            }
        }

        Ok("")
    }

    /// Fill buffer while a predicate is true and grow to fit
    ///
    /// This will return the number of valid bytes that where read, not how many bytes where
    /// actually read. That number is likely higher.
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "The invariant makes it safe"
    )]
    pub fn fill_while<P: Fn(char) -> bool>(
        &mut self,
        mut reader: impl Read,
        predicate: P,
    ) -> io::Result<usize> {
        // Get pos aligned to the next char, this is where we start checking the predicate against.
        let mut check_pos = self.align_pos_to_char(self.pos);
        // The number of valid bytes read.
        let mut total_valid_read = 0;

        loop {
            // If the buffer is currently full then start by growing it
            if self.len == self.cap {
                self.grow();
            }

            let read = self.fill(&mut reader)?;

            // EOF detection
            if read == 0 {
                break;
            }

            // Get the new part
            let string = &self.as_str_from(Some(check_pos))?;

            // Look for a non-matching char
            if let Some((byte_index, _)) = string.char_indices().find(|(_, c)| !predicate(*c)) {
                // Add the number of valid bytes until the predicate breaks to the total valid bytes
                // read so far and return it.
                return Ok(total_valid_read + byte_index);
            }

            // All read characters are valid, some boundary bytes may slip into the next iteration
            total_valid_read += string.len();
            // pos + string.len() <= self.len since we may have skipped a partial char
            check_pos += string.len();
        }

        Ok(total_valid_read)
    }

    /// Fill buffer until a delimiter is encountered and grow to fit
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "The invariant makes it safe"
    )]
    pub fn fill_until(&mut self, reader: impl Read, delimiter: char) -> io::Result<usize> {
        let valid_count = self.fill_while(reader, |c| c != delimiter)?;

        // Check if we stopped at the delimiter (not EOF)
        // Align to the char boundary first
        let check_start = self.align_pos_to_char(self.pos);
        let string = &self.as_str_from(Some(check_start + valid_count))?;
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
#[expect(clippy::unwrap_used, reason = "Unwrap is okay in tests")]
#[expect(clippy::indexing_slicing, reason = "Indexing slicing is okay in tests")]
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
    fn test_as_str() {
        // Valid UTF-8
        let mut buffer = Buffer::new();
        buffer.buf[0..13].copy_from_slice(b"Hello, World!");
        buffer.len = 13;
        assert_eq!(buffer.as_str().unwrap(), "Hello, World!");

        // Valid UTF-8 with multi-byte characters
        let mut buffer = Buffer::new();
        let text = "Hello, 世界!";
        buffer.buf[0..text.len()].copy_from_slice(text.as_bytes());
        buffer.len = text.len();
        assert_eq!(buffer.as_str().unwrap(), text);

        // Partial UTF-8 at the start
        let mut buffer = Buffer::new();
        let text = "Hello, 世界!";
        buffer.buf[0..text.len()].copy_from_slice(text.as_bytes());
        buffer.len = text.len();
        // Consume up to the middle of "世" (3-byte UTF-8 character)
        buffer.pos = 8;
        let result = buffer.as_str().unwrap();
        assert_eq!(result, "界!");

        // Partial UTF-8 at the end
        let mut buffer = Buffer::new();
        let text = "Hello, 世界";
        let bytes = text.as_bytes();
        let truncated = bytes.len() - 1;
        buffer.buf[0..truncated].copy_from_slice(&bytes[0..truncated]);
        buffer.len = truncated;
        let result = buffer.as_str().unwrap();
        assert_eq!(result, "Hello, 世");

        // Invalid UTF-8 in the middle
        let mut buffer = Buffer::new();
        buffer.buf[0..5].copy_from_slice(b"Hello");
        // Continuation byte without start
        buffer.buf[5] = 0b1000_0000;
        buffer.buf[6..12].copy_from_slice(b"World!");
        buffer.len = 12;
        assert!(buffer.as_str().is_err());

        // Empty buffer
        let buffer = Buffer::new();
        assert_eq!(buffer.as_str().unwrap(), "");

        // All consumed
        let mut buffer = Buffer::new();
        buffer.buf[0..5].copy_from_slice(b"Hello");
        buffer.len = 5;
        buffer.pos = 5;
        assert_eq!(buffer.as_str().unwrap(), "");
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
}
