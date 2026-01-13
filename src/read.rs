use std::io::BufRead;

/// Extension of [`BufRead`] with dynamic buffer capacity management.
///
/// This trait extends the standard [`BufRead`] trait with methods to observe and
/// manage the internal buffer's memory. The buffer grows automatically during read
/// operations as needed, while shrinking is left to the user to control.
///
/// # Capacity
///
/// Use [`capacity()`](Self::capacity) to observe the current buffer size. This is useful
/// for deciding when to reclaim memory with [`shrink()`](Self::shrink).
///
/// # Buffer Operations
///
/// Four methods handle buffer data management:
/// - [`shrink()`](Self::shrink): Reclaims unused memory, shrinking to fit current data
/// - [`compact()`](Self::compact): Moves unconsumed data to the start, making room for more reads
/// - [`clear()`](Self::clear): Abandons all buffered data without changing capacity
/// - [`discard()`](Self::discard): Resets the buffer to an empty, minimal-capacity state
pub trait DynamicRead: BufRead {
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
}
