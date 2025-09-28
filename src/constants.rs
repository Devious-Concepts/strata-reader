//! Buffer size constants for dynamic memory allocation.
//!
//! This module defines the core constants used throughout the buffering system to ensure consistent
//! memory allocation behavior. All buffer sizes are managed as multiples of [`CHUNK_SIZE`].
//!
//! # Size Hierarchy
//!
//! The constants establish a three-tier hierarchy:
//! - [`CHUNK_SIZE`]: The fundamental allocation unit (8 KiB)
//! - [`DEFAULT_MAX_SIZE`]: A reasonable default upper bound (256 MiB)
//! - [`PRACTICAL_MAX_SIZE`]: The theoretical platform maximum
//!
//! # Invariant
//!
//! The following mathematical relationships must hold between the constants:
//!
//! - `CHUNK_SIZE` is a power of 2 and a multiple of 1 KiB (1024 bytes)
//! - `DEFAULT_MAX_SIZE > CHUNK_SIZE` and is a power of 2 multiple of `CHUNK_SIZE`
//! - `PRACTICAL_MAX_SIZE > DEFAULT_MAX_SIZE` and is a power of 2 multiple of `CHUNK_SIZE`
//!
//! This invariant ensure that all buffer allocations align properly and that the size hierarchy
//! remains consistent across the system.

/// Buffer chunk size (8 KiB) used for dynamic buffer allocation.
///
/// The size matches [`std::io::BufReader`]'s internal buffer size, deferring to the standard
/// library authors' knowledge of optimal I/O performance.
///
/// The buffer capacity will always be a multiple of this value and it's therefore also the minimum
/// possible size (1 * `CHUNK_SIZE`)
pub const CHUNK_SIZE: usize =
    // 2^13 = 8192 = 8 * 1024 = 8 KiB
    1 << 13;

/// Default maximum buffer size (256 MiB) when no limit is specified.
///
/// This provides a reasonable upper bound for most use cases while preventing runaway memory usage.
/// It is also always a power of two multiple (larger than 1) of [`CHUNK_SIZE`].
pub const DEFAULT_MAX_SIZE: usize =
    // CHUNK_SIZE = 2^13 = 8192 = 8 * 1024 = 8 KiB
    // 2^15 = 32768 = 32 * 1024 = 32 KiB
    // CHUNK_SIZE * 2^15 = 268435456 = 256 * 1024 * 1024 = 256 MiB
    CHUNK_SIZE * (1 << 15);

/// Practical maximum buffer size.
///
/// This is a hardware/platform limit, not a recommended size. It's used to have an upper bound
/// against unreasonable user input. Its value is the largest power of two multiple of
/// [`CHUNK_SIZE`] that fits in usize, representing the theoretical maximum reachable through
/// exponential growth.
///
/// # Safety
///
/// While this is a massive number, it won't overflow as it resolves to `usize::MAX / 2 + 1` just
/// expressed in terms of `CHUNK_SIZE`.
pub const PRACTICAL_MAX_SIZE: usize = CHUNK_SIZE * (1 << CHUNK_SIZE.leading_zeros());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[expect(
        clippy::assertions_on_constants,
        reason = "Asserting an invariant on the constants"
    )]
    fn test_invariant() {
        // CHUNK_SIZE is a multiple of 1 KiB
        assert_eq!(CHUNK_SIZE % 1024, 0);

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
}
