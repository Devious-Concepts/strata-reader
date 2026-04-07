//! Buffer size constants for dynamic memory allocation.
//!
//! All buffer sizes are managed as multiples of [`CHUNK_SIZE`].
//!
//! # Invariant
//!
//! The following mathematical relationships must hold between the constants:
//!
//! - [`CHUNK_SIZE`] is a power of 2 and at least 8 KiB
//! - [`DEFAULT_MAX_SIZE`] and [`THEORETICAL_MAX_SIZE`] are both power-of-two multiples of
//!   `CHUNK_SIZE`
//! - `CHUNK_SIZE < DEFAULT_MAX_SIZE < THEORETICAL_MAX_SIZE`

/// Buffer chunk size. (8 KiB)
///
/// The size matches [`std::io::BufReader`]'s internal buffer size, deferring to the standard
/// library authors' knowledge of optimal I/O performance.
///
/// Buffer capacity is always a multiple of this value, making it also the minimum possible size.
pub const CHUNK_SIZE: usize = 1 << 13; // 2^13 = 8 KiB

/// Default maximum buffer size (256 MiB) when no limit is specified.
///
/// Provides a reasonable upper bound for most use cases while preventing runaway memory usage.
pub const DEFAULT_MAX_SIZE: usize = CHUNK_SIZE * (1 << 15); // CHUNK_SIZE * 2^15 = 256 MiB

/// Theoretical maximum buffer size.
///
/// This is a hardware/platform limit. Used to have an upper bound against unreasonable user input.
/// Its value is the largest power-of-two multiple of [`CHUNK_SIZE`] that fits in usize,
/// representing the theoretical maximum reachable through exponential growth.
///
/// Equals `usize::MAX / 2 + 1` just expressed in terms of `CHUNK_SIZE`.
pub const THEORETICAL_MAX_SIZE: usize = CHUNK_SIZE * (1 << CHUNK_SIZE.leading_zeros());

#[cfg(test)]
mod tests;
