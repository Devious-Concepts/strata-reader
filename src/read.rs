use std::io::{self, BufRead};

/// Extension of [`BufRead`] with dynamic buffer capacity management.
///
/// This trait extends the standard [`BufRead`] trait with methods to observe and
/// manage the internal buffer's memory. The buffer grows automatically during read
/// operations as needed, while shrinking is left to the user to control.
///
/// # Buffer Inspection
///
/// Use [`buffer()`](Self::buffer) to access all buffered data (including
/// consumed bytes), [`pos()`](Self::pos) to find where unconsumed data begins,
/// and [`capacity()`](Self::capacity) to observe the total buffer size. These
/// are useful for deciding when to reclaim memory with
/// [`shrink()`](Self::shrink) or [`compact()`](Self::compact).
///
/// # Buffer Operations
///
/// Four methods handle buffer data management:
/// - [`shrink()`](Self::shrink): Reclaims unused memory, shrinking to fit current data
/// - [`compact()`](Self::compact): Moves unconsumed data to the start, making room for more reads
/// - [`clear()`](Self::clear): Abandons all buffered data without changing capacity
/// - [`discard()`](Self::discard): Resets the buffer to an empty, minimal-capacity state
pub trait DynamicRead: BufRead {
    /// Returns all buffered data, including consumed bytes not yet compacted.
    ///
    /// This is a side-effect-free view of the buffer. The unconsumed portion
    /// (as returned by [`fill_buf`](BufRead::fill_buf)) is a suffix of this
    /// slice.
    fn buffer(&self) -> &[u8];

    /// Returns the current read position within the buffer.
    ///
    /// This is the offset where unconsumed data begins. `buffer()[..pos()]`
    /// is consumed data, `buffer()[pos()..]` is unconsumed data.
    fn pos(&self) -> usize;

    /// Returns the current buffer capacity in bytes.
    fn capacity(&self) -> usize;

    /// Shrinks the buffer capacity to fit the current data.
    ///
    /// Reclaims unused memory by reducing the buffer's capacity. The resulting capacity
    /// is implementation-defined but will be sufficient to hold all unconsumed data.
    fn shrink(&mut self);

    /// Moves unconsumed data to the start of the buffer.
    ///
    /// After this operation, the read position is reset to 0 and the buffer has room
    /// for more data without necessarily growing. Capacity remains unchanged.
    fn compact(&mut self);

    /// Discards all buffered data without changing capacity.
    ///
    /// Resets the buffer to an empty state but retains the current capacity allocation.
    fn clear(&mut self);

    /// Discards all buffered data and shrinks to minimal capacity.
    ///
    /// This is equivalent to [`clear()`](Self::clear) followed by [`shrink()`](Self::shrink),
    /// but implementations may optimize this operation.
    fn discard(&mut self) {
        self.clear();
        self.shrink();
    }

    /// Reads from the underlying reader into available buffer space.
    ///
    /// Fills the free space in the current buffer without growing.
    ///
    /// Unlike [`fill_buf`](BufRead::fill_buf), which returns existing data or
    /// reads a fresh chunk after the buffer is exhausted, this method reads
    /// additional data into remaining capacity while preserving existing buffer
    /// contents.
    ///
    /// Returns the number of bytes read. A return of `0` means the buffer is
    /// already full or the underlying reader has no more data.
    fn fill(&mut self) -> io::Result<usize>;

    /// Reads from the underlying reader while `predicate` returns `true`.
    ///
    /// The predicate receives the contents of the internal buffer (including
    /// data from prior reads) and is called before each read attempt. Returning
    /// `false` stops reading immediately.
    ///
    /// Returns the total number of new bytes read into the buffer. A return of
    /// `0` when the predicate still returns `true` means it could not read
    /// more. See the implementor's documentation for specific conditions.
    ///
    /// # Examples
    ///
    /// Reading until a newline is found, capturing its position:
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
