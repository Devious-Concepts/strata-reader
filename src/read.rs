use std::io::{self, BufRead};

/// Extension of [`BufRead`] with dynamic buffer capacity management.
///
/// This trait extends the standard [`BufRead`] trait with methods to observe and manage the
/// internal buffer's memory. The buffer grows automatically during read operations as needed, while
/// shrinking is left to the user to control.
///
/// # Buffer Inspection
///
/// Use [`buffer()`](Self::buffer) to access the data currently retained in the buffer,
/// [`pos()`](Self::pos) to find where unconsumed data begins when consumed data is retained, and
/// [`capacity()`](Self::capacity) to observe the total buffer size. These are useful for deciding
/// when to reclaim memory with [`shrink()`](Self::shrink) or [`compact()`](Self::compact).
///
/// # Buffer Operations
///
/// Four methods handle buffer data management:
/// - [`shrink()`](Self::shrink): Reclaims unused memory, shrinking to fit current data
/// - [`compact()`](Self::compact): Moves unconsumed data to the start, making room for more reads
/// - [`clear()`](Self::clear): Abandons all buffered data without changing capacity
/// - [`discard()`](Self::discard): Resets the buffer to an empty, minimal-capacity state
pub trait DynamicRead: BufRead {
    /// Returns the data currently retained in the buffer.
    ///
    /// Implementations may retain consumed bytes for inspection or seeking. When they do, the
    /// unconsumed portion starts at [`pos()`](Self::pos).
    fn buffer(&self) -> &[u8];

    /// Returns the current read position within the buffer.
    ///
    /// This is the offset where unconsumed data begins.
    fn pos(&self) -> usize;

    /// Returns the current buffer capacity in bytes.
    fn capacity(&self) -> usize;

    /// Shrinks the buffer capacity to fit the current data.
    ///
    /// Reclaims unused memory by reducing the buffer's capacity. The resulting capacity is
    /// implementation-defined but will be sufficient to hold all data.
    fn shrink(&mut self);

    /// Moves unconsumed data to the start of the buffer.
    ///
    /// After this operation, the read position is reset to 0. Capacity remains unchanged.
    fn compact(&mut self);

    /// Clears all buffered data, retaining allocated capacity.
    fn clear(&mut self);

    /// Discards all buffered data and shrinks to minimal capacity.
    ///
    /// This is equivalent to [`clear()`](Self::clear) followed by [`shrink()`](Self::shrink),
    /// but implementations may optimize this operation.
    fn discard(&mut self) {
        self.clear();
        self.shrink();
    }

    /// Performs a single read from the underlying reader into available buffer space.
    ///
    /// Returns the number of bytes read, or `0` if the buffer is full or no more data could be
    /// read.
    fn fill(&mut self) -> io::Result<usize>;

    /// Reads from the underlying reader while `predicate` returns `true`.
    ///
    /// The predicate is called before each read with the current unconsumed data, not just newly
    /// read data.
    ///
    /// For implementations that retain consumed bytes in [`buffer()`](Self::buffer), this is the
    /// portion starting at [`pos()`](Self::pos).
    ///
    /// Returns the total number of new bytes read. A return of `0` when the predicate still returns
    /// `true` means it could not read more. See the implementor's documentation for specific
    /// conditions.
    ///
    /// # Examples
    ///
    /// Reading until a newline is found, capturing its position.
    ///
    /// **Note**: For larger buffers you may want to track checked data to avoid re-checking the
    /// same data.
    ///
    /// ```ignore
    /// let mut newline_pos = None;
    /// let bytes_read = reader.fill_while(|buf| {
    ///     newline_pos = buf.iter().position(|&b| b == b'\n');
    ///     newline_pos.is_none()
    /// })?;
    ///
    /// if let Some(pos) = newline_pos {
    ///     // newline found at `pos`, process the buffer
    /// } else {
    ///     // predicate unsatisfied, could not read more
    /// }
    /// ```
    fn fill_while<P>(&mut self, predicate: P) -> io::Result<usize>
    where
        P: FnMut(&[u8]) -> bool;
}
