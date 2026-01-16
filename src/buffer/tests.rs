//! Tests for the Buffer
//!
//! These tests are in the same narrative order as the main file, and are designed to not depend on
//! things that we've yet to have written tests for, at least narratively. While the test runner is
//! asynchronous and this does not matter there, when reading the file, it makes the flow much
//! easier to understand since we don't introduce future concepts if we can avoid them.

#![expect(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    reason = "Okay in tests"
)]

use super::*;
use crate::constants::{CHUNK_SIZE, PRACTICAL_MAX_SIZE};
use std::io::{self, Cursor};

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

/* Note: The `impl Default` is a wrapper over the `new` constructor
 * Thus, I've deferred testing that to the tests for `new`
 * There's no need to test the same method twice after all
 */

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

/* Note: `with_capacity` calls `cap_up_linear` internally to round to multiples of `CHUNK_SIZE`
 * Thus, I've deferred testing that to the tests for `cap_up_linear`
 * There's no need to test the same method twice after all
 */

// -----------------------------------------------------------------------------
// Buffer - Accessors and is_empty.
// -----------------------------------------------------------------------------

#[test]
fn test_buffer_buf() {
    let mut buffer = Buffer::new();

    // Starts empty
    assert_eq!(buffer.buf(), &[]);

    // Manually push data to internal vec for testing
    let data = "Hello, World!";
    buffer.buf[..data.len()].copy_from_slice(data.as_bytes());

    // Update len to make data visible
    buffer.len = data.len();

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

    // Update len to pretend we have data
    buffer.len = 123;

    // Accessor should reflect this
    assert_eq!(buffer.len(), 123);
}

#[test]
fn test_buffer_is_empty() {
    let mut buffer = Buffer::new();

    // Starts empty
    assert!(buffer.is_empty());

    // Update len to pretend we have data
    buffer.len = 123;

    // No longer empty
    assert!(!buffer.is_empty());
}

#[test]
fn test_buffer_pos() {
    let mut buffer = Buffer::new();

    // Starts at 0
    assert_eq!(buffer.pos(), 0);

    // Update len and pos to pretend we have data
    // We also update len to maintain the invariant
    // Testing invalid state is useless after all
    buffer.len = 123;
    buffer.pos = 123;

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

    // It should still be empty.
    assert!(buffer.is_empty());
    assert_eq!(buffer.len(), 0);
    assert_eq!(buffer.pos(), 0);

    // Calling it again still shouldn't change anything
    buffer.clear();

    // It should still be empty.
    assert!(buffer.is_empty());
    assert_eq!(buffer.len(), 0);
    assert_eq!(buffer.pos(), 0);

    // Update len and pos to pretend we have data
    buffer.len = 456; // More data than is consumed
    buffer.pos = 123;

    // Clearing the buffer should make it empty.
    buffer.clear();

    // It should now be empty.
    assert!(buffer.is_empty());
    assert_eq!(buffer.len(), 0);
    assert_eq!(buffer.pos(), 0);
}

/* Note: There are no tests for `discard` as it's just a wrapper over `clear` and `shrink`
 * So testing is deferred to the tests for `clear` and `shrink_targeted`
 * Since `shrink` is also a wrapper over over `shrink_targeted`
 * There's no need to test those methods twice after all
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

    // Update len to pretend we have data
    buffer.len = 4567; // around half a CHUNK_SIZE, but not exactly

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

    // It should match the len we set.
    assert_eq!(buffer.pos(), 4567);
    assert_eq!(buffer.pos(), buffer.len()); // to show len didn't change

    // Consuming when all is already consumed should not change anything.
    buffer.consume(123);

    // It should still match the len we set.
    assert_eq!(buffer.pos(), 4567);
    assert_eq!(buffer.pos(), buffer.len()); // to show len still didn't change
}

#[test]
fn test_buffer_compact() {
    let mut buffer = Buffer::with_capacity(8 * CHUNK_SIZE); // Power-of-2 multiple

    // Calling it on an empty buffer shouldn't change anything
    buffer.compact();

    // It should still be empty.
    assert!(buffer.is_empty());
    assert_eq!(buffer.len(), 0);
    assert_eq!(buffer.pos(), 0);

    // Calling it again still shouldn't change anything
    buffer.compact();

    // It should still be empty.
    assert!(buffer.is_empty());
    assert_eq!(buffer.len(), 0);
    assert_eq!(buffer.pos(), 0);

    // Manually push data to internal vec for testing
    let raw = "Hello, World!".repeat(36); // 13 × 36 > 456
    let data = str::from_utf8(&raw.as_bytes()[..456]).unwrap(); // Get a 456 byte string
    buffer.buf[..data.len()].copy_from_slice(data.as_bytes());

    // Why 456? Because I said so…

    // Update len to pretend we have data and consume some
    buffer.len = 456; // More data than is consumed.
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

    // It should now be empty.
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
        Buffer::cap_down(PRACTICAL_MAX_SIZE - 1),
        PRACTICAL_MAX_SIZE - CHUNK_SIZE // Note: `PRACTICAL_MAX_SIZE` is a multiple of `CHUNK_SIZE`
    );

    // Sizes above `PRACTICAL_MAX_SIZE` should round down to `PRACTICAL_MAX_SIZE` as that's the max
    assert_eq!(Buffer::cap_down(usize::MAX), PRACTICAL_MAX_SIZE);
    assert_eq!(
        Buffer::cap_down(PRACTICAL_MAX_SIZE + 1234),
        PRACTICAL_MAX_SIZE
    );
    assert_eq!(Buffer::cap_down(PRACTICAL_MAX_SIZE + 1), PRACTICAL_MAX_SIZE);
}

#[test]
fn test_buffer_cap_up() {
    // Sizes above `PRACTICAL_MAX_SIZE` should round down to `PRACTICAL_MAX_SIZE` as that's the max
    assert_eq!(Buffer::cap_up(usize::MAX), PRACTICAL_MAX_SIZE);
    assert_eq!(
        Buffer::cap_up(PRACTICAL_MAX_SIZE + 1234),
        PRACTICAL_MAX_SIZE
    );
    assert_eq!(Buffer::cap_up(PRACTICAL_MAX_SIZE + 1), PRACTICAL_MAX_SIZE);

    // Other values should round up to the nearest power-of-2 multiple of `CHUNK_SIZE`
    assert_eq!(Buffer::cap_up(CHUNK_SIZE - 123), CHUNK_SIZE);
    assert_eq!(Buffer::cap_up(2 * CHUNK_SIZE - 123), 2 * CHUNK_SIZE);
    assert_eq!(Buffer::cap_up(5 * CHUNK_SIZE - 456), 8 * CHUNK_SIZE);
    assert_eq!(Buffer::cap_up(20 * CHUNK_SIZE - 789), 32 * CHUNK_SIZE);
    assert_eq!(Buffer::cap_up(123 * CHUNK_SIZE - 1234), 128 * CHUNK_SIZE);
    assert_eq!(
        Buffer::cap_up(PRACTICAL_MAX_SIZE - 1),
        PRACTICAL_MAX_SIZE // Note: `PRACTICAL_MAX_SIZE` is a multiple of `CHUNK_SIZE`
    );

    // Sizes below `CHUNK_SIZE` should round up to `CHUNK_SIZE` as that's the min
    assert_eq!(Buffer::cap_up(usize::MIN), CHUNK_SIZE);
    assert_eq!(Buffer::cap_up(CHUNK_SIZE - 1), CHUNK_SIZE);
}

#[test]
fn test_buffer_cap_up_linear() {
    // Sizes above `PRACTICAL_MAX_SIZE` should round down to `PRACTICAL_MAX_SIZE` as that's the max
    assert_eq!(Buffer::cap_up_linear(usize::MAX), PRACTICAL_MAX_SIZE);
    assert_eq!(
        Buffer::cap_up_linear(PRACTICAL_MAX_SIZE + 1234),
        PRACTICAL_MAX_SIZE
    );
    assert_eq!(
        Buffer::cap_up_linear(PRACTICAL_MAX_SIZE + 1),
        PRACTICAL_MAX_SIZE
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
        Buffer::cap_up_linear(PRACTICAL_MAX_SIZE - 1),
        PRACTICAL_MAX_SIZE // Note: `PRACTICAL_MAX_SIZE` is a multiple of `CHUNK_SIZE`
    );

    // Sizes below `CHUNK_SIZE` should round up to `CHUNK_SIZE` as that's the min
    assert_eq!(Buffer::cap_up_linear(usize::MIN), CHUNK_SIZE);
    assert_eq!(Buffer::cap_up_linear(CHUNK_SIZE - 1), CHUNK_SIZE);
}

// -----------------------------------------------------------------------------
// Buffer - Capacity control
// -----------------------------------------------------------------------------

#[test]
fn test_buffer_grow() {
    let mut buffer = Buffer::with_capacity(5 * CHUNK_SIZE); // Not power-of-2 multiple

    // When not aligned to a power-of-2 multiples of `CHUNK_SIZE`, we only grows so we're aligned
    buffer.grow();
    assert_eq!(buffer.cap(), 8 * CHUNK_SIZE);
    assert!(buffer.buf.capacity() >= 8 * CHUNK_SIZE); // Also double check the internals
    assert_eq!(buffer.buf.len(), 8 * CHUNK_SIZE);

    // Each grow, when aligned, doubles.
    buffer.grow();
    assert_eq!(buffer.cap(), 16 * CHUNK_SIZE);
    assert!(buffer.buf.capacity() >= 16 * CHUNK_SIZE); // Also double check the internals
    assert_eq!(buffer.buf.len(), 16 * CHUNK_SIZE);

    // Again
    buffer.grow();
    assert_eq!(buffer.cap(), 32 * CHUNK_SIZE);
    assert!(buffer.buf.capacity() >= 32 * CHUNK_SIZE); // Also double check the internals
    assert_eq!(buffer.buf.len(), 32 * CHUNK_SIZE);

    // And again
    buffer.grow();
    assert_eq!(buffer.cap(), 64 * CHUNK_SIZE);
    assert!(buffer.buf.capacity() >= 64 * CHUNK_SIZE); // Also double check the internals
    assert_eq!(buffer.buf.len(), 64 * CHUNK_SIZE);

    // And two more times for good measure
    buffer.grow();
    buffer.grow(); // double grow
    assert_eq!(buffer.cap(), 256 * CHUNK_SIZE);
    assert!(buffer.buf.capacity() >= 256 * CHUNK_SIZE); // Also double check the internals
    assert_eq!(buffer.buf.len(), 256 * CHUNK_SIZE);
}

/* Note: The `shrink` method is just a wrapper over `shrink_targeted`
 * Thus, I've deferred testing to the tests for `shrink_targeted`
 * There's no need to test the same method twice after all
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
    buffer.buf[..(250 * CHUNK_SIZE - 123)].copy_from_slice(data.as_slice());
    buffer.len = 250 * CHUNK_SIZE - 123;

    // Double-check the starting cap after our meddling
    assert_eq!(buffer.cap(), 260 * CHUNK_SIZE);
    assert!(buffer.buf.capacity() >= 260 * CHUNK_SIZE); // Also double check the internals
    assert_eq!(buffer.buf.len(), 260 * CHUNK_SIZE);

    /* Note: We can't check an upper bound of `buffer.buf.capacity()` since it's resized using an
     * "at least" method with no documented ceiling. While usually exact or slightly bigger, there's
     * no way we can guarantee for our tests. The best we can, or should, do is follow the docs
     */

    // In this case we have slightly less than 250 × `CHUNK_SIZE` data so that's becomes our target
    // since we shouldn't be able to truncate our data with a shrink operation
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
// Buffer - Fill
// -----------------------------------------------------------------------------

#[test]
fn test_buffer_fill() {
    let mut buffer = Buffer::new();
    let data = "Hello, World!";
    let rem = CHUNK_SIZE % data.len();

    // Make sure the length of the data is not a multiple of `CHUNK_SIZE` or the test won't work
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
        assert_eq!(read, FillResult::Eof(data.len()));
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

    // Try to fill once more now that we're full.
    let cur = Cursor::new(data);
    let read = buffer.fill(cur).unwrap();

    // We should have not read anything.
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

    // Fill the buffer to more data for the next test
    let mut cur = Cursor::new(&data);
    let read = buffer.fill_amount(&mut cur, 2 * CHUNK_SIZE).unwrap();

    // We should have 4 × `CHUNK_SIZE` bytes of data as there were bytes left in the buffer
    assert_eq!(read, FillResult::Complete(4 * CHUNK_SIZE - raw.len()));
    assert_eq!(buffer.len(), 4 * CHUNK_SIZE);

    // Discard everything for a clean slate
    buffer.discard();

    // Read a big amount of data, specifically 3 × `CHUNK_SIZE` bytes
    let cur = Cursor::new(&data);
    let read = buffer.fill_amount(cur, 3 * CHUNK_SIZE).unwrap();

    // We should have 4 × `CHUNK_SIZE` bytes of data since 4 is the closest power-of-2 after 3
    assert_eq!(read, FillResult::Complete(4 * CHUNK_SIZE));
    assert_eq!(buffer.len(), 4 * CHUNK_SIZE);
    assert_eq!(buffer.buf(), &data.as_bytes()[..buffer.len()]); // Data should match

    // Discard everything for a clean slate
    buffer.discard();

    // Test error when requesting more than the buffer can accommodate
    // The buffer cannot grow beyond `PRACTICAL_MAX_SIZE`, so `amt > PRACTICAL_MAX_SIZE - self.len`
    // is an unfulfillable request
    let cur = Cursor::new(&data);
    let result = buffer.fill_amount(cur, PRACTICAL_MAX_SIZE + 1);

    // We should get an InvalidInput error
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidInput);

    // Even with some data already in the buffer, requesting more than the remaining capacity errors
    let cur = Cursor::new(&data);
    buffer.fill_amount(cur, CHUNK_SIZE).unwrap(); // Add some data first

    let cur = Cursor::new(&data);
    let result = buffer.fill_amount(cur, PRACTICAL_MAX_SIZE); // Now this exceeds remaining capacity

    // We should get an InvalidInput error
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidInput);
}

/* Note: `fill_amount` calls `shrink_targeted` using whatever the starting
 * capacity is; as it's target, so there is no reason to test shrinking again as
 * test for `shrink_targeted` have already been written above.
 */

#[test]
fn test_buffer_fill_exact() {
    let mut buffer = Buffer::new();
    let raw = "Hello, World!";
    let data = raw.repeat(6000); // > 8 × `CHUNK_SIZE`

    // Let's start with an EOF test (requesting more than available)
    let cur = Cursor::new(raw);
    let result = buffer.fill_exact(cur, 100); // Request more than `raw.len()`

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

    // Test error when requesting more than the buffer can accommodate
    // The buffer cannot grow beyond `PRACTICAL_MAX_SIZE`, so `amt > PRACTICAL_MAX_SIZE - self.len`
    // is an unfulfillable request
    let cur = Cursor::new(&data);
    let result = buffer.fill_exact(cur, PRACTICAL_MAX_SIZE + 1);

    // We should get an InvalidInput error
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidInput);

    // Even with some data already in the buffer, requesting more than the remaining capacity errors
    let cur = Cursor::new(&data);
    buffer.fill_exact(cur, CHUNK_SIZE).unwrap(); // Add some data first

    let cur = Cursor::new(&data);
    let result = buffer.fill_exact(cur, PRACTICAL_MAX_SIZE); // Now this exceeds remaining capacity

    // We should get an InvalidInput error
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidInput);
}

/* Note: `fill_exact` calls `shrink_targeted` using whatever the starting
 * capacity is; as it's target, so there is no reason to test shrinking again as
 * tests for `shrink_targeted` have already been written above.
 */

// -----------------------------------------------------------------------------
// Buffer - UTF-8 Helpers
// -----------------------------------------------------------------------------

/* Note: For the tests in this section we're creating a buffer with multibyte
 * data by using "世界" which is "World" in Japanese in order to test char
 * alignment.
 *
 * Byte layout:
 *   "Hello, " = 7 bytes (0-6),
 *   '世' is 3 bytes (7-9),
 *   '界' is 3 bytes (10-12),
 *   '!' is 1 byte (13)
 */

#[test]
fn test_buffer_align_pos_to_char() {
    let mut buffer = Buffer::new();
    let data = "Hello, 世界!";
    let cur = Cursor::new(data);
    buffer.fill(cur).unwrap();

    // Let's start with an offset that is already on a char boundary.
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

    // For an edge-case test, let's use some random non-UTF-8 bytes.
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
}

#[test]
fn test_buffer_align_pos_to_next_char() {
    let mut buffer = Buffer::new();
    let data = "Hello, 世界!";
    let cur = Cursor::new(data);
    buffer.fill(cur).unwrap();

    // Let's start with an offset that is already on a char boundary.
    let aligned = buffer.align_pos_to_next_char(5);
    assert_eq!(aligned, 5); // Start of ','

    // Next let's try one byte into '世', this should return the start offset of the next char
    let aligned = buffer.align_pos_to_next_char(8);
    assert_eq!(aligned, 10); // Aligns back to start of '界'

    // And again with the second byte into '世'
    let aligned = buffer.align_pos_to_next_char(9);
    assert_eq!(aligned, 10); // Still aligns back to start of '界'

    // The offset should clamp to len as it's max
    let aligned = buffer.align_pos_to_next_char(100);
    assert_eq!(aligned, buffer.len()); // Clamped to len

    // The offset should clamp to pos as it's min
    buffer.consume(10); // Now pos = 10 (start of '界')
    let aligned = buffer.align_pos_to_next_char(2);
    assert_eq!(aligned, 10); // Clamped to pos

    // Discard everything for a clean slate
    buffer.discard();

    // For an edge-case test, let's use some random non-UTF-8 bytes.
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

/* Note: There are no tests for `as_str` as it's just a wrapper
 * over `as_str_from` with `self.pos()` as it's `pos`.
 * So testing is deferred to the tests for `as_str_from`.
 * There's no need to test the same method twice after all
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
    buffer.buf[0..5].copy_from_slice(b"Hello");
    buffer.buf[5] = 0b1000_0000; // Invalid continuation byte without start
    buffer.buf[6..12].copy_from_slice(b"World!");
    buffer.len = 12;

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

/* Note: Testing various UTF-8 byte lengths (2-byte, 3-byte, 4-byte) is unnecessary here
 * as `as_str_from` delegates character boundary alignment to `align_pos_to_next_char`,
 * which has already been tested with arbitrary binary data matching the continuation
 * byte pattern. The method only adds UTF-8 validation on top of that alignment.
 */
