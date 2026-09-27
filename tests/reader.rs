//! Public-API scenarios for reading records, keeping lookbehind, recovering from stops, and
//! handing buffered input to another consumer. The module tests cover individual methods;
//! these tests follow several operations on the same input without private buffer access.

#![expect(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    reason = "Okay in tests"
)]

#[path = "reader/buffer.rs"]
mod buffer;
#[path = "reader/io.rs"]
mod io;
#[path = "reader/records.rs"]
mod records;
#[path = "reader/recovery.rs"]
mod recovery;
#[path = "reader/retention.rs"]
mod retention;
