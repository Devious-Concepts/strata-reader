//! What does it cost to discard a consumed prefix versus keep an owned copy of it?
//! A 64 KiB reader has either 8 KiB or 56 KiB consumed. Compact and compact+shrink discard that
//! prefix. The owned pair keeps identical prefix bytes: `take_consumed` returns a Buffer; copying
//! the prefix to a Vec then compacting/shrinking returns a Vec. Both keep the unread suffix.
//! Setup/filling and final destruction of inputs/outputs are excluded with `iter_batched_ref`.
//! Each operation's own allocation, copying, and shrinking are timed. Batches hold 16 fixtures.

use criterion::{BatchSize, BenchmarkId, Criterion};
use std::hint::black_box;
use std::io::{BufRead, Cursor};
use strata_reader::{DynamicRead, Reader, constants::CHUNK_SIZE};

fn retained_reader(data: &[u8], consumed: usize) -> Reader<Cursor<&[u8]>> {
    let mut reader = Reader::new(Cursor::new(data));
    reader.fill_exact(data.len()).unwrap();
    reader.consume(consumed);
    reader
}

fn copy_prefix(reader: &mut Reader<Cursor<&[u8]>>) -> Vec<u8> {
    let prefix = reader.peek_behind(reader.pos()).to_vec();
    reader.compact();
    reader.shrink();
    prefix
}

pub(super) fn bench(c: &mut Criterion) {
    let data: Vec<_> = (0u8..=255).cycle().take(8 * CHUNK_SIZE).collect();
    let mut group = c.benchmark_group("retention");
    for consumed in [CHUNK_SIZE, 7 * CHUNK_SIZE] {
        // Both discard variants preserve the suffix; only one releases its spare capacity
        let mut compacted = retained_reader(&data, consumed);
        compacted.compact();
        assert_eq!(compacted.buffer(), &data[consumed..]);
        assert_eq!(compacted.capacity(), data.len());
        compacted.shrink();
        assert_eq!(compacted.buffer(), &data[consumed..]);
        assert_eq!(compacted.capacity(), data.len() - consumed);

        // The owned variants preserve both byte sequences, despite different return types
        let mut transferred = retained_reader(&data, consumed);
        let prefix = transferred.take_consumed();
        assert_eq!(prefix.buf(), &data[..consumed]);
        assert_eq!(prefix.pos(), consumed);
        assert_eq!(transferred.buffer(), &data[consumed..]);
        assert_eq!(transferred.capacity(), data.len() - consumed);
        let mut copied = retained_reader(&data, consumed);
        let prefix = copy_prefix(&mut copied);
        assert_eq!(prefix, &data[..consumed]);
        assert_eq!(copied.buffer(), &data[consumed..]);
        assert_eq!(copied.capacity(), transferred.capacity());

        group.bench_function(BenchmarkId::new("compact", consumed), |b| {
            b.iter_batched_ref(
                || retained_reader(&data, consumed),
                |reader| {
                    reader.compact();
                    black_box(reader.buffer());
                },
                BatchSize::NumIterations(16),
            );
        });
        group.bench_function(BenchmarkId::new("compact_and_shrink", consumed), |b| {
            b.iter_batched_ref(
                || retained_reader(&data, consumed),
                |reader| {
                    reader.compact();
                    reader.shrink();
                    black_box(reader.buffer());
                },
                BatchSize::NumIterations(16),
            );
        });
        group.bench_function(BenchmarkId::new("take_consumed", consumed), |b| {
            b.iter_batched_ref(
                || retained_reader(&data, consumed),
                |reader| black_box(reader.take_consumed()),
                BatchSize::NumIterations(16),
            );
        });
        group.bench_function(BenchmarkId::new("copy_prefix", consumed), |b| {
            b.iter_batched_ref(
                || retained_reader(&data, consumed),
                |reader| black_box(copy_prefix(reader)),
                BatchSize::NumIterations(16),
            );
        });
    }
    group.finish();
}
