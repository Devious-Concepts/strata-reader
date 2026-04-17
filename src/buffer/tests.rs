//! Tests for the Buffer

#![expect(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    reason = "Okay in tests"
)]

use super::*;
use crate::constants::{CHUNK_SIZE, MAX_EXPONENTIAL_CAPACITY, MAX_SUPPORTED_CAPACITY};
use std::io::{self, Cursor, Read};

/// A reader that returns `Interrupted` once, then delegates to inner.
pub(crate) struct InterruptOnceReader<R> {
    pub(crate) inner: R,
    pub(crate) interrupted: bool,
}

impl<R: Read> Read for InterruptOnceReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if !self.interrupted {
            self.interrupted = true;
            return Err(io::Error::new(io::ErrorKind::Interrupted, "interrupted"));
        }
        self.inner.read(buf)
    }
}

impl Buffer {
    /// Test helper to append data directly into the buffer.
    ///
    /// This bypasses the normal fill operations and directly appends to the buffer
    /// contents, useful for testing specific buffer states. It does not validate
    /// that the buffer invariants are maintained.
    ///
    /// # Panics
    ///
    /// Panics if the appended data would exceed the buffer's allocated capacity.
    #[expect(
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        reason = "Used in tests only, so it being unsafe is fine"
    )]
    pub fn inject_test_data(&mut self, data: &[u8]) {
        self.buf[self.len..self.len + data.len()].copy_from_slice(data);
        self.len += data.len();
    }
}

// -----------------------------------------------------------------------------
// FillResult and UnboundedFillResult enums
// -----------------------------------------------------------------------------

#[test]
fn test_fill_result_count() {
    let complete = FillResult::Complete(123);
    assert_eq!(complete.count(), 123);

    let eof = FillResult::Eof(123);
    assert_eq!(eof.count(), 123);
}

#[test]
fn test_unbounded_fill_result_count() {
    let complete = UnboundedFillResult::Complete(123);
    assert_eq!(complete.count(), 123);

    let eof = UnboundedFillResult::Eof(123);
    assert_eq!(eof.count(), 123);

    let capped = UnboundedFillResult::Capped(123);
    assert_eq!(capped.count(), 123);
}

#[test]
fn test_unbounded_fill_result_from_fill_result() {
    let fill_complete = FillResult::Complete(123);
    let fill_eof = FillResult::Eof(123);

    let unbounded_complete = UnboundedFillResult::from(fill_complete);
    let unbounded_eof = UnboundedFillResult::from(fill_eof);

    assert!(matches!(
        unbounded_complete,
        UnboundedFillResult::Complete(123)
    ));
    assert!(matches!(unbounded_eof, UnboundedFillResult::Eof(123)));
}

// -----------------------------------------------------------------------------
// Buffer - Creation
// -----------------------------------------------------------------------------

#[test]
fn test_buffer_new() {
    let buffer = Buffer::new();

    // Check that the internal Vec state matches expectations
    assert!(buffer.buf.capacity() >= CHUNK_SIZE); // Capacity could be larger
    assert_eq!(buffer.buf.len(), CHUNK_SIZE);

    // Check internal state matches expectations
    assert_eq!(buffer.cap, CHUNK_SIZE);
    assert_eq!(buffer.len, 0);
    assert_eq!(buffer.pos, 0);
}

#[test]
fn test_buffer_with_capacity() {
    let buffer = Buffer::with_capacity(123 * CHUNK_SIZE); // Accepts linear multiple

    // Check that the internal Vec state matches expectations
    assert!(buffer.buf.capacity() >= 123 * CHUNK_SIZE); // Capacity could be larger
    assert_eq!(buffer.buf.len(), 123 * CHUNK_SIZE);

    // Check internal state matches expectations
    assert_eq!(buffer.cap, 123 * CHUNK_SIZE);
    assert_eq!(buffer.len, 0);
    assert_eq!(buffer.pos, 0);
}

/* Coverage note: `with_capacity` relies on `cap_up_linear` for capacity rounding.
 * That behavior is covered by the `cap_up_linear` tests.
 */

// -----------------------------------------------------------------------------
// Buffer - Accessors and is_empty
// -----------------------------------------------------------------------------

#[test]
fn test_buffer_buf() {
    let mut buffer = Buffer::new();

    // Starts empty
    assert_eq!(buffer.buf(), &[]);

    // Manually push data to internal vec for testing
    let data = "Hello, World!";
    buffer.inject_test_data(data.as_bytes());

    // Accessor should now return `data`
    assert_eq!(buffer.buf(), data.as_bytes());
}

#[test]
fn test_buffer_cap() {
    let buffer = Buffer::with_capacity(5 * CHUNK_SIZE); // Not power-of-2 multiple

    // Accessor should return the set capacity
    assert_eq!(buffer.cap(), 5 * CHUNK_SIZE);
}

#[test]
fn test_buffer_len() {
    let mut buffer = Buffer::new();

    // Starts at 0
    assert_eq!(buffer.len(), 0);

    // Inject test data so the buffer is no longer empty
    buffer.inject_test_data(&[b'x'; 123]);

    // Accessor should reflect this
    assert_eq!(buffer.len(), 123);
}

#[test]
fn test_buffer_is_empty() {
    let mut buffer = Buffer::new();

    // Starts empty
    assert!(buffer.is_empty());

    // Inject test data so the buffer is no longer empty
    buffer.inject_test_data(&[b'x'; 123]);

    // No longer empty
    assert!(!buffer.is_empty());
}

#[test]
fn test_buffer_pos() {
    let mut buffer = Buffer::new();

    // Starts at 0
    assert_eq!(buffer.pos(), 0);

    // Inject test data, then mark it consumed to set up the position
    buffer.inject_test_data(&[b'x'; 123]);
    buffer.pos = 123; // consumed manually sine we've not tested consume() yet

    // Accessor should reflect this
    assert_eq!(buffer.pos(), 123);
}

// -----------------------------------------------------------------------------
// Buffer - State mutation
// -----------------------------------------------------------------------------

#[test]
fn test_buffer_clear() {
    let mut buffer = Buffer::new();

    // Calling it on an empty buffer shouldn't change anything
    buffer.clear();

    // It should still be empty
    assert!(buffer.is_empty());
    assert_eq!(buffer.len(), 0);
    assert_eq!(buffer.pos(), 0);

    // Calling it again still shouldn't change anything
    buffer.clear();

    // It should still be empty
    assert!(buffer.is_empty());
    assert_eq!(buffer.len(), 0);
    assert_eq!(buffer.pos(), 0);

    // Inject test data, then mark part of it consumed
    buffer.inject_test_data(&[b'x'; 456]); // More data than is consumed
    buffer.pos = 123; // consumed manually sine we've not tested consume() yet

    // Clearing the buffer should make it empty
    buffer.clear();

    // It should now be empty
    assert!(buffer.is_empty());
    assert_eq!(buffer.len(), 0);
    assert_eq!(buffer.pos(), 0);
}

/* Coverage note: `discard` is composed of `clear` and `shrink`.
 * Its behavior is covered by the `clear` and `shrink_targeted` tests.
 */

#[test]
fn test_buffer_consume() {
    let mut buffer = Buffer::new();

    // Calling consume on an empty buffer shouldn't move pos
    buffer.consume(123);

    // It should not have moved pos
    assert_eq!(buffer.pos(), 0);

    // Calling consume again still shouldn't move pos
    buffer.consume(123);

    // It should not have moved pos
    assert_eq!(buffer.pos(), 0);

    // Inject test data so consume operates on real contents
    buffer.inject_test_data(&[b'x'; 4567]); // almost half a CHUNK_SIZE

    // Consuming some bytes should now move pos
    buffer.consume(123);

    // It should have moved pos to match
    assert_eq!(buffer.pos(), 123);

    // Consuming some more bytes should still move pos
    buffer.consume(333);

    // It should have moved pos to match the total consumed
    assert_eq!(buffer.pos(), 456);

    // Consuming more than available should cap out
    buffer.consume(5000);

    // It should match the len we set
    assert_eq!(buffer.pos(), 4567);

    // Consuming when all is already consumed should not change anything
    buffer.consume(123);

    // It should still match the len we set
    assert_eq!(buffer.pos(), 4567);
}

#[test]
fn test_buffer_unconsume() {
    let mut buffer = Buffer::new();

    // Calling unconsume on an empty buffer should cap out
    buffer.unconsume(123);

    // It should not have moved pos
    assert_eq!(buffer.pos(), 0);

    // Inject test data so unconsume operates on real contents
    buffer.inject_test_data(&[b'x'; 4567]); // almost half a CHUNK_SIZE

    // Calling unconsume with only unconsumed data should cap out
    buffer.unconsume(123);

    // It should not have moved pos
    assert_eq!(buffer.pos(), 0);

    // Consume part of the data so we can unconsume
    buffer.consume(3000);

    // Unconsuming some data should move pos
    buffer.unconsume(123);

    // It should move pos backwards
    assert_eq!(buffer.pos(), 2877); // = 3000 - 123

    // Unconsuming some more bytes should still move pos
    buffer.unconsume(877);

    // It should have moved pos further backwards
    assert_eq!(buffer.pos(), 2000); // = 2877 - 877

    // Unconsuming more than available should cap out
    buffer.unconsume(5000);

    // It should unconsume all data
    assert_eq!(buffer.pos(), 0);
}

#[test]
fn test_buffer_compact() {
    let mut buffer = Buffer::with_capacity(8 * CHUNK_SIZE); // Power-of-2 multiple

    // Calling it on an empty buffer shouldn't change anything
    buffer.compact();

    // It should still be empty
    assert!(buffer.is_empty());
    assert_eq!(buffer.len(), 0);
    assert_eq!(buffer.pos(), 0);

    // Calling it again still shouldn't change anything
    buffer.compact();

    // It should still be empty
    assert!(buffer.is_empty());
    assert_eq!(buffer.len(), 0);
    assert_eq!(buffer.pos(), 0);

    // Manually push data to internal vec for testing
    let raw = "Hello, World!".repeat(36); // 13 × 36 > 456
    buffer.inject_test_data(&raw.as_bytes()[..456]);

    // Why 456? Because I said so…
    buffer.consume(123);

    // It should match the following now
    assert_eq!(&buffer.buf()[..13], "Hello, World!".as_bytes());
    assert!(!buffer.is_empty());
    assert_eq!(buffer.len(), 456);
    assert_eq!(buffer.pos(), 123);

    // Compacting the buffer should work now
    buffer.compact();

    // It should match the following now
    assert_eq!(
        &buffer.buf()[..13],
        " World!Hello,".as_bytes() // 123 % 13 = 6, so I rotated the string left by 6
    );
    assert!(!buffer.is_empty());
    assert_eq!(buffer.len(), 333);
    assert_eq!(buffer.pos(), 0);

    // Consume the rest of the data
    buffer.consume(333);

    // Compacting the buffer should make it empty
    buffer.compact();

    // It should now be empty
    assert!(buffer.is_empty());
    assert_eq!(buffer.len(), 0);
    assert_eq!(buffer.pos(), 0);
}

// -----------------------------------------------------------------------------
// Buffer - Capacity rounding
// -----------------------------------------------------------------------------

#[test]
fn test_buffer_cap_down() {
    // Sizes below `CHUNK_SIZE` should round up to `CHUNK_SIZE` as that's the min
    assert_eq!(Buffer::cap_down(usize::MIN), CHUNK_SIZE);
    assert_eq!(Buffer::cap_down(CHUNK_SIZE - 1), CHUNK_SIZE);

    // Other values should round down to nearest linear multiple of `CHUNK_SIZE`
    assert_eq!(Buffer::cap_down(CHUNK_SIZE + 123), CHUNK_SIZE);
    assert_eq!(Buffer::cap_down(2 * CHUNK_SIZE + 123), 2 * CHUNK_SIZE);
    assert_eq!(Buffer::cap_down(5 * CHUNK_SIZE + 456), 5 * CHUNK_SIZE);
    assert_eq!(Buffer::cap_down(20 * CHUNK_SIZE + 789), 20 * CHUNK_SIZE);
    assert_eq!(Buffer::cap_down(123 * CHUNK_SIZE + 1234), 123 * CHUNK_SIZE);
    assert_eq!(
        Buffer::cap_down(MAX_SUPPORTED_CAPACITY - 1),
        MAX_SUPPORTED_CAPACITY - CHUNK_SIZE
    );
    assert_eq!(
        Buffer::cap_down(MAX_SUPPORTED_CAPACITY),
        MAX_SUPPORTED_CAPACITY
    );

    /* Sizes above `MAX_SUPPORTED_CAPACITY` should round down to
    `MAX_SUPPORTED_CAPACITY` as that's the max */
    assert_eq!(Buffer::cap_down(usize::MAX), MAX_SUPPORTED_CAPACITY);
    assert_eq!(
        Buffer::cap_down(MAX_SUPPORTED_CAPACITY + 1234),
        MAX_SUPPORTED_CAPACITY
    );
    assert_eq!(
        Buffer::cap_down(MAX_SUPPORTED_CAPACITY + 1),
        MAX_SUPPORTED_CAPACITY
    );

    // Exact sizes should stay the same
    assert_eq!(Buffer::cap_down(3 * CHUNK_SIZE), 3 * CHUNK_SIZE);
    assert_eq!(
        Buffer::cap_down(MAX_SUPPORTED_CAPACITY - CHUNK_SIZE),
        MAX_SUPPORTED_CAPACITY - CHUNK_SIZE
    );
}

#[test]
fn test_buffer_cap_up() {
    /* Sizes above `MAX_SUPPORTED_CAPACITY` should round down to
    `MAX_SUPPORTED_CAPACITY` as that's the max */
    assert_eq!(Buffer::cap_up(usize::MAX), MAX_SUPPORTED_CAPACITY);
    assert_eq!(
        Buffer::cap_up(MAX_SUPPORTED_CAPACITY + 1234),
        MAX_SUPPORTED_CAPACITY
    );
    assert_eq!(
        Buffer::cap_up(MAX_SUPPORTED_CAPACITY + 1),
        MAX_SUPPORTED_CAPACITY
    );

    // Other values should round up to the nearest power-of-2 multiple of `CHUNK_SIZE`
    assert_eq!(Buffer::cap_up(CHUNK_SIZE - 123), CHUNK_SIZE);
    assert_eq!(Buffer::cap_up(2 * CHUNK_SIZE - 123), 2 * CHUNK_SIZE);
    assert_eq!(Buffer::cap_up(5 * CHUNK_SIZE - 456), 8 * CHUNK_SIZE);
    assert_eq!(Buffer::cap_up(20 * CHUNK_SIZE - 789), 32 * CHUNK_SIZE);
    assert_eq!(Buffer::cap_up(123 * CHUNK_SIZE - 1234), 128 * CHUNK_SIZE);
    assert_eq!(
        Buffer::cap_up(MAX_SUPPORTED_CAPACITY - 1),
        MAX_SUPPORTED_CAPACITY
    );
    assert_eq!(
        Buffer::cap_up(MAX_EXPONENTIAL_CAPACITY + 1),
        MAX_SUPPORTED_CAPACITY
    );

    // Sizes below `CHUNK_SIZE` should round up to `CHUNK_SIZE` as that's the min
    assert_eq!(Buffer::cap_up(usize::MIN), CHUNK_SIZE);
    assert_eq!(Buffer::cap_up(CHUNK_SIZE - 1), CHUNK_SIZE);

    // Exact sizes should stay the same
    assert_eq!(Buffer::cap_up(4 * CHUNK_SIZE), 4 * CHUNK_SIZE);
    assert_eq!(
        Buffer::cap_up(MAX_EXPONENTIAL_CAPACITY),
        MAX_EXPONENTIAL_CAPACITY
    );
}

#[test]
fn test_buffer_cap_up_linear() {
    /* Sizes above `MAX_SUPPORTED_CAPACITY` should round down to
    `MAX_SUPPORTED_CAPACITY` as that's the max */
    assert_eq!(Buffer::cap_up_linear(usize::MAX), MAX_SUPPORTED_CAPACITY);
    assert_eq!(
        Buffer::cap_up_linear(MAX_SUPPORTED_CAPACITY + 1234),
        MAX_SUPPORTED_CAPACITY
    );
    assert_eq!(
        Buffer::cap_up_linear(MAX_SUPPORTED_CAPACITY + 1),
        MAX_SUPPORTED_CAPACITY
    );

    // Other values should round up to nearest linear multiple of `CHUNK_SIZE`
    assert_eq!(Buffer::cap_up_linear(CHUNK_SIZE - 123), CHUNK_SIZE);
    assert_eq!(Buffer::cap_up_linear(2 * CHUNK_SIZE - 123), 2 * CHUNK_SIZE);
    assert_eq!(Buffer::cap_up_linear(5 * CHUNK_SIZE - 456), 5 * CHUNK_SIZE);
    assert_eq!(
        Buffer::cap_up_linear(20 * CHUNK_SIZE - 789),
        20 * CHUNK_SIZE
    );
    assert_eq!(
        Buffer::cap_up_linear(123 * CHUNK_SIZE - 1234),
        123 * CHUNK_SIZE
    );
    assert_eq!(
        Buffer::cap_up_linear(MAX_SUPPORTED_CAPACITY - 1),
        MAX_SUPPORTED_CAPACITY // Note: `MAX_SUPPORTED_CAPACITY` is a multiple of `CHUNK_SIZE`
    );
    assert_eq!(
        Buffer::cap_up_linear(MAX_SUPPORTED_CAPACITY - CHUNK_SIZE + 1),
        MAX_SUPPORTED_CAPACITY
    );

    // Sizes below `CHUNK_SIZE` should round up to `CHUNK_SIZE` as that's the min
    assert_eq!(Buffer::cap_up_linear(usize::MIN), CHUNK_SIZE);
    assert_eq!(Buffer::cap_up_linear(CHUNK_SIZE - 1), CHUNK_SIZE);

    // Exact sizes should stay the same
    assert_eq!(Buffer::cap_up_linear(3 * CHUNK_SIZE), 3 * CHUNK_SIZE);
    assert_eq!(
        Buffer::cap_up_linear(MAX_SUPPORTED_CAPACITY - CHUNK_SIZE),
        MAX_SUPPORTED_CAPACITY - CHUNK_SIZE
    );
    assert_eq!(
        Buffer::cap_up_linear(MAX_SUPPORTED_CAPACITY),
        MAX_SUPPORTED_CAPACITY
    );
}

// -----------------------------------------------------------------------------
// Buffer - Capacity control
// -----------------------------------------------------------------------------

/* Coverage note: `grow` is a thin wrapper over `grow_targeted`.
 * Its behavior is covered by the `grow_targeted` tests.
 */

#[test]
fn test_buffer_grow_targeted() {
    let mut buffer = Buffer::new(); // Starts at CHUNK_SIZE

    // Double-check the starting cap
    assert_eq!(buffer.cap(), CHUNK_SIZE);
    assert!(buffer.buf.capacity() >= CHUNK_SIZE); // Also double check the internals
    assert_eq!(buffer.buf.len(), CHUNK_SIZE);

    /* Assertion note: these tests only assert `buffer.buf.capacity() >= target`.
     * Allocation uses an "at least" strategy, so no stable upper bound is assumed.
     */

    // Growing with a target equal to the current cap is a no-op
    buffer.grow_targeted(CHUNK_SIZE);
    assert_eq!(buffer.cap(), CHUNK_SIZE);
    assert!(buffer.buf.capacity() >= CHUNK_SIZE); // Also double check the internals
    assert_eq!(buffer.buf.len(), CHUNK_SIZE);

    // Grow to a target that is an exact power-of-2 multiple, no overshoot
    buffer.grow_targeted(4 * CHUNK_SIZE);
    assert_eq!(buffer.cap(), 4 * CHUNK_SIZE);
    assert!(buffer.buf.capacity() >= 4 * CHUNK_SIZE); // Also double check the internals
    assert_eq!(buffer.buf.len(), 4 * CHUNK_SIZE);

    // Growing with a target below the current cap is also a no-op, even usize::MIN
    buffer.grow_targeted(usize::MIN); // rounds to CHUNK_SIZE, which is less than 4 * CHUNK_SIZE
    assert_eq!(buffer.cap(), 4 * CHUNK_SIZE);
    assert!(buffer.buf.capacity() >= 4 * CHUNK_SIZE); // Also double check the internals
    assert_eq!(buffer.buf.len(), 4 * CHUNK_SIZE);

    // Grow to a non-aligned target to show rounding to the next power-of-2 multiple
    buffer.grow_targeted(5 * CHUNK_SIZE); // not a power-of-2 multiple
    assert_eq!(buffer.cap(), 8 * CHUNK_SIZE);
    assert!(buffer.buf.capacity() >= 8 * CHUNK_SIZE); // Also double check the internals
    assert_eq!(buffer.buf.len(), 8 * CHUNK_SIZE);

    // Grow another step
    buffer.grow_targeted(16 * CHUNK_SIZE);
    assert_eq!(buffer.cap(), 16 * CHUNK_SIZE);
    assert!(buffer.buf.capacity() >= 16 * CHUNK_SIZE); // Also double check the internals
    assert_eq!(buffer.buf.len(), 16 * CHUNK_SIZE);

    // And again
    buffer.grow_targeted(64 * CHUNK_SIZE);
    assert_eq!(buffer.cap(), 64 * CHUNK_SIZE);
    assert!(buffer.buf.capacity() >= 64 * CHUNK_SIZE); // Also double check the internals
    assert_eq!(buffer.buf.len(), 64 * CHUNK_SIZE);

    // And finally a large non-aligned jump
    buffer.grow_targeted(200 * CHUNK_SIZE - 123); // not a power-of-2 multiple
    assert_eq!(buffer.cap(), 256 * CHUNK_SIZE);
    assert!(buffer.buf.capacity() >= 256 * CHUNK_SIZE); // Also double check the internals
    assert_eq!(buffer.buf.len(), 256 * CHUNK_SIZE);
}

#[test]
fn test_buffer_grow_targeted_linear() {
    let mut buffer = Buffer::new(); // Starts at CHUNK_SIZE

    // Double-check the starting cap
    assert_eq!(buffer.cap(), CHUNK_SIZE);
    assert!(buffer.buf.capacity() >= CHUNK_SIZE); // Also double check the internals
    assert_eq!(buffer.buf.len(), CHUNK_SIZE);

    /* Assertion note: these tests only assert `buffer.buf.capacity() >= target`.
     * Allocation uses an "at least" strategy, so no stable upper bound is assumed.
     */

    // Growing with a target equal to the current cap is a no-op
    buffer.grow_targeted_linear(CHUNK_SIZE);
    assert_eq!(buffer.cap(), CHUNK_SIZE);
    assert!(buffer.buf.capacity() >= CHUNK_SIZE); // Also double check the internals
    assert_eq!(buffer.buf.len(), CHUNK_SIZE);

    // Grow to a target that is an exact multiple, no overshoot
    buffer.grow_targeted_linear(3 * CHUNK_SIZE);
    assert_eq!(buffer.cap(), 3 * CHUNK_SIZE);
    assert!(buffer.buf.capacity() >= 3 * CHUNK_SIZE); // Also double check the internals
    assert_eq!(buffer.buf.len(), 3 * CHUNK_SIZE);

    // Growing with a target below the current cap is also a no-op, even usize::MIN
    buffer.grow_targeted_linear(usize::MIN); // rounds to CHUNK_SIZE, still below 3 * CHUNK_SIZE
    assert_eq!(buffer.cap(), 3 * CHUNK_SIZE);
    assert!(buffer.buf.capacity() >= 3 * CHUNK_SIZE); // Also double check the internals
    assert_eq!(buffer.buf.len(), 3 * CHUNK_SIZE);

    // Grow to a non-aligned target to show rounding to the next multiple
    buffer.grow_targeted_linear(5 * CHUNK_SIZE - 123); // not a power-of-2 multiple
    assert_eq!(buffer.cap(), 5 * CHUNK_SIZE);
    assert!(buffer.buf.capacity() >= 5 * CHUNK_SIZE); // Also double check the internals
    assert_eq!(buffer.buf.len(), 5 * CHUNK_SIZE);

    // Grow another step
    buffer.grow_targeted_linear(10 * CHUNK_SIZE);
    assert_eq!(buffer.cap(), 10 * CHUNK_SIZE);
    assert!(buffer.buf.capacity() >= 10 * CHUNK_SIZE); // Also double check the internals
    assert_eq!(buffer.buf.len(), 10 * CHUNK_SIZE);

    // And again
    buffer.grow_targeted_linear(20 * CHUNK_SIZE);
    assert_eq!(buffer.cap(), 20 * CHUNK_SIZE);
    assert!(buffer.buf.capacity() >= 20 * CHUNK_SIZE); // Also double check the internals
    assert_eq!(buffer.buf.len(), 20 * CHUNK_SIZE);

    // And finally a large non-aligned jump
    buffer.grow_targeted_linear(200 * CHUNK_SIZE - 123); // not a multiple
    assert_eq!(buffer.cap(), 200 * CHUNK_SIZE);
    assert!(buffer.buf.capacity() >= 200 * CHUNK_SIZE); // Also double check the internals
    assert_eq!(buffer.buf.len(), 200 * CHUNK_SIZE);
}

/* Coverage note: `shrink` is a thin wrapper over `shrink_targeted`.
 * Its behavior is covered by the `shrink_targeted` tests.
 */

#[test]
fn test_buffer_shrink_targeted() {
    let mut buffer = Buffer::with_capacity(260 * CHUNK_SIZE); // Not power-of-2 multiple

    // Manually push data to internal vec for testing, we need it for the first set of tests
    let data = "Hello, World!"
        .as_bytes()
        .iter()
        .cycle()
        .take(250 * CHUNK_SIZE - 123)
        .copied()
        .collect::<Vec<_>>();
    buffer.inject_test_data(data.as_slice());

    // Double-check the starting cap after our meddling
    assert_eq!(buffer.cap(), 260 * CHUNK_SIZE);
    assert!(buffer.buf.capacity() >= 260 * CHUNK_SIZE); // Also double check the internals
    assert_eq!(buffer.buf.len(), 260 * CHUNK_SIZE);

    /* Assertion note: these tests only assert `buffer.buf.capacity() >= target`.
     * Allocation uses an "at least" strategy, so no stable upper bound is assumed.
     */

    // We have just under `250 * CHUNK_SIZE` of data, so data length becomes the shrink target.
    buffer.shrink_targeted(usize::MIN); // target gets rounded to `CHUNK_SIZE` here
    assert_eq!(buffer.cap(), 250 * CHUNK_SIZE);
    assert!(buffer.buf.capacity() >= 250 * CHUNK_SIZE); // Also double check the internals
    assert_eq!(buffer.buf.len(), 250 * CHUNK_SIZE);

    // Delete some data
    buffer.consume(150 * CHUNK_SIZE);
    buffer.compact();

    // Shrink to a target greater than our data length
    buffer.shrink_targeted(200 * CHUNK_SIZE - 123); // not aligned, to show we round
    assert_eq!(buffer.cap(), 200 * CHUNK_SIZE);
    assert!(buffer.buf.capacity() >= 200 * CHUNK_SIZE); // Also double check the internals
    assert_eq!(buffer.buf.len(), 200 * CHUNK_SIZE);

    // And again for good measure
    buffer.shrink_targeted(150 * CHUNK_SIZE);
    assert_eq!(buffer.cap(), 150 * CHUNK_SIZE);
    assert!(buffer.buf.capacity() >= 150 * CHUNK_SIZE); // Also double check the internals
    assert_eq!(buffer.buf.len(), 150 * CHUNK_SIZE);

    // Now with a lower target than the data length again to show the data length is the target
    buffer.shrink_targeted(16 * CHUNK_SIZE); // arbitrary
    assert_eq!(buffer.cap(), 100 * CHUNK_SIZE);
    assert!(buffer.buf.capacity() >= 100 * CHUNK_SIZE); // Also double check the internals
    assert_eq!(buffer.buf.len(), 100 * CHUNK_SIZE);

    // Delete all data
    buffer.clear(); // soft delete, technically

    // Shrinking with a target is now required to not hit the min cap of `CHUNK_SIZE`
    buffer.shrink_targeted(30 * CHUNK_SIZE);
    assert_eq!(buffer.cap(), 30 * CHUNK_SIZE);
    assert!(buffer.buf.capacity() >= 30 * CHUNK_SIZE); // Also double check the internals
    assert_eq!(buffer.buf.len(), 30 * CHUNK_SIZE);

    // Let's do it again
    buffer.shrink_targeted(15 * CHUNK_SIZE);
    assert_eq!(buffer.cap(), 15 * CHUNK_SIZE);
    assert!(buffer.buf.capacity() >= 15 * CHUNK_SIZE); // Also double check the internals
    assert_eq!(buffer.buf.len(), 15 * CHUNK_SIZE);

    // And again
    buffer.shrink_targeted(8 * CHUNK_SIZE);
    assert_eq!(buffer.cap(), 8 * CHUNK_SIZE);
    assert!(buffer.buf.capacity() >= 8 * CHUNK_SIZE); // Also double check the internals
    assert_eq!(buffer.buf.len(), 8 * CHUNK_SIZE);

    // Shrinking with a target lower than the min of `CHUNK_SIZE` still stops at `CHUNK_SIZE`
    buffer.shrink_targeted(usize::MIN);
    assert_eq!(buffer.cap(), CHUNK_SIZE);
    assert!(buffer.buf.capacity() >= CHUNK_SIZE); // Also double check the internals
    assert_eq!(buffer.buf.len(), CHUNK_SIZE);
}

// -----------------------------------------------------------------------------
// Buffer - read_once
// -----------------------------------------------------------------------------

#[test]
fn test_buffer_read_once() {
    // Interrupt retry: read_once retries on Interrupted, then succeeds
    let mut buffer = Buffer::new();
    let mut reader = InterruptOnceReader {
        inner: Cursor::new("Hello!"),
        interrupted: false,
    };
    let mut total = 0;
    let result = buffer.read_once(&mut reader, &mut total, Some(buffer.cap()));

    assert_eq!(result.unwrap(), ReadOnce::Read);
    assert_eq!(total, 6);
    assert_eq!(buffer.buf(), b"Hello!");
}

/* Coverage note: `read_once` Read, Eof, Capped, and growth paths are exercised
 * by the `fill`, `fill_amount`, `fill_to_end`, and `fill_while` tests.
 */

// -----------------------------------------------------------------------------
// Buffer - Fill
// -----------------------------------------------------------------------------

#[test]
fn test_buffer_fill() {
    let mut buffer = Buffer::new();
    let data = "Hello, World!";
    let rem = CHUNK_SIZE % data.len();

    // `CHUNK_SIZE` must not divide `data.len()` evenly or this test won't work.
    assert_ne!(rem, 0);

    // Let's start with an EOF test
    let cur = Cursor::new("");
    let read = buffer.fill(cur).unwrap();

    // We should not have read any bytes
    assert_eq!(read, FillResult::Eof(0));
    assert_eq!(buffer.len(), 0);

    // While there is enough space to add `data` once more
    while buffer.len() + data.len() < buffer.cap() {
        let cur = Cursor::new(data);
        let read = buffer.fill(cur).unwrap();

        // All bytes should have been read and the data should match
        assert_eq!(read, FillResult::Complete(data.len()));
        assert_eq!(
            &buffer.buf()[(buffer.len() - read.count())..],
            data.as_bytes()
        );
    }

    // The remaining space should match `rem`
    assert_eq!(buffer.cap() - buffer.len(), rem);

    // Fill the last bit
    let cur = Cursor::new(data);
    let read = buffer.fill(cur).unwrap();

    // Only `rem` bytes should have been read and the data should match
    assert_eq!(read, FillResult::Complete(rem));
    assert_eq!(
        &buffer.buf()[(buffer.len() - read.count())..],
        &data.as_bytes()[..read.count()]
    );
    assert_eq!(buffer.len(), buffer.cap());

    // Try to fill once more now that we're full
    let cur = Cursor::new(data);
    let read = buffer.fill(cur).unwrap();

    // We should have not read anything
    assert_eq!(read, FillResult::Complete(0));
}

#[test]
fn test_buffer_fill_amount() {
    let mut buffer = Buffer::new();
    let raw = "Hello, World!";
    let data = raw.repeat(6000); // > 8 × `CHUNK_SIZE`

    // Let's start with an EOF test
    let cur = Cursor::new("");
    let read = buffer.fill_amount(cur, 123).unwrap();

    // We should not have read any bytes
    assert_eq!(read, FillResult::Eof(0));
    assert_eq!(buffer.len(), 0);

    // This time, read a bit before hitting EOF
    let cur = Cursor::new(raw);
    let read = buffer.fill_amount(cur, 123).unwrap();

    // We should have one instance of `raw`
    assert_eq!(read, FillResult::Eof(raw.len()));
    assert_eq!(buffer.len(), raw.len());
    assert_eq!(buffer.buf(), raw.as_bytes());

    // Fill the buffer to more data for the next test, reads are "at least"
    let mut cur = Cursor::new(&data);
    let read = buffer.fill_amount(&mut cur, 2 * CHUNK_SIZE).unwrap();

    // We should have 3 × `CHUNK_SIZE` bytes of data as there were bytes left in the buffer
    assert_eq!(read, FillResult::Complete(3 * CHUNK_SIZE - raw.len()));
    assert_eq!(buffer.len(), 3 * CHUNK_SIZE);

    // Discard everything for a clean slate
    buffer.discard();

    // Read a big amount of data, a bit more than 2 × `CHUNK_SIZE`
    let cur = Cursor::new(&data);
    let read = buffer.fill_amount(cur, 2 * CHUNK_SIZE + 1).unwrap();

    // We should have 3 × `CHUNK_SIZE` bytes of data since that's the next multiple
    assert_eq!(read, FillResult::Complete(3 * CHUNK_SIZE));
    assert_eq!(buffer.len(), 3 * CHUNK_SIZE);
    assert_eq!(buffer.buf(), &data.as_bytes()[..buffer.len()]); // Data should match

    // Discard everything for a clean slate
    buffer.discard();

    /* Test error when requesting more than the buffer can accommodate. The buffer cannot grow
    beyond `MAX_SUPPORTED_CAPACITY`, so
    `amt > MAX_SUPPORTED_CAPACITY - self.len` is an unfulfillable request. */
    let cur = Cursor::new(&data);
    let result = buffer.fill_amount(cur, MAX_SUPPORTED_CAPACITY + 1);

    // We should get an InvalidInput error
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidInput);

    // Even with some data already in the buffer, requesting more than the remaining capacity errors
    let cur = Cursor::new(&data);
    buffer.fill_amount(cur, CHUNK_SIZE).unwrap(); // Add some data first

    let cur = Cursor::new(&data);
    let result = buffer.fill_amount(
        cur,
        MAX_SUPPORTED_CAPACITY, // Now this exceeds remaining capacity
    );

    // We should get an InvalidInput error
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidInput);
}

/* Coverage note: `fill_amount` relies on `grow_targeted_linear`, `read_once`,
 * and `shrink_targeted` on EOF. Those helpers are tested separately; this
 * section covers `fill_amount`-specific behavior.
 */

#[test]
fn test_buffer_fill_exact() {
    let mut buffer = Buffer::new();
    let raw = "Hello, World!";
    let data = raw.repeat(6000); // > 8 × `CHUNK_SIZE`

    // Let's start with an EOF test (requesting more than available)
    let cur = Cursor::new(raw);
    let result = buffer.fill_exact(
        cur, 100, // Request more than `raw.len()`
    );

    // We should get an UnexpectedEof error
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::UnexpectedEof);

    // Buffer state is unspecified after error, so discard and start fresh
    buffer.discard();

    // Now let's read exactly what's available
    let cur = Cursor::new(raw);
    buffer.fill_exact(cur, raw.len()).unwrap();

    // We should have exactly `raw.len()` bytes
    assert_eq!(buffer.len(), raw.len());
    assert_eq!(buffer.buf(), raw.as_bytes());

    // Fill more data on top of existing data
    let mut cur = Cursor::new(&data);
    buffer.fill_exact(&mut cur, 2 * CHUNK_SIZE).unwrap();

    // We should have `raw.len() + 2 × CHUNK_SIZE` bytes
    assert_eq!(buffer.len(), raw.len() + 2 * CHUNK_SIZE);

    // Discard everything for a clean slate
    buffer.discard();

    // Read a big amount of data, specifically 3 × `CHUNK_SIZE` bytes
    let cur = Cursor::new(&data);
    buffer.fill_exact(cur, 3 * CHUNK_SIZE).unwrap();

    // We should have exactly 3 × `CHUNK_SIZE` bytes
    assert_eq!(buffer.len(), 3 * CHUNK_SIZE);
    assert_eq!(buffer.buf(), &data.as_bytes()[..buffer.len()]); // Data should match

    // Discard everything for a clean slate
    buffer.discard();

    /* Test error when requesting more than the buffer can accommodate. The buffer cannot grow
    beyond `MAX_SUPPORTED_CAPACITY`, so
    `amt > MAX_SUPPORTED_CAPACITY - self.len` is an unfulfillable request. */
    let cur = Cursor::new(&data);
    let result = buffer.fill_exact(cur, MAX_SUPPORTED_CAPACITY + 1);

    // We should get an InvalidInput error
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidInput);

    // Even with some data already in the buffer, requesting more than the remaining capacity errors
    let cur = Cursor::new(&data);
    buffer.fill_exact(cur, CHUNK_SIZE).unwrap(); // Add some data first

    let cur = Cursor::new(&data);
    let result = buffer.fill_exact(
        cur,
        MAX_SUPPORTED_CAPACITY, // Now this exceeds remaining capacity
    );

    // We should get an InvalidInput error
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidInput);
}

/* Coverage note: `fill_exact` relies on `grow_targeted_linear` for preallocation.
 * That helper is tested separately; this section covers `fill_exact`-specific behavior.
 */

// -----------------------------------------------------------------------------
// Buffer - fill_to_end
// -----------------------------------------------------------------------------

#[test]
fn test_buffer_fill_to_end() {
    let mut buffer = Buffer::new();

    // Let's start with an empty reader (immediate EOF)
    let cur = Cursor::new("");
    let read = buffer.fill_to_end(cur).unwrap();

    // We should not have read any bytes
    assert_eq!(read, 0);
    assert_eq!(buffer.len(), 0);
    assert_eq!(buffer.cap(), CHUNK_SIZE); // Capacity unchanged

    // Read some data that fits within a single chunk
    let raw = "Hello, World!";
    let cur = Cursor::new(raw);
    let read = buffer.fill_to_end(cur).unwrap();

    // We should have read all the data
    assert_eq!(read, raw.len());
    assert_eq!(buffer.len(), raw.len());
    assert_eq!(buffer.buf(), raw.as_bytes());
    assert_eq!(buffer.cap(), CHUNK_SIZE); // No growth needed

    // Discard everything for a clean slate
    buffer.discard();

    // Read data that exactly fills one chunk; final capacity should stay at one chunk
    let data = raw.repeat(4000); // > 5 × `CHUNK_SIZE`
    let cur = Cursor::new(&data.as_bytes()[..CHUNK_SIZE]);
    let read = buffer.fill_to_end(cur).unwrap();

    // We should have read exactly one chunk
    assert_eq!(read, CHUNK_SIZE);
    assert_eq!(buffer.len(), CHUNK_SIZE);
    assert_eq!(buffer.buf(), &data.as_bytes()[..buffer.len()]); // Data should match
    assert_eq!(buffer.cap(), CHUNK_SIZE); // Grew to 2× during the read, then shrunk back

    // Discard everything for a clean slate
    buffer.discard();

    // Read data that spans multiple chunks to exercise growth
    let cur = Cursor::new(&data.as_bytes()[..5 * CHUNK_SIZE]);
    let read = buffer.fill_to_end(cur).unwrap();

    // We should have read all the data
    assert_eq!(read, 5 * CHUNK_SIZE);
    assert_eq!(buffer.len(), 5 * CHUNK_SIZE);
    assert_eq!(buffer.buf(), &data.as_bytes()[..buffer.len()]); // Data should match
    // Capacity should have shrunk back to fit the data (linear rounding)
    assert_eq!(buffer.cap(), 5 * CHUNK_SIZE);

    // Discard everything for a clean slate
    buffer.discard();

    // Read data that is not a multiple of `CHUNK_SIZE` to test shrink behavior
    let cur = Cursor::new(&data.as_bytes()[..3 * CHUNK_SIZE + 123]);
    let read = buffer.fill_to_end(cur).unwrap();

    // We should have read all the data
    assert_eq!(read, 3 * CHUNK_SIZE + 123);
    assert_eq!(buffer.len(), 3 * CHUNK_SIZE + 123);
    assert_eq!(buffer.buf(), &data.as_bytes()[..buffer.len()]); // Data should match
    // Capacity should be the next linear multiple above the data length
    assert_eq!(buffer.cap(), 4 * CHUNK_SIZE);

    // Discard everything for a clean slate
    buffer.discard();

    // Read into a buffer that already has data to test appending behavior
    let cur = Cursor::new(raw);
    buffer.fill_to_end(cur).unwrap();

    let cur = Cursor::new(&data.as_bytes()[..2 * CHUNK_SIZE]);
    let read = buffer.fill_to_end(cur).unwrap();

    // We should have appended to the existing data
    assert_eq!(read, 2 * CHUNK_SIZE);
    assert_eq!(buffer.len(), raw.len() + 2 * CHUNK_SIZE);
    // The first part should still be intact
    assert_eq!(&buffer.buf()[..raw.len()], raw.as_bytes());
    // Capacity should accommodate the combined data
    assert_eq!(buffer.cap(), 3 * CHUNK_SIZE);
}

/* Coverage note: `fill_to_end` relies on `read_once` and `shrink_targeted`.
 * Those behaviors are tested separately; this section covers method-specific behavior.
 */

// -----------------------------------------------------------------------------
// Buffer - fill_while
// -----------------------------------------------------------------------------

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "Exercises the full fill_while behavior matrix in one place"
)]
fn test_buffer_fill_while() {
    let mut buffer = Buffer::new();
    let raw = "Hello, World!";
    let data = raw.repeat(6000); // > 8 × `CHUNK_SIZE`

    // Let's start with an empty reader (immediate EOF)
    let cur = Cursor::new("");
    let read = buffer.fill_while(cur, |_| true, None).unwrap();

    // We should not have read any bytes
    assert_eq!(read, UnboundedFillResult::Eof(0));
    assert_eq!(buffer.len(), 0);
    assert_eq!(buffer.cap(), CHUNK_SIZE); // Capacity unchanged

    // Read some data where the predicate stops mid-stream (looking for a space)
    let cur = Cursor::new(raw);
    let read = buffer
        .fill_while(cur, |d| !d.contains(&b' '), None)
        .unwrap();

    // The predicate should have stopped, and the buffer should contain a space
    assert!(matches!(read, UnboundedFillResult::Complete(_)));
    assert!(buffer.buf().contains(&b' '));
    assert_eq!(buffer.cap(), CHUNK_SIZE); // No growth needed

    // Discard everything for a clean slate
    buffer.discard();

    // Read data where the predicate is never satisfied, should reach EOF
    let cur = Cursor::new(raw);
    let read = buffer
        .fill_while(cur, |d| !d.contains(&b'\n'), None)
        .unwrap();

    // We should have hit EOF with all the data read
    assert_eq!(read, UnboundedFillResult::Eof(raw.len()));
    assert_eq!(buffer.len(), raw.len());
    assert_eq!(buffer.buf(), raw.as_bytes());

    // Discard everything for a clean slate
    buffer.discard();

    // Read data that spans multiple chunks to exercise growth
    let cur = Cursor::new(&data.as_bytes()[..5 * CHUNK_SIZE]);
    let read = buffer
        .fill_while(cur, |d| !d.contains(&b'\n'), None)
        .unwrap();

    // We should have hit EOF after reading all the data
    assert_eq!(read, UnboundedFillResult::Eof(5 * CHUNK_SIZE));
    assert_eq!(buffer.len(), 5 * CHUNK_SIZE);
    assert_eq!(buffer.buf(), &data.as_bytes()[..buffer.len()]); // Data should match
    // Capacity should have shrunk back to fit the data (linear rounding)
    assert_eq!(buffer.cap(), 5 * CHUNK_SIZE);

    // Discard everything for a clean slate
    buffer.discard();

    // Read into a buffer that already has data to test the predicate sees full unconsumed data
    buffer.inject_test_data(b"pre:");
    let cur = Cursor::new(raw);
    let read = buffer
        .fill_while(
            cur,
            |d| {
                // The predicate should always see the "pre:" prefix
                assert!(d.starts_with(b"pre:"));
                !d.contains(&b' ')
            },
            None,
        )
        .unwrap();

    // The predicate should have stopped once it saw the space in "Hello, World!"
    assert!(matches!(read, UnboundedFillResult::Complete(_)));
    assert_eq!(&buffer.buf()[..4], b"pre:");

    // Discard everything for a clean slate
    buffer.discard();

    // Read with consumed data (pos > 0), the predicate should only see unconsumed data
    buffer.inject_test_data(b"consumed:kept:");
    buffer.consume(9); // consume "consumed:"

    let cur = Cursor::new(raw);
    let read = buffer
        .fill_while(
            cur,
            |d| {
                // Should see "kept:" prefix, not "consumed:"
                assert!(d.starts_with(b"kept:"));
                !d.contains(&b' ')
            },
            None,
        )
        .unwrap();

    // The predicate should have stopped
    assert!(matches!(read, UnboundedFillResult::Complete(_)));

    // Discard everything for a clean slate
    buffer.discard();

    // Test growth_limit, cap at `CHUNK_SIZE` with a reader larger than that
    let big_data = vec![b'a'; CHUNK_SIZE * 3];
    let cur = Cursor::new(&big_data[..]);
    let read = buffer.fill_while(cur, |_| true, Some(CHUNK_SIZE)).unwrap();

    // We should have hit the capacity ceiling
    assert!(matches!(read, UnboundedFillResult::Capped(_)));
    assert_eq!(buffer.len(), CHUNK_SIZE); // Filled to cap
    assert_eq!(buffer.cap(), CHUNK_SIZE); // Did not grow beyond

    // Discard everything for a clean slate
    buffer.discard();

    // Test growth_limit with `None`, behaves like unbounded, reads to EOF
    let cur = Cursor::new(&data.as_bytes()[..4 * CHUNK_SIZE]);
    let read = buffer.fill_while(cur, |_| true, None).unwrap();

    // We should have hit EOF
    assert_eq!(read, UnboundedFillResult::Eof(4 * CHUNK_SIZE));
    assert_eq!(buffer.len(), 4 * CHUNK_SIZE);
    assert_eq!(buffer.buf(), &data.as_bytes()[..buffer.len()]); // Data should match

    // Discard everything for a clean slate
    buffer.discard();

    // Condition already fulfilled in existing data, should return without reading
    buffer.inject_test_data(b"has a space here");
    let cur = Cursor::new(b"unreachable");
    let read = buffer
        .fill_while(cur, |d| !d.contains(&b' '), None)
        .unwrap();

    // Should complete immediately with 0 bytes read
    assert_eq!(read, UnboundedFillResult::Complete(0));
    assert_eq!(buffer.buf(), b"has a space here");

    // Discard everything for a clean slate
    buffer.discard();

    /* If the read that reaches the growth limit also makes the predicate fail, the result should be
    `Complete`, not `Capped`. `Capped` is only returned before a read when another read would be
    needed but no more growth is allowed */
    buffer.inject_test_data(&vec![b'a'; CHUNK_SIZE]);

    let mut reader_data = vec![b'b'; CHUNK_SIZE - 512];
    reader_data.push(b'\0'); // Delimiter that makes the predicate false
    reader_data.extend_from_slice(&[b'c'; 511]); // Pad to exactly one chunk

    let read = buffer
        .fill_while(
            Cursor::new(&reader_data),
            |data| !data.contains(&b'\0'),
            Some(2 * CHUNK_SIZE),
        )
        .unwrap();

    assert_eq!(read, UnboundedFillResult::Complete(CHUNK_SIZE));
    assert_eq!(buffer.len(), 2 * CHUNK_SIZE);
    assert!(buffer.buf().contains(&b'\0'));
    assert!(buffer.buf()[..CHUNK_SIZE].iter().all(|&b| b == b'a'));

    // Test non-aligned growth_limit, it should round up and then degrade to linear at the ceiling
    let mut buffer = Buffer::with_capacity(4 * CHUNK_SIZE);
    let big_data = vec![b'a'; CHUNK_SIZE * 6];
    let cur = Cursor::new(&big_data[..]);
    let read = buffer
        .fill_while(cur, |_| true, Some(5 * CHUNK_SIZE - 1))
        .unwrap();

    // We should have filled exactly to the rounded ceiling without overshooting to 8 * CHUNK_SIZE
    assert_eq!(read, UnboundedFillResult::Capped(5 * CHUNK_SIZE));
    assert_eq!(buffer.len(), 5 * CHUNK_SIZE);
    assert_eq!(buffer.cap(), 5 * CHUNK_SIZE);
}

/* Coverage note: `fill_while` relies on `read_once` and `shrink_targeted`.
 * Those behaviors are tested separately; this section covers method-specific behavior.
 */

// -----------------------------------------------------------------------------
// Buffer - fill_until
// -----------------------------------------------------------------------------

#[test]
fn test_buffer_fill_until() {
    let mut buffer = Buffer::new();
    let raw = "Hello, World!";
    let data = raw.repeat(6000); // > 8 × `CHUNK_SIZE`

    // Let's start with an empty reader (immediate EOF)
    let cur = Cursor::new("");
    let read = buffer.fill_until(cur, b'\n', None).unwrap();

    // We should not have read any bytes
    assert_eq!(read, UnboundedFillResult::Eof(0));
    assert_eq!(buffer.len(), 0);
    assert_eq!(buffer.cap(), CHUNK_SIZE); // Capacity unchanged

    // Read some data where the delimiter is present
    let cur = Cursor::new(b"key=value\nother");
    let read = buffer.fill_until(cur, b'\n', None).unwrap();

    // The delimiter should have been found
    assert!(matches!(read, UnboundedFillResult::Complete(_)));
    assert!(buffer.buf().contains(&b'\n'));
    assert_eq!(buffer.cap(), CHUNK_SIZE); // No growth needed

    // Discard everything for a clean slate
    buffer.discard();

    // Read data where the delimiter is never found, should reach EOF
    let cur = Cursor::new(raw);
    let read = buffer.fill_until(cur, b'\n', None).unwrap();

    // We should have hit EOF with all the data read
    assert_eq!(read, UnboundedFillResult::Eof(raw.len()));
    assert_eq!(buffer.len(), raw.len());
    assert_eq!(buffer.buf(), raw.as_bytes());

    // Discard everything for a clean slate
    buffer.discard();

    // Read data that spans multiple chunks to exercise growth
    let cur = Cursor::new(&data.as_bytes()[..5 * CHUNK_SIZE]);
    let read = buffer.fill_until(cur, b'\n', None).unwrap();

    // We should have hit EOF after reading all the data (no newlines in `raw`)
    assert_eq!(read, UnboundedFillResult::Eof(5 * CHUNK_SIZE));
    assert_eq!(buffer.len(), 5 * CHUNK_SIZE);
    assert_eq!(buffer.buf(), &data.as_bytes()[..buffer.len()]); // Data should match
    // Capacity should have shrunk back to fit the data (linear rounding)
    assert_eq!(buffer.cap(), 5 * CHUNK_SIZE);

    // Discard everything for a clean slate
    buffer.discard();

    // Read into a buffer that already has data
    buffer.inject_test_data(b"pre:");
    let cur = Cursor::new(b"data\nmore");
    let read = buffer.fill_until(cur, b'\n', None).unwrap();

    // The delimiter should have been found
    assert!(matches!(read, UnboundedFillResult::Complete(_)));
    assert_eq!(&buffer.buf()[..4], b"pre:");
    assert!(buffer.buf().contains(&b'\n'));

    // Discard everything for a clean slate
    buffer.discard();

    // Read with consumed data (pos > 0), search should only cover unconsumed data
    buffer.inject_test_data(b"consumed\nkept:");
    buffer.consume(9); // consume "consumed\n"

    // The delimiter is in the consumed part, so a search for '\n' should NOT find it
    let cur = Cursor::new(b"more data");
    let read = buffer.fill_until(cur, b'\n', None).unwrap();

    // We should have hit EOF, no newline in the unconsumed region
    assert!(matches!(read, UnboundedFillResult::Eof(_)));

    // Discard everything for a clean slate
    buffer.discard();

    // Test growth_limit, cap at `CHUNK_SIZE` with a reader larger than that
    let big_data = vec![b'a'; CHUNK_SIZE * 3];
    let cur = Cursor::new(&big_data[..]);
    let read = buffer.fill_until(cur, b'\n', Some(CHUNK_SIZE)).unwrap();

    // We should have hit the capacity ceiling
    assert!(matches!(read, UnboundedFillResult::Capped(_)));
    assert_eq!(buffer.len(), CHUNK_SIZE); // Filled to cap
    assert_eq!(buffer.cap(), CHUNK_SIZE); // Did not grow beyond

    // Discard everything for a clean slate
    buffer.discard();

    // Delimiter already present in existing data, should return without reading
    buffer.inject_test_data(b"already\nhere");
    let cur = Cursor::new(b"unreachable");
    let read = buffer.fill_until(cur, b'\n', None).unwrap();

    // Should complete immediately with 0 bytes read
    assert_eq!(read, UnboundedFillResult::Complete(0));
    assert_eq!(buffer.buf(), b"already\nhere");

    // Test non-aligned growth_limit, it should round up and then degrade to linear at the ceiling
    let mut buffer = Buffer::with_capacity(4 * CHUNK_SIZE);
    let big_data = vec![b'a'; CHUNK_SIZE * 6];
    let cur = Cursor::new(&big_data[..]);
    let read = buffer
        .fill_until(cur, b'\n', Some(5 * CHUNK_SIZE - 1))
        .unwrap();

    // We should have filled exactly to the rounded ceiling without overshooting to 8 * CHUNK_SIZE
    assert_eq!(read, UnboundedFillResult::Capped(5 * CHUNK_SIZE));
    assert_eq!(buffer.len(), 5 * CHUNK_SIZE);
    assert_eq!(buffer.cap(), 5 * CHUNK_SIZE);
}

/* Coverage note: `fill_until` relies on `read_once` and `shrink_targeted`.
 * Those behaviors are tested separately; this section covers method-specific behavior.
 */

// -----------------------------------------------------------------------------
// Buffer - UTF-8 Helpers
// -----------------------------------------------------------------------------

/* Test data note: these UTF-8 tests use `"Hello, 世界!"` to exercise char boundaries.
 * Byte layout: "Hello, " = 7 bytes, '世' = 3 bytes, '界' = 3 bytes, '!' = 1 byte.
 */

#[test]
fn test_buffer_align_pos_to_char() {
    let mut buffer = Buffer::new();
    let data = "Hello, 世界!"; // World in Japanese
    let cur = Cursor::new(data);
    buffer.fill(cur).unwrap();

    // Let's start with an offset that is already on a char boundary
    let aligned = buffer.align_pos_to_char(5);
    assert_eq!(aligned, 5); // Start of ','

    // Next let's try one byte into '世', this should return the start offset
    let aligned = buffer.align_pos_to_char(8);
    assert_eq!(aligned, 7); // Aligns back to start of '世'

    // And again with the second byte into '世'
    let aligned = buffer.align_pos_to_char(9);
    assert_eq!(aligned, 7); // Still aligns back to start of '世'

    // The offset should clamp to len as it's max
    let aligned = buffer.align_pos_to_char(100);
    assert_eq!(aligned, buffer.len()); // Clamped to len

    // The offset should clamp to pos as it's min
    buffer.consume(10); // Now pos = 10 (start of '界')
    let aligned = buffer.align_pos_to_char(2);
    assert_eq!(aligned, 10); // Clamped to pos

    // Discard everything for a clean slate
    buffer.discard();

    // For an edge-case test, let's use some random non-UTF-8 bytes
    let garbage: [u8; 8] = [
        0b1010_1010, // 0xAA - valid continuation byte (10xxxxxx)
        0b1100_0000, // 0xC0 - invalid: would start 2-byte seq but is overlong
        0b1011_1111, // 0xBF - valid continuation byte (10xxxxxx)
        0b1000_0000, // 0x80 - valid continuation byte (10xxxxxx)
        0b1111_1000, // 0xF8 - invalid: would start 5-byte seq (not valid UTF-8)
        0b1111_1111, // 0xFF - invalid: all bits set (not valid UTF-8)
        0b1100_0001, // 0xC1 - invalid: would start 2-byte seq but is overlong
        0b1001_0101, // 0x95 - valid continuation byte (10xxxxxx)
    ];
    let cur = Cursor::new(garbage);
    buffer.fill(cur).unwrap();

    // Should find 0xC0 at position 1
    let aligned = buffer.align_pos_to_char(3);
    assert_eq!(aligned, 1);

    // Should find 0xF8 at position 4
    let aligned = buffer.align_pos_to_char(4);
    assert_eq!(aligned, 4);

    // Discard everything for a clean slate
    buffer.discard();

    // Full buffer (len == cap): must not panic.
    buffer.inject_test_data(&[b'A'; CHUNK_SIZE]);
    assert_eq!(buffer.len(), buffer.cap());
    let aligned = buffer.align_pos_to_char(buffer.len());
    assert_eq!(aligned, buffer.len());

    // Discard everything for a clean slate
    buffer.discard();

    // Complete trailing multi-byte char: 世 = [0xE4, 0xB8, 0x96].
    // self.len is a valid boundary → returns self.len.
    buffer.inject_test_data(b"Hello\xE4\xB8\x96");
    let aligned = buffer.align_pos_to_char(buffer.len());
    assert_eq!(aligned, buffer.len());

    // Discard everything for a clean slate
    buffer.discard();

    // Incomplete trailing 3-byte sequence: first 2 bytes of 世.
    // self.len is mid-character → returns start of the incomplete sequence.
    buffer.inject_test_data(b"Hello\xE4\xB8");
    let aligned = buffer.align_pos_to_char(buffer.len());
    assert_eq!(aligned, 5); // start of the incomplete sequence

    // Discard everything for a clean slate
    buffer.discard();

    // Incomplete trailing 4-byte sequence: only the leading byte.
    buffer.inject_test_data(b"Hello\xF0");
    let aligned = buffer.align_pos_to_char(buffer.len());
    assert_eq!(aligned, 5);

    // Discard everything for a clean slate
    buffer.discard();

    // Empty region (pos == len): returns self.len.
    assert_eq!(buffer.pos(), buffer.len());
    let aligned = buffer.align_pos_to_char(buffer.len());
    assert_eq!(aligned, buffer.len());
}

#[test]
fn test_buffer_align_pos_to_next_char() {
    let mut buffer = Buffer::new();
    let data = "Hello, 世界!";
    let cur = Cursor::new(data);
    buffer.fill(cur).unwrap();

    // Let's start with an offset that is already on a char boundary
    let aligned = buffer.align_pos_to_next_char(5);
    assert_eq!(aligned, 5); // Start of ','

    // Next let's try one byte into '世', this should return the start offset of the next char
    let aligned = buffer.align_pos_to_next_char(8);
    assert_eq!(aligned, 10); // Aligns forward to start of '界'

    // And again with the second byte into '世'
    let aligned = buffer.align_pos_to_next_char(9);
    assert_eq!(aligned, 10); // Still aligns forward to start of '界'

    // The offset should clamp to len as it's max
    let aligned = buffer.align_pos_to_next_char(100);
    assert_eq!(aligned, buffer.len()); // Clamped to len

    // The offset should clamp to pos as it's min
    buffer.consume(10); // Now pos = 10 (start of '界')
    let aligned = buffer.align_pos_to_next_char(2);
    assert_eq!(aligned, 10); // Clamped to pos

    // Discard everything for a clean slate
    buffer.discard();

    // For an edge-case test, let's use some random non-UTF-8 bytes
    let garbage: [u8; 8] = [
        0b1010_1010, // 0xAA - valid continuation byte (10xxxxxx)
        0b1100_0000, // 0xC0 - invalid: would start 2-byte seq but is overlong
        0b1011_1111, // 0xBF - valid continuation byte (10xxxxxx)
        0b1000_0000, // 0x80 - valid continuation byte (10xxxxxx)
        0b1111_1000, // 0xF8 - invalid: would start 5-byte seq (not valid UTF-8)
        0b1111_1111, // 0xFF - invalid: all bits set (not valid UTF-8)
        0b1100_0001, // 0xC1 - invalid: would start 2-byte seq but is overlong
        0b1001_0101, // 0x95 - valid continuation byte (10xxxxxx)
    ];
    let cur = Cursor::new(garbage);
    buffer.fill(cur).unwrap();

    // Should find 0xF8 at position 4
    let aligned = buffer.align_pos_to_next_char(2);
    assert_eq!(aligned, 4);

    // Should find 0xFF at position 5
    let aligned = buffer.align_pos_to_next_char(5);
    assert_eq!(aligned, 5);
}

/* Coverage note: `as_str` is a thin wrapper over `as_str_from(self.pos())`.
 * Its behavior is covered by the `as_str_from` tests.
 */

#[test]
fn test_buffer_as_str_from() {
    let mut buffer = Buffer::new();
    let data = "Hello, 世界!";
    let cur = Cursor::new(data);
    buffer.fill(cur).unwrap();

    // Get the full string from the start
    let text = buffer.as_str_from(0).unwrap();
    assert_eq!(text, "Hello, 世界!");

    // Get substring from middle (on char boundary)
    let text = buffer.as_str_from(7).unwrap();
    assert_eq!(text, "世界!");

    // Get substring from middle of multibyte char (should skip to next char)
    let text = buffer.as_str_from(8).unwrap();
    assert_eq!(text, "界!"); // Skips incomplete '世'

    // Get substring from end
    let text = buffer.as_str_from(14).unwrap();
    assert_eq!(text, ""); // At end, returns empty string

    // Get substring beyond len (should clamp to len)
    let text = buffer.as_str_from(100).unwrap();
    assert_eq!(text, ""); // Beyond len, returns empty string

    // Test with partial UTF-8 at the end
    let mut buffer = Buffer::new();
    let full_data = "Hello, 世界";
    let bytes = full_data.as_bytes();
    let truncated = bytes.len() - 1; // Remove last byte of '界'
    let cur = Cursor::new(&bytes[0..truncated]);
    buffer.fill(cur).unwrap();

    let text = buffer.as_str_from(0).unwrap();
    assert_eq!(text, "Hello, 世"); // Incomplete '界' at end is trimmed

    // Test with invalid UTF-8 in the middle (not at boundaries)
    let mut buffer = Buffer::new();
    let mut invalid_utf8 = Vec::from(b"Hello");
    invalid_utf8.push(0b1000_0000); // Invalid continuation byte without start
    invalid_utf8.extend_from_slice(b"World!");
    buffer.inject_test_data(&invalid_utf8);

    let result = buffer.as_str_from(0);
    assert!(result.is_err()); // Should error on invalid UTF-8

    // Test with consumed data (pos > 0)
    let mut buffer = Buffer::new();
    let data = "Hello, World!";
    let cur = Cursor::new(data);
    buffer.fill(cur).unwrap();
    buffer.consume(7); // Consume "Hello, "

    // Trying to access before pos should clamp to pos
    let text = buffer.as_str_from(0).unwrap();
    assert_eq!(text, "World!"); // Clamped to pos=7

    let text = buffer.as_str_from(7).unwrap();
    assert_eq!(text, "World!"); // At pos
}

/* Coverage note: extra UTF-8 width cases are omitted here.
 * Boundary handling is covered by `align_pos_to_next_char`; this test focuses on UTF-8 validation.
 */

// -----------------------------------------------------------------------------
// Buffer - fill_until_char
// -----------------------------------------------------------------------------

#[test]
fn test_buffer_fill_until_char() {
    let mut buffer = Buffer::new();

    // Let's start with an empty reader (immediate EOF)
    let cur = Cursor::new("");
    let read = buffer.fill_until_char(cur, '界', None).unwrap();

    // We should not have read any bytes
    assert_eq!(read, UnboundedFillResult::Eof(0));
    assert_eq!(buffer.len(), 0);
    assert_eq!(buffer.cap(), CHUNK_SIZE); // Capacity unchanged

    // Read some data containing a multibyte char delimiter
    let cur = Cursor::new("Hello, 世界!");
    let read = buffer.fill_until_char(cur, '界', None).unwrap();

    // The delimiter should have been found
    assert!(matches!(read, UnboundedFillResult::Complete(_)));
    assert_eq!(buffer.cap(), CHUNK_SIZE); // No growth needed

    // Discard everything for a clean slate
    buffer.discard();

    // Read with an ASCII char delimiter
    let cur = Cursor::new(b"key=value\nother");
    let read = buffer.fill_until_char(cur, '\n', None).unwrap();

    // The delimiter should have been found
    assert!(matches!(read, UnboundedFillResult::Complete(_)));
    assert!(buffer.buf().contains(&b'\n'));

    // Discard everything for a clean slate
    buffer.discard();

    // Read data where the char delimiter is never found, should reach EOF
    let cur = Cursor::new("Hello, World!");
    let read = buffer.fill_until_char(cur, '界', None).unwrap();

    // We should have hit EOF
    assert!(matches!(read, UnboundedFillResult::Eof(_)));
    assert_eq!(buffer.len(), 13);

    // Discard everything for a clean slate
    buffer.discard();

    // Test growth_limit, cap at `CHUNK_SIZE` with a reader larger than that
    let big_data = "a".repeat(CHUNK_SIZE * 3);
    let cur = Cursor::new(big_data.as_bytes());
    let read = buffer.fill_until_char(cur, '界', Some(CHUNK_SIZE)).unwrap();

    // We should have hit the capacity ceiling
    assert!(matches!(read, UnboundedFillResult::Capped(_)));
    assert_eq!(buffer.len(), CHUNK_SIZE); // Filled to cap
    assert_eq!(buffer.cap(), CHUNK_SIZE); // Did not grow beyond

    // Discard everything for a clean slate
    buffer.discard();

    // Delimiter already present in existing data, should return without reading
    buffer.inject_test_data("Hello, 世界!".as_bytes());
    let cur = Cursor::new("unreachable");
    let read = buffer.fill_until_char(cur, '界', None).unwrap();

    // Should complete immediately with 0 bytes read
    assert_eq!(read, UnboundedFillResult::Complete(0));
    assert_eq!(buffer.buf(), "Hello, 世界!".as_bytes());

    // Test non-aligned growth_limit, it should round up and then degrade to linear at the ceiling
    let mut buffer = Buffer::with_capacity(4 * CHUNK_SIZE);
    let big_data = "a".repeat(CHUNK_SIZE * 6);
    let cur = Cursor::new(big_data.as_bytes());
    let read = buffer
        .fill_until_char(cur, '界', Some(5 * CHUNK_SIZE - 1))
        .unwrap();

    // We should have filled exactly to the rounded ceiling without overshooting to 8 * CHUNK_SIZE
    assert_eq!(read, UnboundedFillResult::Capped(5 * CHUNK_SIZE));
    assert_eq!(buffer.len(), 5 * CHUNK_SIZE);
    assert_eq!(buffer.cap(), 5 * CHUNK_SIZE);
}

/* Coverage note: `fill_until_char` delegates to `fill_until_str`.
 * The shared behavior is covered by the `fill_until_str` and `shrink_targeted` tests.
 */

// -----------------------------------------------------------------------------
// Buffer - fill_until_str
// -----------------------------------------------------------------------------

#[test]
fn test_buffer_fill_until_str() {
    let mut buffer = Buffer::new();
    let raw = "Hello, World!";
    let data = raw.repeat(6000); // > 8 × `CHUNK_SIZE`

    // Let's start with an empty needle, should return immediately
    let cur = Cursor::new(raw);
    let read = buffer.fill_until_str(cur, "", None).unwrap();

    // We should get Complete(0) without reading anything
    assert_eq!(read, UnboundedFillResult::Complete(0));
    assert_eq!(buffer.len(), 0);

    // Let's try with an empty reader (immediate EOF)
    let cur = Cursor::new("");
    let read = buffer.fill_until_str(cur, "END", None).unwrap();

    // We should not have read any bytes
    assert_eq!(read, UnboundedFillResult::Eof(0));
    assert_eq!(buffer.len(), 0);
    assert_eq!(buffer.cap(), CHUNK_SIZE); // Capacity unchanged

    // Read some data where the needle is present
    let cur = Cursor::new(b"Hello, World!END more data");
    let read = buffer.fill_until_str(cur, "END", None).unwrap();

    // The needle should have been found
    assert!(matches!(read, UnboundedFillResult::Complete(_)));
    assert_eq!(buffer.cap(), CHUNK_SIZE); // No growth needed

    // Discard everything for a clean slate
    buffer.discard();

    // Read data where the needle is never found, should reach EOF
    let cur = Cursor::new(raw);
    let read = buffer.fill_until_str(cur, "END", None).unwrap();

    // We should have hit EOF with all the data read
    assert_eq!(read, UnboundedFillResult::Eof(raw.len()));
    assert_eq!(buffer.len(), raw.len());
    assert_eq!(buffer.buf(), raw.as_bytes());

    // Discard everything for a clean slate
    buffer.discard();

    // Read data that spans multiple chunks to exercise growth
    let cur = Cursor::new(&data.as_bytes()[..5 * CHUNK_SIZE]);
    let read = buffer.fill_until_str(cur, "END", None).unwrap();

    // We should have hit EOF after reading all the data (no "END" in `raw`)
    assert_eq!(read, UnboundedFillResult::Eof(5 * CHUNK_SIZE));
    assert_eq!(buffer.len(), 5 * CHUNK_SIZE);
    assert_eq!(buffer.buf(), &data.as_bytes()[..buffer.len()]); // Data should match
    // Capacity should have shrunk back to fit the data (linear rounding)
    assert_eq!(buffer.cap(), 5 * CHUNK_SIZE);

    // Discard everything for a clean slate
    buffer.discard();

    // Read into a buffer that already has data
    buffer.inject_test_data(b"pre:");
    let cur = Cursor::new(b"dataENDmore");
    let read = buffer.fill_until_str(cur, "END", None).unwrap();

    // The needle should have been found
    assert!(matches!(read, UnboundedFillResult::Complete(_)));
    assert_eq!(&buffer.buf()[..4], b"pre:");

    // Discard everything for a clean slate
    buffer.discard();

    // Read with consumed data (pos > 0), search should only cover unconsumed data
    buffer.inject_test_data(b"consumedENDkept:");
    buffer.consume(11); // consume "consumedEND"

    // The needle is in the consumed part, so a search for "END" should NOT find it
    let cur = Cursor::new(b"more data");
    let read = buffer.fill_until_str(cur, "END", None).unwrap();

    // We should have hit EOF, no "END" in the unconsumed region
    assert!(matches!(read, UnboundedFillResult::Eof(_)));

    // Discard everything for a clean slate
    buffer.discard();

    // Test growth_limit, cap at `CHUNK_SIZE` with a reader larger than that
    let big_data = vec![b'a'; CHUNK_SIZE * 3];
    let cur = Cursor::new(&big_data[..]);
    let read = buffer.fill_until_str(cur, "END", Some(CHUNK_SIZE)).unwrap();

    // We should have hit the capacity ceiling
    assert!(matches!(read, UnboundedFillResult::Capped(_)));
    assert_eq!(buffer.len(), CHUNK_SIZE); // Filled to cap
    assert_eq!(buffer.cap(), CHUNK_SIZE); // Did not grow beyond

    // Discard everything for a clean slate
    buffer.discard();

    // Test with a multibyte needle to ensure the search works with UTF-8
    let cur = Cursor::new("Hello, 世界! The end.");
    let read = buffer.fill_until_str(cur, "世界", None).unwrap();

    // The needle should have been found
    assert!(matches!(read, UnboundedFillResult::Complete(_)));

    // Discard everything for a clean slate
    buffer.discard();

    // Needle already present in existing data, should return without reading
    buffer.inject_test_data(b"Hello, World!END more data");
    let cur = Cursor::new(b"unreachable");
    let read = buffer.fill_until_str(cur, "END", None).unwrap();

    // Should complete immediately with 0 bytes read
    assert_eq!(read, UnboundedFillResult::Complete(0));
    assert_eq!(buffer.buf(), b"Hello, World!END more data");

    // Test non-aligned growth_limit, it should round up and then degrade to linear at the ceiling
    let mut buffer = Buffer::with_capacity(4 * CHUNK_SIZE);
    let big_data = vec![b'a'; CHUNK_SIZE * 6];
    let cur = Cursor::new(&big_data[..]);
    let read = buffer
        .fill_until_str(cur, "END", Some(5 * CHUNK_SIZE - 1))
        .unwrap();

    // We should have filled exactly to the rounded ceiling without overshooting to 8 * CHUNK_SIZE
    assert_eq!(read, UnboundedFillResult::Capped(5 * CHUNK_SIZE));
    assert_eq!(buffer.len(), 5 * CHUNK_SIZE);
    assert_eq!(buffer.cap(), 5 * CHUNK_SIZE);
}

/* Coverage note: `fill_until_str` relies on `read_once` and `shrink_targeted`.
 * Those behaviors are tested separately; this section covers method-specific behavior.
 */

// -----------------------------------------------------------------------------
// Buffer - Clone, PartialEq, Debug, and Default
// -----------------------------------------------------------------------------

#[test]
fn test_buffer_clone() {
    let mut buffer = Buffer::with_capacity(4 * CHUNK_SIZE);
    buffer.inject_test_data(b"abcdef");
    buffer.pos = 2;

    // Write distinct bytes into spare capacity beyond the logical `len`.
    buffer.buf[buffer.len] = b'x';
    buffer.buf[buffer.cap - 1] = b'y';

    // Clone preserves the logical contents and read position.
    let cloned = buffer.clone();
    assert_eq!(cloned, buffer);
    assert_eq!(cloned.buf(), b"abcdef");
    assert_eq!(cloned.pos(), 2);

    // Clone rebuilds storage from initialized bytes only.
    assert_eq!(cloned.cap(), CHUNK_SIZE);
    assert!(cloned.buf[cloned.len..].iter().all(|&byte| byte == 0));
}

#[test]
fn test_buffer_partial_eq() {
    let mut left = Buffer::with_capacity(4 * CHUNK_SIZE);
    let mut right = Buffer::new();

    left.inject_test_data(b"abcdef");
    right.inject_test_data(b"abcdef");

    // Keep the logical read position the same on both buffers.
    left.pos = 2;
    right.pos = 2;

    // Write different bytes into spare capacity beyond each buffer's logical `len`.
    left.buf[left.len] = b'x';
    left.buf[left.cap - 1] = b'y';
    right.buf[right.len] = b'z';
    right.buf[right.cap - 1] = b'w';

    // Equality ignores capacity and spare bytes.
    assert_eq!(left, right);

    // Equality still distinguishes different logical read positions.
    right.pos = 3;

    assert_ne!(left, right);
}

#[test]
fn test_buffer_debug() {
    let mut buffer = Buffer::with_capacity(4 * CHUNK_SIZE);
    buffer.inject_test_data(b"abcdef");
    buffer.pos = 2;

    // Write a distinct byte into spare capacity to prove it is not printed.
    buffer.buf[buffer.len] = b'x';

    let debug = format!("{buffer:?}");
    let pretty = format!("{buffer:#?}");

    // Only the logical cursors are exposed; the raw byte storage is omitted,
    // marked as non-exhaustive with a trailing `..`.
    assert_eq!(
        debug,
        format!("Buffer {{ pos: 2, len: 6, cap: {}, .. }}", 4 * CHUNK_SIZE)
    );
    assert!(pretty.starts_with("Buffer {\n"));
    assert!(pretty.contains("pos: 2,"));
    assert!(pretty.contains("len: 6,"));
    assert!(pretty.contains(&format!("cap: {},", 4 * CHUNK_SIZE)));
    assert!(pretty.contains(".."));

    // No raw buffer bytes leak into the output, regardless of format flavor.
    assert!(!debug.contains("abcdef"));
    assert!(!pretty.contains("abcdef"));
}

/* Coverage note: `Default` is a thin wrapper over `new`.
 * Its behavior is covered by the `new` tests.
 */
