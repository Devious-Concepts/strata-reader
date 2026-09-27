//! How does initial capacity affect filling the same 24 KiB input through EOF?
//! Start at 8, 24, or 32 KiB, always capped at 32 KiB. The exact-sized case matters because
//! discovering EOF after a full buffer can require another growth step. This measures the
//! complete fill lifecycle, not reallocation alone: construction, reads, any growth/shrink,
//! and destruction are timed. Input generation is outside timing.

use criterion::{BenchmarkId, Criterion, Throughput};
use std::hint::black_box;
use std::io::Cursor;
use strata_reader::{DynamicRead, Reader, constants::CHUNK_SIZE};

pub(super) fn bench(c: &mut Criterion) {
    let data = vec![b'x'; 3 * CHUNK_SIZE];
    let mut group = c.benchmark_group("growth");
    group.throughput(Throughput::Bytes(u64::try_from(data.len()).unwrap()));
    for initial in [CHUNK_SIZE, 3 * CHUNK_SIZE, 4 * CHUNK_SIZE] {
        let make_reader = || {
            Reader::builder(Cursor::new(black_box(&data)))
                .initial_capacity(initial)
                .max_capacity(4 * CHUNK_SIZE)
                .build()
        };
        let mut reader = make_reader();
        let len = reader.fill_to_end().unwrap();

        assert_eq!(len, data.len());
        assert_eq!(reader.buffer(), data);
        assert_eq!(reader.capacity(), initial.max(data.len()));

        group.bench_function(BenchmarkId::from_parameter(initial), |b| {
            b.iter(|| {
                let mut reader = make_reader();
                black_box(reader.fill_to_end().unwrap());
                black_box(reader.buffer());
            });
        });
    }
    group.finish();
}
