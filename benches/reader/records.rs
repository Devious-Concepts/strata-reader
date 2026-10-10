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

/// Runs a recipe over a validation input and checks that it visits exactly its records, in order.
fn check_records(
    check: &[u8],
    size: usize,
    run: impl FnOnce(&mut dyn FnMut(&[u8])) -> io::Result<usize>,
) {
    let mut expected = check.chunks_exact(size);
    let count = run(&mut |bytes| assert_eq!(Some(bytes), expected.next())).unwrap();

    assert_eq!(count, 64);
    assert!(expected.next().is_none());
}

pub(super) fn bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("records");
    // Sizes include LF: many records per buffer, exactly one, and a record requiring growth
    for size in [64, CHUNK_SIZE, CHUNK_SIZE + 1] {
        let mut record = vec![b'x'; size - 1];
        record.push(b'\n');
        let data = record.repeat(64);

        // Validate with an input of the same shape whose records each start with a distinct
        // non-LF byte, so a skipped, repeated, or misaligned record is detected
        let mut check = data.clone();
        for (index, chunk) in check.chunks_exact_mut(size).enumerate() {
            chunk[0] = b'A' + u8::try_from(index).unwrap();
        }
        check_records(&check, size, |visit| {
            copied_records(Reader::new(Cursor::new(&check)), visit)
        });
        check_records(&check, size, |visit| {
            copied_records(
                BufReader::with_capacity(CHUNK_SIZE, Cursor::new(&check)),
                visit,
            )
        });
        for compact_each in [true, false] {
            check_records(&check, size, |visit| {
                retained_records(&check, compact_each, visit)
            });
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
