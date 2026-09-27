//! In-memory I/O, delimiter searches, and retained-buffer operations.

#![expect(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    reason = "Bounded benchmark fixtures; I/O failures must stop the benchmark"
)]

use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;
use std::io::{self, BufRead, BufReader, Cursor, Read};
use strata_reader::{DynamicRead, Reader, constants::CHUNK_SIZE};

fn read_chunks(mut reader: impl Read, mut visit: impl FnMut(&[u8])) -> io::Result<usize> {
    let mut output = [0; 1024];
    let mut total = 0;
    loop {
        let count = reader.read(&mut output)?;
        if count == 0 {
            return Ok(total);
        }
        visit(&output[..count]);
        total += count;
    }
}

fn sequential(c: &mut Criterion) {
    let mut group = c.benchmark_group("sequential");
    for size in [1024, CHUNK_SIZE, 1024 * 1024] {
        let data: Vec<_> = (0u8..=255).cycle().take(size).collect();
        // Check exact output outside timing, using the same read loop as both measurements.
        let mut actual = Vec::new();
        assert_eq!(
            read_chunks(Reader::new(Cursor::new(&data)), |bytes| actual
                .extend_from_slice(bytes))
            .unwrap(),
            size
        );
        assert_eq!(actual, data);
        actual.clear();
        assert_eq!(
            read_chunks(
                BufReader::with_capacity(CHUNK_SIZE, Cursor::new(&data)),
                |bytes| actual.extend_from_slice(bytes)
            )
            .unwrap(),
            size
        );
        assert_eq!(actual, data);

        group.throughput(Throughput::Bytes(u64::try_from(size).unwrap()));
        group.bench_with_input(BenchmarkId::new("reader", size), &data, |b, data| {
            b.iter(|| {
                read_chunks(Reader::new(Cursor::new(black_box(data))), |bytes| {
                    black_box(bytes);
                })
                .unwrap()
            });
        });
        group.bench_with_input(BenchmarkId::new("bufreader", size), &data, |b, data| {
            b.iter(|| {
                read_chunks(
                    BufReader::with_capacity(CHUNK_SIZE, Cursor::new(black_box(data))),
                    |bytes| {
                        black_box(bytes);
                    },
                )
                .unwrap()
            });
        });
    }
    group.finish();
}

fn retained_records(data: &[u8], mut visit: impl FnMut(&[u8])) -> io::Result<usize> {
    let mut reader = Reader::builder(Cursor::new(data))
        .max_capacity(4 * CHUNK_SIZE)
        .build();
    let mut records = 0;
    loop {
        reader.fill_until(b'\n')?;
        let available = reader.peek(usize::MAX);
        if available.is_empty() {
            return Ok(records);
        }
        let newline = available
            .iter()
            .position(|&byte| byte == b'\n')
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "incomplete benchmark record")
            })?;
        let len = newline + 1;
        reader.consume(len);
        visit(reader.peek_behind(len));
        reader.compact();
        records += 1;
    }
}

fn copied_records(data: &[u8], mut visit: impl FnMut(&[u8])) -> io::Result<usize> {
    let mut reader = BufReader::with_capacity(CHUNK_SIZE, Cursor::new(data));
    let mut record = Vec::new();
    let mut records = 0;
    while reader.read_until(b'\n', &mut record)? != 0 {
        visit(&record);
        records += 1;
        record.clear();
    }
    Ok(records)
}

fn records(c: &mut Criterion) {
    let mut group = c.benchmark_group("records");
    // Record sizes include the newline: below, at, and above the initial buffer boundary.
    for size in [64, CHUNK_SIZE, CHUNK_SIZE + 1] {
        let mut record = vec![b'x'; size - 1];
        record.push(b'\n');
        let data = record.repeat(64);
        let mut actual = Vec::new();
        assert_eq!(
            retained_records(&data, |bytes| {
                assert_eq!(bytes, record);
                actual.extend_from_slice(bytes);
            })
            .unwrap(),
            64
        );
        assert_eq!(actual, data);
        actual.clear();
        assert_eq!(
            copied_records(&data, |bytes| {
                assert_eq!(bytes, record);
                actual.extend_from_slice(bytes);
            })
            .unwrap(),
            64
        );
        assert_eq!(actual, data);

        group.throughput(Throughput::Bytes(u64::try_from(data.len()).unwrap()));
        group.bench_with_input(
            BenchmarkId::new("reader_borrowed", size),
            &data,
            |b, data| {
                b.iter(|| {
                    retained_records(black_box(data), |record| {
                        black_box(record);
                    })
                    .unwrap()
                });
            },
        );
        group.bench_with_input(
            BenchmarkId::new("bufreader_copied", size),
            &data,
            |b, data| {
                b.iter(|| {
                    copied_records(black_box(data), |record| {
                        black_box(record);
                    })
                    .unwrap()
                });
            },
        );
    }
    group.finish();
}

fn retained_reader(data: &[u8], consumed: usize) -> Reader<Cursor<&[u8]>> {
    let mut reader = Reader::new(Cursor::new(data));
    reader.fill_exact(data.len()).unwrap();
    reader.consume(consumed);
    reader
}

fn retention(c: &mut Criterion) {
    let data: Vec<_> = (0u8..=255).cycle().take(8 * CHUNK_SIZE).collect();
    let mut group = c.benchmark_group("retention");
    for consumed in [CHUNK_SIZE, 7 * CHUNK_SIZE] {
        let mut compacted = retained_reader(&data, consumed);
        compacted.compact();
        assert_eq!(compacted.buffer(), &data[consumed..]);
        assert_eq!(compacted.capacity(), data.len());
        compacted.shrink();
        assert_eq!(compacted.buffer(), &data[consumed..]);
        assert_eq!(compacted.capacity(), data.len() - consumed);
        let mut transferred = retained_reader(&data, consumed);
        let prefix = transferred.take_consumed();
        assert_eq!(prefix.buf(), &data[..consumed]);
        assert_eq!(prefix.pos(), consumed);
        assert_eq!(transferred.buffer(), &data[consumed..]);

        // Filling/consuming the fixture is setup, outside the measured operation.
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
        group.bench_function(BenchmarkId::new("take_consumed", consumed), |b| {
            b.iter_batched_ref(
                || retained_reader(&data, consumed),
                |reader| black_box(reader.take_consumed()),
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
    }
    group.finish();
}

fn growth(c: &mut Criterion) {
    let data = vec![b'x'; 3 * CHUNK_SIZE];
    let mut group = c.benchmark_group("growth");
    group.throughput(Throughput::Bytes(u64::try_from(data.len()).unwrap()));
    for initial in [CHUNK_SIZE, 4 * CHUNK_SIZE] {
        let make_reader = || {
            Reader::builder(Cursor::new(black_box(&data)))
                .initial_capacity(initial)
                .max_capacity(4 * CHUNK_SIZE)
                .build()
        };
        let mut reader = make_reader();
        assert_eq!(reader.fill_to_end().unwrap(), data.len());
        assert_eq!(reader.buffer(), data);
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

criterion_group!(benches, sequential, records, retention, growth);
criterion_main!(benches);
