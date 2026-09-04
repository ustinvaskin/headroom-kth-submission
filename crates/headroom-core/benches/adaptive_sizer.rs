//! Scaling benchmark for adaptive sizing's SimHash diversity estimate.
//!
//! The three distributions separate the number of input items from the number
//! of cluster representatives created by the current greedy algorithm:
//!
//! - repetitive: three exact templates;
//! - mixed: half repetitive templates and half high-diversity records;
//! - diverse: deterministic records intended to produce distinct fingerprints.

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use headroom_core::transforms::adaptive_sizer::{count_unique_simhash, hamming_distance, simhash};
use headroom_core::transforms::live_zone::DEFAULT_MODEL;
use headroom_core::transforms::{
    compress_anthropic_live_zone, AuthMode, BlockAction, LiveZoneOutcome,
};
use md5::{Digest, Md5};
use serde_json::{json, Value};
use sha2::Sha256;

const SIZES: &[usize] = &[100, 1_000, 5_000];
const STAGE_PROFILE_SIZE: usize = 5_000;
const END_TO_END_SIZES: &[usize] = &[200, 1_000, 5_000];
const HAMMING_THRESHOLD: u32 = 3;

fn repetitive_items(size: usize) -> Vec<String> {
    const TEMPLATES: &[&str] = &[
        "INFO api-gateway health check passed latency_ms=12",
        "INFO auth-service health check passed latency_ms=15",
        "INFO db-proxy health check passed latency_ms=8",
    ];

    (0..size)
        .map(|index| TEMPLATES[index % TEMPLATES.len()].to_owned())
        .collect()
}

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

fn mixed_items(size: usize) -> Vec<String> {
    let repetitive = repetitive_items(size - size / 2);
    let diverse = diverse_items(size / 2);
    repetitive
        .into_iter()
        .zip(diverse)
        .flat_map(|(repeated, unique)| [repeated, unique])
        .take(size)
        .collect()
}

fn distributions(size: usize) -> [(&'static str, Vec<String>); 3] {
    [
        ("repetitive", repetitive_items(size)),
        ("mixed", mixed_items(size)),
        ("diverse", diverse_items(size)),
    ]
}

fn unicode_items(size: usize) -> Vec<String> {
    const TEMPLATES: &[&str] = &[
        "数据库连接失败 请求编号",
        "İstanbul hizmet durumu",
        "Σίσυφος υπηρεσία συμβάν",
        "café résumé événement",
        "👩🏽‍💻 build sonuçlandı",
    ];

    (0..size)
        .map(|index| format!("{}-{index:05}", TEMPLATES[index % TEMPLATES.len()]))
        .collect()
}

fn anthropic_tool_result_body(tool_use_id: &str, content: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "model": "claude-sonnet-4-6",
        "max_tokens": 64,
        "system": "you are a helpful assistant",
        "messages": [{
            "role": "user",
            "content": [{
                "type": "tool_result",
                "tool_use_id": tool_use_id,
                "content": content,
            }],
        }],
    }))
    .expect("serialize Anthropic request")
}

fn coding_agent_body(size: usize) -> Vec<u8> {
    let records: Vec<Value> = (0..size)
        .map(|index| {
            json!({
                "id": index,
                "status": "ok",
                "value": format!("repeat-pattern-{}", index % 3),
            })
        })
        .collect();
    let payload = serde_json::to_string(&records).expect("serialize tool records");

    anthropic_tool_result_body("toolu_adaptive_sizer_benchmark", &payload)
}

fn log_tool_output(size: usize, include_unicode: bool) -> String {
    let mut lines = Vec::with_capacity(size);

    for index in 0..size {
        let component = match index % 5 {
            0 => "compiler",
            1 => "test-runner",
            2 => "package-manager",
            3 => "artifact-store",
            _ => "build-cache",
        };
        let message = if include_unicode {
            format!(
                "compiling módulo_{} résumé=ok worker=東京 request_id={index:05}",
                index % 23
            )
        } else {
            format!(
                "compiling module_{} resume=ok worker=build request_id={index:05}",
                index % 23
            )
        };
        lines.push(format!(
            "[INFO] 2026-09-02T12:{:02}:{:02}.000Z component={component} {message}",
            (index / 60) % 60,
            index % 60,
        ));
    }

    lines.join("\n")
}

fn search_tool_output(size: usize, include_unicode: bool) -> String {
    let mut lines = Vec::with_capacity(size);

    for index in 0..size {
        let file = format!("workspace/src/module_{}/handler.rs", index % 10);
        let content = if include_unicode {
            format!(
                "let résumé_{} = process_request(\"東京-{}\");",
                index % 37,
                index % 19
            )
        } else {
            format!(
                "let request_{} = process_request(\"job-{}\");",
                index % 37,
                index % 19
            )
        };
        lines.push(format!("{file}:{}:{content}", 40 + index / 10));
    }

    lines.join("\n")
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn print_live_zone_proof(size: usize, body: &[u8]) {
    print_route_proof("smart_crusher", &size.to_string(), "smart_crusher", body);
}

fn print_route_proof(route: &str, fixture: &str, expected_strategy: &str, body: &[u8]) {
    let outcome = compress_anthropic_live_zone(body, 0, AuthMode::Payg, DEFAULT_MODEL)
        .expect("compress valid Anthropic request");
    let LiveZoneOutcome::Modified { new_body, manifest } = outcome else {
        panic!("expected {route} to modify {fixture}");
    };
    let action = manifest
        .block_outcomes
        .iter()
        .find(|outcome| outcome.block_type == "tool_result")
        .expect("tool_result outcome");
    let BlockAction::Compressed {
        strategy,
        original_bytes,
        compressed_bytes,
        original_tokens,
        compressed_tokens,
    } = &action.action
    else {
        panic!("expected {route} compression, got {:?}", action.action);
    };
    assert_eq!(*strategy, expected_strategy);
    assert!(compressed_bytes < original_bytes);
    assert!(compressed_tokens < original_tokens);

    let output = new_body.get().as_bytes();
    eprintln!(
        "E2E_PROOF route={route} fixture={fixture} input_sha256={} output_sha256={} output_bytes={} \
         strategy={strategy} original_tokens={original_tokens} compressed_tokens={compressed_tokens}",
        sha256_hex(body),
        sha256_hex(output),
        output.len(),
    );
}

fn reference_simhash(text: &str) -> u64 {
    let lower = text.to_lowercase();
    let chars: Vec<char> = lower.chars().collect();
    let iter_count = if chars.len() <= 3 { 1 } else { chars.len() - 3 };
    let mut votes = [0_i32; 64];

    for index in 0..iter_count {
        let gram: String = chars.iter().skip(index).take(4).collect();
        let digest = Md5::digest(gram.as_bytes());
        let hash = u64::from_be_bytes([
            digest[0], digest[1], digest[2], digest[3], digest[4], digest[5], digest[6], digest[7],
        ]);

        for (bit, vote) in votes.iter_mut().enumerate() {
            if (hash >> bit) & 1 == 1 {
                *vote += 1;
            } else {
                *vote -= 1;
            }
        }
    }

    votes
        .iter()
        .enumerate()
        .fold(0_u64, |fingerprint, (bit, &vote)| {
            if vote > 0 {
                fingerprint | (1 << bit)
            } else {
                fingerprint
            }
        })
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

fn preprocess_for_simhash(items: &[String]) -> Vec<Vec<char>> {
    items
        .iter()
        .map(|item| item.to_lowercase().chars().collect())
        .collect()
}

fn hash_character_grams(items: &[Vec<char>]) -> Vec<Vec<u64>> {
    items
        .iter()
        .map(|chars| {
            let iter_count = if chars.len() <= 3 { 1 } else { chars.len() - 3 };
            (0..iter_count)
                .map(|index| {
                    let gram: String = chars.iter().skip(index).take(4).collect();
                    let digest = Md5::digest(gram.as_bytes());
                    u64::from_be_bytes([
                        digest[0], digest[1], digest[2], digest[3], digest[4], digest[5],
                        digest[6], digest[7],
                    ])
                })
                .collect()
        })
        .collect()
}

fn accumulate_votes(items: &[Vec<u64>]) -> Vec<u64> {
    items
        .iter()
        .map(|hashes| {
            let mut votes = [0_i32; 64];
            for &hash in hashes {
                for (bit, vote) in votes.iter_mut().enumerate() {
                    if (hash >> bit) & 1 == 1 {
                        *vote += 1;
                    } else {
                        *vote -= 1;
                    }
                }
            }

            votes
                .iter()
                .enumerate()
                .fold(0_u64, |fingerprint, (bit, &vote)| {
                    if vote > 0 {
                        fingerprint | (1 << bit)
                    } else {
                        fingerprint
                    }
                })
        })
        .collect()
}

fn bench_distribution(
    group: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>,
    name: &str,
    items: Vec<String>,
) {
    let item_refs: Vec<&str> = items.iter().map(String::as_str).collect();
    group.throughput(Throughput::Elements(item_refs.len() as u64));
    group.bench_with_input(
        BenchmarkId::new(name, item_refs.len()),
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

fn bench_count_unique_simhash(c: &mut Criterion) {
    let mut group = c.benchmark_group("adaptive_sizer/count_unique_simhash");

    for &size in SIZES {
        bench_distribution(&mut group, "repetitive", repetitive_items(size));
        bench_distribution(&mut group, "mixed", mixed_items(size));
        bench_distribution(&mut group, "diverse", diverse_items(size));
    }

    group.finish();
}

fn bench_components(c: &mut Criterion) {
    let mut simhash_group = c.benchmark_group("adaptive_sizer/components/simhash_generation");
    for &size in SIZES {
        for (name, items) in distributions(size) {
            let item_refs: Vec<&str> = items.iter().map(String::as_str).collect();
            simhash_group.throughput(Throughput::Elements(item_refs.len() as u64));
            simhash_group.bench_with_input(
                BenchmarkId::new(name, item_refs.len()),
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
    }
    simhash_group.finish();

    let mut clustering_group = c.benchmark_group("adaptive_sizer/components/greedy_clustering");
    for &size in SIZES {
        for (name, items) in distributions(size) {
            let fingerprints: Vec<u64> = items.iter().map(|item| simhash(item)).collect();
            clustering_group.throughput(Throughput::Elements(fingerprints.len() as u64));
            clustering_group.bench_with_input(
                BenchmarkId::new(name, fingerprints.len()),
                &fingerprints,
                |bencher, values| {
                    bencher.iter(|| black_box(greedy_cluster_count(black_box(values.as_slice()))));
                },
            );
        }
    }
    clustering_group.finish();
}

fn bench_simhash_stages(c: &mut Criterion) {
    let mut preprocessing_group = c.benchmark_group("adaptive_sizer/stages/preprocessing");
    for (name, items) in distributions(STAGE_PROFILE_SIZE) {
        preprocessing_group.throughput(Throughput::Elements(items.len() as u64));
        preprocessing_group.bench_with_input(
            BenchmarkId::new(name, items.len()),
            &items,
            |bencher, values| {
                bencher.iter(|| black_box(preprocess_for_simhash(black_box(values.as_slice()))));
            },
        );
    }
    preprocessing_group.finish();

    let mut hashing_group = c.benchmark_group("adaptive_sizer/stages/gram_hashing");
    for (name, items) in distributions(STAGE_PROFILE_SIZE) {
        let prepared = preprocess_for_simhash(&items);
        hashing_group.throughput(Throughput::Elements(prepared.len() as u64));
        hashing_group.bench_with_input(
            BenchmarkId::new(name, prepared.len()),
            &prepared,
            |bencher, values| {
                bencher.iter(|| black_box(hash_character_grams(black_box(values.as_slice()))));
            },
        );
    }
    hashing_group.finish();

    let mut voting_group = c.benchmark_group("adaptive_sizer/stages/vote_accumulation");
    for (name, items) in distributions(STAGE_PROFILE_SIZE) {
        let prepared = preprocess_for_simhash(&items);
        let hashes = hash_character_grams(&prepared);
        voting_group.throughput(Throughput::Elements(hashes.len() as u64));
        voting_group.bench_with_input(
            BenchmarkId::new(name, hashes.len()),
            &hashes,
            |bencher, values| {
                bencher.iter(|| black_box(accumulate_votes(black_box(values.as_slice()))));
            },
        );
    }
    voting_group.finish();
}

fn bench_unicode_control(c: &mut Criterion) {
    let items = unicode_items(STAGE_PROFILE_SIZE);
    let item_refs: Vec<&str> = items.iter().map(String::as_str).collect();
    let mut group = c.benchmark_group("adaptive_sizer/unicode_simhash_generation");
    group.throughput(Throughput::Elements(item_refs.len() as u64));

    group.bench_with_input("reference", &item_refs, |bencher, values| {
        bencher.iter(|| {
            black_box(
                values
                    .iter()
                    .map(|value| reference_simhash(black_box(value)))
                    .collect::<Vec<_>>(),
            )
        });
    });
    group.bench_with_input("candidate", &item_refs, |bencher, values| {
        bencher.iter(|| {
            black_box(
                values
                    .iter()
                    .map(|value| simhash(black_box(value)))
                    .collect::<Vec<_>>(),
            )
        });
    });

    group.finish();
}

fn bench_coding_agent_live_zone(c: &mut Criterion) {
    let mut group = c.benchmark_group("adaptive_sizer/coding_agent_live_zone");

    for &size in END_TO_END_SIZES {
        let body = coding_agent_body(size);
        print_live_zone_proof(size, &body);
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(size),
            &body,
            |bencher, value| {
                bencher.iter(|| {
                    black_box(
                        compress_anthropic_live_zone(
                            black_box(value.as_slice()),
                            0,
                            AuthMode::Payg,
                            DEFAULT_MODEL,
                        )
                        .expect("compress valid Anthropic request"),
                    )
                });
            },
        );
    }

    group.finish();
}

fn bench_log_compressor_live_zone(c: &mut Criterion) {
    let mut group = c.benchmark_group("adaptive_sizer/log_compressor_live_zone");

    for &size in END_TO_END_SIZES {
        let content = log_tool_output(size, false);
        let body = anthropic_tool_result_body("toolu_log_adaptive_sizer_benchmark", &content);
        print_route_proof(
            "log_compressor",
            &format!("ascii-{size}"),
            "log_compressor",
            &body,
        );
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(BenchmarkId::new("ascii", size), &body, |bencher, value| {
            bencher.iter(|| {
                black_box(
                    compress_anthropic_live_zone(
                        black_box(value.as_slice()),
                        0,
                        AuthMode::Payg,
                        DEFAULT_MODEL,
                    )
                    .expect("compress valid log request"),
                )
            });
        });
    }

    let unicode_size = 1_000;
    let content = log_tool_output(unicode_size, true);
    let body = anthropic_tool_result_body("toolu_log_unicode_adaptive_sizer", &content);
    print_route_proof(
        "log_compressor",
        &format!("unicode-{unicode_size}"),
        "log_compressor",
        &body,
    );
    group.throughput(Throughput::Elements(unicode_size as u64));
    group.bench_with_input(
        BenchmarkId::new("unicode", unicode_size),
        &body,
        |bencher, value| {
            bencher.iter(|| {
                black_box(
                    compress_anthropic_live_zone(
                        black_box(value.as_slice()),
                        0,
                        AuthMode::Payg,
                        DEFAULT_MODEL,
                    )
                    .expect("compress valid Unicode log request"),
                )
            });
        },
    );

    group.finish();
}

fn bench_search_compressor_live_zone(c: &mut Criterion) {
    let mut group = c.benchmark_group("adaptive_sizer/search_compressor_live_zone");

    for &size in END_TO_END_SIZES {
        let content = search_tool_output(size, false);
        let body = anthropic_tool_result_body("toolu_search_adaptive_sizer_benchmark", &content);
        print_route_proof(
            "search_compressor",
            &format!("ascii-{size}"),
            "search_compressor",
            &body,
        );
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(BenchmarkId::new("ascii", size), &body, |bencher, value| {
            bencher.iter(|| {
                black_box(
                    compress_anthropic_live_zone(
                        black_box(value.as_slice()),
                        0,
                        AuthMode::Payg,
                        DEFAULT_MODEL,
                    )
                    .expect("compress valid search request"),
                )
            });
        });
    }

    let unicode_size = 1_000;
    let content = search_tool_output(unicode_size, true);
    let body = anthropic_tool_result_body("toolu_search_unicode_adaptive_sizer", &content);
    print_route_proof(
        "search_compressor",
        &format!("unicode-{unicode_size}"),
        "search_compressor",
        &body,
    );
    group.throughput(Throughput::Elements(unicode_size as u64));
    group.bench_with_input(
        BenchmarkId::new("unicode", unicode_size),
        &body,
        |bencher, value| {
            bencher.iter(|| {
                black_box(
                    compress_anthropic_live_zone(
                        black_box(value.as_slice()),
                        0,
                        AuthMode::Payg,
                        DEFAULT_MODEL,
                    )
                    .expect("compress valid Unicode search request"),
                )
            });
        },
    );

    group.finish();
}

criterion_group!(
    benches,
    bench_count_unique_simhash,
    bench_components,
    bench_simhash_stages,
    bench_unicode_control,
    bench_coding_agent_live_zone,
    bench_log_compressor_live_zone,
    bench_search_compressor_live_zone
);
criterion_main!(benches);
