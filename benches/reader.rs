//! Compare reader choices for sequential input, records, retention, and growth.
//! Each module states its question and timing boundary, then checks outputs before measuring.
//! Run `cargo bench --bench reader`; use `-- --test` to run the correctness checks without timing.

#![expect(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    reason = "Bounded benchmark fixtures; I/O failures must stop the benchmark"
)]

use criterion::{criterion_group, criterion_main};

#[path = "reader/growth.rs"]
mod growth;
#[path = "reader/records.rs"]
mod records;
#[path = "reader/retention.rs"]
mod retention;
#[path = "reader/sequential.rs"]
mod sequential;

criterion_group!(
    benches,
    sequential::bench,
    records::bench,
    retention::bench,
    growth::bench
);
criterion_main!(benches);
