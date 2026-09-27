//! How much is the choice of buffered reader, and how much is the record-processing recipe?
//! The copied pair uses the same `BufRead::read_until` loop and reused Vec for both readers.
//! Two borrowed Reader recipes instead expose retained slices: compact after every record, or
//! only when the next delimiter is missing. All process the same 64 LF-terminated records.
//! Construction, searches, copying/compaction, EOF, and destruction are timed; input generation
//! and correctness assertions are not. Borrowed and copied recipes have different ownership.

use criterion::{BenchmarkId, Criterion, Throughput};
use std::hint::black_box;
use std::io::{self, BufRead, BufReader, Cursor};
use strata_reader::{DynamicRead, Reader, constants::CHUNK_SIZE};

fn copied_records(mut reader: impl BufRead, mut visit: impl FnMut(&[u8])) -> io::Result<usize> {
    let mut record = Vec::new();
    let mut records = 0;
    while reader.read_until(b'\n', &mut record)? != 0 {
        visit(&record);
        records += 1;
        record.clear();
    }
    Ok(records)
}

fn retained_records(
    data: &[u8],
    compact_each: bool,
    mut visit: impl FnMut(&[u8]),
) -> io::Result<usize> {
    let mut reader = Reader::builder(Cursor::new(data))
        .max_capacity(4 * CHUNK_SIZE)
        .build();
    let mut records = 0;
    loop {
        // A complete buffered record needs neither another fill nor compaction
        if !reader.peek(usize::MAX).contains(&b'\n') {
            reader.compact();
            reader.fill_until(b'\n')?;
        }
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
        if compact_each {
            reader.compact();
        }
        records += 1;
    }
}

pub(super) fn bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("records");
    // Sizes include LF: many records per buffer, exactly one, and a record requiring growth
    for size in [64, CHUNK_SIZE, CHUNK_SIZE + 1] {
        let mut record = vec![b'x'; size - 1];
        record.push(b'\n');
        let data = record.repeat(64);

        // Check every record's boundaries and contents, not just total bytes processed
        let mut actual = Vec::new();
        let count = copied_records(Reader::new(Cursor::new(&data)), |bytes| {
            assert_eq!(bytes, record);
            actual.extend_from_slice(bytes);
        })
        .unwrap();
        assert_eq!(count, 64);
        assert_eq!(actual, data);
        actual.clear();
        let count = copied_records(
            BufReader::with_capacity(CHUNK_SIZE, Cursor::new(&data)),
            |bytes| {
                assert_eq!(bytes, record);
                actual.extend_from_slice(bytes);
            },
        )
        .unwrap();
        assert_eq!(count, 64);
        assert_eq!(actual, data);
        for compact_each in [true, false] {
            actual.clear();
            let count = retained_records(&data, compact_each, |bytes| {
                assert_eq!(bytes, record);
                actual.extend_from_slice(bytes);
            })
            .unwrap();
            assert_eq!(count, 64);
            assert_eq!(actual, data);
        }

        group.throughput(Throughput::Bytes(u64::try_from(data.len()).unwrap()));
        group.bench_with_input(BenchmarkId::new("reader_copied", size), &data, |b, data| {
            b.iter(|| {
                copied_records(Reader::new(Cursor::new(black_box(data))), |bytes| {
                    black_box(bytes);
                })
                .unwrap()
            });
        });
        group.bench_with_input(
            BenchmarkId::new("bufreader_copied", size),
            &data,
            |b, data| {
                b.iter(|| {
                    copied_records(
                        BufReader::with_capacity(CHUNK_SIZE, Cursor::new(black_box(data))),
                        |bytes| {
                            black_box(bytes);
                        },
                    )
                    .unwrap()
                });
            },
        );
        for (name, compact_each) in [
            ("borrowed_compact_each", true),
            ("borrowed_compact_on_fill", false),
        ] {
            group.bench_with_input(BenchmarkId::new(name, size), &data, |b, data| {
                b.iter(|| {
                    retained_records(black_box(data), compact_each, |bytes| {
                        black_box(bytes);
                    })
                    .unwrap()
                });
            });
        }
    }
    group.finish();
}
