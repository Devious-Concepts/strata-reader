//! Buffer size constants for dynamic memory allocation.
//!
//! All buffer sizes are managed as multiples of [`CHUNK_SIZE`].

/// Buffer chunk size. (8 KiB)
///
/// The size matches [`std::io::BufReader`]'s internal buffer size, deferring to the standard
/// library authors' knowledge of optimal I/O performance.
///
/// Buffer capacity is always a multiple of this value, making it also the minimum possible size.
pub const CHUNK_SIZE: usize = 1 << 13; // 2^13 = 8 KiB

/// Default maximum buffer capacity (256 MiB) when no limit is specified.
///
/// Provides a reasonable upper bound for most use cases while preventing runaway memory usage.
pub const DEFAULT_MAX_CAPACITY: usize = CHUNK_SIZE * (1 << 15); // CHUNK_SIZE * 2^15 = 256 MiB

/// Largest aligned capacity supported by the implementation.
///
/// This is an internal technical ceiling derived from the backing `Vec<u8>` allocation model,
/// rounded down from the platform's signed-index ceiling to a [`CHUNK_SIZE`] multiple.
///
/// It is not a sane operating target or public tuning value.
pub(crate) const MAX_SUPPORTED_CAPACITY: usize = ((usize::MAX >> 1) / CHUNK_SIZE) * CHUNK_SIZE;

/// Largest power-of-two multiple of [`CHUNK_SIZE`] below the allocator ceiling.
///
/// Used internally to determine when exponential growth must saturate to
/// [`MAX_SUPPORTED_CAPACITY`] instead of rounding to the next power-of-two capacity.
pub(crate) const MAX_EXPONENTIAL_CAPACITY: usize =
    ((MAX_SUPPORTED_CAPACITY / CHUNK_SIZE).next_power_of_two() >> 1) * CHUNK_SIZE;

#[cfg(test)]
mod tests;
