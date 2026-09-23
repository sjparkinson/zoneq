//! Parsing, querying and output over synthetic zones. Run with `cargo bench`.

#[path = "support/zonegen.rs"]
mod zonegen;

use std::hint::black_box;
use std::io;
use std::path::{Path, PathBuf};

use criterion::{
    BenchmarkId, Criterion, SamplingMode, Throughput, criterion_group, criterion_main,
};
use zoneq::query::Query;
use zoneq::zone::{Zone, parse_str};
use zoneq::{Options, run};

const LARGE: usize = 100_000;

fn parse(input: &str) -> Zone {
    parse_str(input, Path::new("bench.zone"), None).unwrap_or_else(|e| panic!("{e}"))
}

fn bench_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_str");

    let example = include_str!("../example.zone");
    group.throughput(Throughput::Bytes(example.len() as u64));
    group.bench_function("example.zone", |b| b.iter(|| parse(black_box(example))));

    for records in [1_000, 10_000, LARGE] {
        let input = zonegen::generate(records);
        group.throughput(Throughput::Bytes(input.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(records), &input, |b, input| {
            b.iter(|| parse(black_box(input)));
        });
    }
    group.finish();
}

fn bench_query(c: &mut Criterion) {
    let zone = parse(&zonegen::generate(LARGE));
    let mut group = c.benchmark_group("query");
    for q in [".", "host500", ".example.com"] {
        group.bench_with_input(BenchmarkId::from_parameter(q), q, |b, q| {
            b.iter(|| {
                let query = Query::parse(black_box(q), zone.origin.as_ref()).unwrap();
                zone.records
                    .iter()
                    .filter(|r| query.matches(&r.name))
                    .count()
            });
        });
    }
    group.finish();
}

fn bench_run(c: &mut Criterion) {
    let file = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("bench-100k.zone");
    std::fs::write(&file, zonegen::generate(LARGE)).unwrap();

    let mut group = c.benchmark_group("run");
    group.sample_size(20).sampling_mode(SamplingMode::Flat);
    for json in [false, true] {
        let opts = Options {
            query: ".".into(),
            file: file.clone(),
            record_types: Vec::new(),
            origin: None,
            json,
            data: None,
        };
        let name = if json { "json" } else { "text" };
        group.bench_function(name, |b| b.iter(|| run(&opts, &mut io::sink()).unwrap()));
    }
    group.finish();
}

criterion_group!(benches, bench_parse, bench_query, bench_run);
criterion_main!(benches);
