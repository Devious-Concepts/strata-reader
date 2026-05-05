use std::io::{self, BufRead};

/// Extension of [`std::io::BufRead`] with dynamic buffer capacity management.
///
/// Provides access to the retained buffer contents and methods to manage its memory. The buffer
/// grows automatically during reads as needed; shrinking is explicit.
///
/// This trait is the retained-buffer surface. Its fill methods preserve retained consumed data
/// while reading. Inherited [`std::io::BufRead`] and [`std::io::Read`] methods may discard retained
/// consumed data when they need to synchronize with the underlying reader.
///
/// Use [`buffer()`](Self::buffer), [`pos()`](Self::pos), and [`capacity()`](Self::capacity) to
/// inspect the retained buffer. Use [`shrink()`](Self::shrink), [`compact()`](Self::compact),
/// [`clear()`](Self::clear), and [`discard()`](Self::discard) to manage it. Use
/// [`fill()`](Self::fill) for a single read or [`fill_while_dyn()`](Self::fill_while_dyn) to read
/// until a predicate is satisfied. Most callers should prefer
/// [`DynamicReadExt::fill_while`].
///
/// Implement `DynamicRead` directly; [`DynamicReadExt`] is blanket-implemented.
pub trait DynamicRead: BufRead {
    /// Returns the data currently retained in the buffer.
    ///
    /// This may include consumed bytes; use [`pos()`](Self::pos) to find where the unconsumed
    /// portion begins.
    fn buffer(&self) -> &[u8];

    /// Returns the offset of the unconsumed portion within [`buffer()`](Self::buffer).
    fn pos(&self) -> usize;

    /// Returns the current buffer capacity in bytes.
    fn capacity(&self) -> usize;

    /// Shrinks the buffer capacity to fit the current data.
    ///
    /// The resulting capacity is implementation-defined but sufficient to hold all retained data.
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

    /// Performs a single read, retrying if interrupted, from the underlying reader into available
    /// buffer space.
    ///
    /// Returns the number of bytes read, or `0` if the buffer is full or no more data could be
    /// read.
    fn fill(&mut self) -> io::Result<usize>;

    /// Reads from the underlying reader while `predicate` returns `true`.
    ///
    /// Object-safe primitive used by [`DynamicReadExt::fill_while`].
    ///
    /// Before each read, `predicate` is called with the current unconsumed data, not just newly
    /// read data.
    ///
    /// Returns the total number of new bytes read. A return of `0` while `predicate` still returns
    /// `true` means it could not read more. See the implementor's documentation for specific stop
    /// conditions.
    fn fill_while_dyn(&mut self, predicate: &mut dyn FnMut(&[u8]) -> bool) -> io::Result<usize>;
}

/// Ergonomic extensions to [`DynamicRead`].
///
/// Provides generic wrappers around object-safe [`DynamicRead`] methods. Blanket-implemented for
/// every `DynamicRead` implementor, including `&mut dyn DynamicRead`, so do not implement it
/// directly.
pub trait DynamicReadExt: DynamicRead {
    /// Reads from the underlying reader while `predicate` returns `true`.
    ///
    /// Generic wrapper around [`DynamicRead::fill_while_dyn`]; see that method for the contract.
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
    fn fill_while<P>(&mut self, mut predicate: P) -> io::Result<usize>
    where
        P: FnMut(&[u8]) -> bool,
    {
        self.fill_while_dyn(&mut predicate)
    }
}

impl<T: DynamicRead + ?Sized> DynamicReadExt for T {}
