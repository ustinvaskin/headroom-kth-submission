//! High-cardinality scaling benchmark for the rejected greedy SimHash-clustering
//! hypothesis. Kept separate from `adaptive_sizer` so routine 100/1k/5k checks
//! do not initialize large fixtures.

use std::hint::black_box;
use std::time::Duration;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use headroom_core::transforms::adaptive_sizer::{count_unique_simhash, hamming_distance, simhash};

const LARGE_DIVERSE_SIZES: &[usize] = &[5_000, 10_000, 12_500, 15_000, 25_000];
const HAMMING_THRESHOLD: u32 = 3;

fn diverse_items(size: usize) -> Vec<String> {
    let mut state = 0x5eed_2026_0902_cafe_u64;
    (0..size)
        .map(|index| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let first = state;
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let second = state;
            format!(
                "artifact-{index:05x} module-{first:016x}.rs symbol-{second:016x} \
                 checksum-{first:016x}{second:016x}"
            )
        })
        .collect()
}

fn greedy_cluster_count(fingerprints: &[u64]) -> usize {
    let mut representatives = Vec::new();

    'fingerprint: for &fingerprint in fingerprints {
        for &representative in &representatives {
            if hamming_distance(fingerprint, representative) <= HAMMING_THRESHOLD {
                continue 'fingerprint;
            }
        }
        representatives.push(fingerprint);
    }

    representatives.len()
}

fn greedy_cluster_stats(fingerprints: &[u64]) -> (usize, u64) {
    let mut representatives = Vec::new();
    let mut comparisons = 0_u64;

    'fingerprint: for &fingerprint in fingerprints {
        for &representative in &representatives {
            comparisons += 1;
            if hamming_distance(fingerprint, representative) <= HAMMING_THRESHOLD {
                continue 'fingerprint;
            }
        }
        representatives.push(fingerprint);
    }

    (representatives.len(), comparisons)
}

fn configure_group<'a>(
    group: &mut criterion::BenchmarkGroup<'a, criterion::measurement::WallTime>,
) {
    group
        .sample_size(20)
        .warm_up_time(Duration::from_millis(500))
        .measurement_time(Duration::from_secs(5));
}

fn bench_large_diverse_scaling(c: &mut Criterion) {
    let mut cases = Vec::new();
    for &size in LARGE_DIVERSE_SIZES {
        let items = diverse_items(size);
        let item_refs: Vec<&str> = items.iter().map(String::as_str).collect();
        let fingerprints: Vec<u64> = item_refs.iter().map(|item| simhash(item)).collect();
        let (clusters, comparisons) = greedy_cluster_stats(&fingerprints);
        assert_eq!(
            clusters, size,
            "large diverse fixture must retain one representative per item"
        );
        assert_eq!(
            comparisons,
            (size as u64 * (size as u64 - 1)) / 2,
            "large diverse fixture must exercise every greedy comparison"
        );
        eprintln!("LARGE_DIVERSE_PROOF items={size} clusters={clusters} comparisons={comparisons}");
        cases.push((items, fingerprints));
    }

    let mut full_group = c.benchmark_group("adaptive_sizer_large/full_operation");
    configure_group(&mut full_group);
    for (items, _) in &cases {
        let item_refs: Vec<&str> = items.iter().map(String::as_str).collect();
        full_group.throughput(Throughput::Elements(item_refs.len() as u64));
        full_group.bench_with_input(
            BenchmarkId::from_parameter(item_refs.len()),
            &item_refs,
            |bencher, values| {
                bencher.iter(|| {
                    black_box(count_unique_simhash(
                        black_box(values.as_slice()),
                        HAMMING_THRESHOLD,
                    ))
                });
            },
        );
    }
    full_group.finish();

    let mut simhash_group = c.benchmark_group("adaptive_sizer_large/simhash_generation");
    configure_group(&mut simhash_group);
    for (items, _) in &cases {
        let item_refs: Vec<&str> = items.iter().map(String::as_str).collect();
        simhash_group.throughput(Throughput::Elements(item_refs.len() as u64));
        simhash_group.bench_with_input(
            BenchmarkId::from_parameter(item_refs.len()),
            &item_refs,
            |bencher, values| {
                bencher.iter(|| {
                    black_box(
                        values
                            .iter()
                            .map(|value| simhash(black_box(value)))
                            .collect::<Vec<_>>(),
                    )
                });
            },
        );
    }
    simhash_group.finish();

    let mut clustering_group = c.benchmark_group("adaptive_sizer_large/greedy_clustering");
    configure_group(&mut clustering_group);
    for (_, fingerprints) in &cases {
        clustering_group.throughput(Throughput::Elements(fingerprints.len() as u64));
        clustering_group.bench_with_input(
            BenchmarkId::from_parameter(fingerprints.len()),
            fingerprints,
            |bencher, values| {
                bencher.iter(|| black_box(greedy_cluster_count(black_box(values.as_slice()))));
            },
        );
    }
    clustering_group.finish();
}

criterion_group!(benches, bench_large_diverse_scaling);
criterion_main!(benches);
