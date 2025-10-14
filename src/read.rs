use std::io::{self, Read};

/// A `DynamicRead` is an alternative to `BufRead` with a buffer that can grow and shrink in size as
/// needed. This allows for peeking far into this source without by growing the buffer as needed.
/// Shrinking requires a manual call to [`compact()`].
///
/// TODO: More docs
///
/// [`compact()`]: DynamicRead::compact
pub trait DynamicRead: Read {}
