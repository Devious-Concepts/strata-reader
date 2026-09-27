//! What does wrapping an in-memory source cost for small versus buffer-bypassing Read calls?
//! Reader and `BufReader` both start at 8 KiB. A bare Cursor is the unbuffered control. All three
//! run the same copy loop with 1 KiB or 16 KiB output arrays and the same source bytes.
//! Construction, reads, EOF, and destruction are timed; source generation is outside timing.

use criterion::{BenchmarkId, Criterion, Throughput};
use std::hint::black_box;
use std::io::{self, BufReader, Cursor, Read};
use strata_reader::{Reader, constants::CHUNK_SIZE};

fn read_chunks<const N: usize>(
    mut reader: impl Read,
    mut visit: impl FnMut(&[u8]),
) -> io::Result<usize> {
    let mut output = [0; N];
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

fn measure<const N: usize>(c: &mut Criterion) {
    let mut group = c.benchmark_group(format!("sequential_{N}"));
    for size in [1024, CHUNK_SIZE, 1024 * 1024] {
        let data: Vec<_> = (0u8..=255).cycle().take(size).collect();

        // Validate full output with the same loop and call size used below
        let mut actual = Vec::new();
        let len = read_chunks::<N>(Reader::new(Cursor::new(&data)), |bytes| {
            actual.extend_from_slice(bytes);
        })
        .unwrap();
        assert_eq!(len, size);
        assert_eq!(actual, data);
        actual.clear();
        let len = read_chunks::<N>(
            BufReader::with_capacity(CHUNK_SIZE, Cursor::new(&data)),
            |bytes| actual.extend_from_slice(bytes),
        )
        .unwrap();
        assert_eq!(len, size);
        assert_eq!(actual, data);
        actual.clear();
        let len =
            read_chunks::<N>(Cursor::new(&data), |bytes| actual.extend_from_slice(bytes)).unwrap();
        assert_eq!(len, size);
        assert_eq!(actual, data);

        group.throughput(Throughput::Bytes(u64::try_from(size).unwrap()));
        group.bench_with_input(BenchmarkId::new("reader", size), &data, |b, data| {
            b.iter(|| {
                read_chunks::<N>(Reader::new(Cursor::new(black_box(data))), |bytes| {
                    black_box(bytes);
                })
                .unwrap()
            });
        });
        group.bench_with_input(BenchmarkId::new("bufreader", size), &data, |b, data| {
            b.iter(|| {
                read_chunks::<N>(
                    BufReader::with_capacity(CHUNK_SIZE, Cursor::new(black_box(data))),
                    |bytes| {
                        black_box(bytes);
                    },
                )
                .unwrap()
            });
        });
        group.bench_with_input(BenchmarkId::new("cursor", size), &data, |b, data| {
            b.iter(|| {
                read_chunks::<N>(Cursor::new(black_box(data)), |bytes| {
                    black_box(bytes);
                })
                .unwrap()
            });
        });
    }
    group.finish();
}

pub(super) fn bench(c: &mut Criterion) {
    measure::<1024>(c);
    measure::<16384>(c);
}
