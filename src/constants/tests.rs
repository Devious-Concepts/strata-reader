//! Tests for the constants

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

    // CHUNK_SIZE is a power of two
    assert_eq!(CHUNK_SIZE & (CHUNK_SIZE - 1), 0);

    // DEFAULT_MAX_CAPACITY is larger than CHUNK_SIZE
    assert!(DEFAULT_MAX_CAPACITY > CHUNK_SIZE);

    // DEFAULT_MAX_CAPACITY is a multiple of CHUNK_SIZE
    assert_eq!(DEFAULT_MAX_CAPACITY % CHUNK_SIZE, 0);

    // DEFAULT_MAX_CAPACITY is a power of two (transitive)
    assert_eq!(DEFAULT_MAX_CAPACITY & (DEFAULT_MAX_CAPACITY - 1), 0);

    // DEFAULT_MAX_CAPACITY is a power of two multiple of CHUNK_SIZE
    let k = DEFAULT_MAX_CAPACITY / CHUNK_SIZE;
    assert_eq!(k & (k - 1), 0);

    // MAX_SUPPORTED_CAPACITY is larger than DEFAULT_MAX_CAPACITY
    assert!(MAX_SUPPORTED_CAPACITY > DEFAULT_MAX_CAPACITY);

    // MAX_SUPPORTED_CAPACITY is a multiple of CHUNK_SIZE
    assert_eq!(MAX_SUPPORTED_CAPACITY % CHUNK_SIZE, 0);

    let signed_index_ceiling = usize::MAX >> 1;

    // MAX_SUPPORTED_CAPACITY does not exceed what Vec<u8> can index safely
    assert!(MAX_SUPPORTED_CAPACITY <= signed_index_ceiling);

    // MAX_SUPPORTED_CAPACITY is exactly the aligned allocator ceiling
    let expected = (signed_index_ceiling / CHUNK_SIZE) * CHUNK_SIZE;
    assert_eq!(MAX_SUPPORTED_CAPACITY, expected);

    // MAX_EXPONENTIAL_CAPACITY is the last exact exponential growth step below the ceiling
    assert!(MAX_EXPONENTIAL_CAPACITY <= MAX_SUPPORTED_CAPACITY);
    assert_eq!(MAX_EXPONENTIAL_CAPACITY % CHUNK_SIZE, 0);
    let k = MAX_EXPONENTIAL_CAPACITY / CHUNK_SIZE;
    assert_eq!(k & (k - 1), 0);
    assert!(MAX_EXPONENTIAL_CAPACITY < MAX_SUPPORTED_CAPACITY);

    // The next aligned step would exceed the allocator ceiling
    assert!(MAX_SUPPORTED_CAPACITY > signed_index_ceiling - CHUNK_SIZE);
}
