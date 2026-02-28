//! Tests for the constants
//!
//! This test validates the mathematical invariants between the buffer size constants.

use super::*;

#[test]
#[expect(
    clippy::assertions_on_constants,
    reason = "Asserting an invariant on the constants"
)]
fn test_invariant() {
    // CHUNK_SIZE is a multiple of 1 KiB
    assert_eq!(CHUNK_SIZE % 1024, 0);

    // CHUNK_SIZE is at least 8 KiB for optimal I/O performance
    assert!(CHUNK_SIZE >= 8 * 1024);

    // CHUNK_SIZE is a power of two.
    assert_eq!(CHUNK_SIZE & (CHUNK_SIZE - 1), 0);

    // DEFAULT_MAX_SIZE is larger than CHUNK_SIZE
    assert!(DEFAULT_MAX_SIZE > CHUNK_SIZE);

    // DEFAULT_MAX_SIZE is a multiple of CHUNK_SIZE
    assert_eq!(DEFAULT_MAX_SIZE % CHUNK_SIZE, 0);

    // DEFAULT_MAX_SIZE is a power of two (transitive)
    assert_eq!(DEFAULT_MAX_SIZE & (DEFAULT_MAX_SIZE - 1), 0);

    // DEFAULT_MAX_SIZE is a power of two multiple of CHUNK_SIZE
    let k = DEFAULT_MAX_SIZE / CHUNK_SIZE;
    assert_eq!(k & (k - 1), 0);

    // PRACTICAL_MAX_SIZE is larger than DEFAULT_MAX_SIZE
    assert!(PRACTICAL_MAX_SIZE > DEFAULT_MAX_SIZE);

    // PRACTICAL_MAX_SIZE is a multiple of CHUNK_SIZE
    assert_eq!(PRACTICAL_MAX_SIZE % CHUNK_SIZE, 0);

    // PRACTICAL_MAX_SIZE is a power of two (transitive)
    assert_eq!(PRACTICAL_MAX_SIZE & (PRACTICAL_MAX_SIZE - 1), 0);

    // PRACTICAL_MAX_SIZE is a power of two multiple of CHUNK_SIZE
    let k = PRACTICAL_MAX_SIZE / CHUNK_SIZE;
    assert_eq!(k & (k - 1), 0);
}
