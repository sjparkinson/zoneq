//! Parsing, querying and output over synthetic zones. Run with `cargo bench`,
//! or `cargo bench -- <filter>` for the benchmarks with that in their name.

#[path = "support/zonegen.rs"]
mod zonegen;

use std::hint::black_box;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use zoneq::query::Query;
use zoneq::zone::{Zone, parse_str};
use zoneq::{Options, run};

const LARGE: usize = 100_000;
const SAMPLES: usize = 25;
/// Batches are grown until one takes at least this long, so the clock's
/// resolution doesn't swamp quick benchmarks.
const MIN_BATCH: Duration = Duration::from_millis(5);

fn parse(input: &str) -> Zone {
    parse_str(input, Path::new("bench.zone"), None).unwrap_or_else(|e| panic!("{e}"))
}

fn main() {
    // Cargo passes `--bench`, so anything starting with a dash is skipped.
    let filters: Vec<String> = std::env::args()
        .skip(1)
        .filter(|a| !a.starts_with('-'))
        .collect();
    let selected = |name: &str| filters.is_empty() || filters.iter().any(|f| name.contains(f));

    let example = include_str!("../example.zone");
    if selected("parse_str/example.zone") {
        bench("parse_str/example.zone", Some(example.len()), || {
            parse(black_box(example))
        });
    }
    for records in [1_000, 10_000, LARGE] {
        let name = format!("parse_str/{records}");
        if selected(&name) {
            let input = zonegen::generate(records);
            bench(&name, Some(input.len()), || parse(black_box(&input)));
        }
    }

    if ["query/.", "query/host500", "query/.example.com"]
        .iter()
        .any(|n| selected(n))
    {
        let zone = parse(&zonegen::generate(LARGE));
        for q in [".", "host500", ".example.com"] {
            let name = format!("query/{q}");
            if selected(&name) {
                bench(&name, None, || {
                    let query = Query::parse(black_box(q), zone.origin.as_ref()).unwrap();
                    zone.records
                        .iter()
                        .filter(|r| query.matches(&r.name))
                        .count()
                });
            }
        }
    }

    if selected("run/text") || selected("run/json") {
        let file = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("bench-100k.zone");
        std::fs::write(&file, zonegen::generate(LARGE)).unwrap();
        for json in [false, true] {
            let name = if json { "run/json" } else { "run/text" };
            if selected(name) {
                let opts = Options {
                    query: ".".into(),
                    file: file.clone(),
                    record_types: Vec::new(),
                    origin: None,
                    json,
                    data: None,
                    resolve: false,
                };
                bench(name, None, || run(&opts, &mut io::sink()).unwrap());
            }
        }
    }
}

/// Times `f` and prints the median per call, the spread between the fastest
/// and slowest tenth of samples, and the throughput when `bytes` is given.
fn bench<T>(name: &str, bytes: Option<usize>, mut f: impl FnMut() -> T) {
    let mut batch = 1u32;
    loop {
        let start = Instant::now();
        for _ in 0..batch {
            black_box(f());
        }
        if start.elapsed() >= MIN_BATCH {
            break;
        }
        batch *= 2;
    }

    let mut samples: Vec<Duration> = (0..SAMPLES)
        .map(|_| {
            let start = Instant::now();
            for _ in 0..batch {
                black_box(f());
            }
            start.elapsed() / batch
        })
        .collect();
    samples.sort();
    let median = samples[SAMPLES / 2];
    let (low, high) = (samples[SAMPLES / 10], samples[SAMPLES - 1 - SAMPLES / 10]);

    let throughput = bytes
        .map(|b| {
            format!(
                "  {:>8.1} MiB/s",
                b as f64 / median.as_secs_f64() / 1_048_576.0
            )
        })
        .unwrap_or_default();
    println!("{name:<24} {median:>12.2?}  [{low:.2?} .. {high:.2?}]{throughput}");
}
