# Contributing

## Checks

CI runs these on every pull request, so run them before you push:

```sh
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

The sample zones in `tests/samples` double as fixtures for the tests and the fuzzer's seed corpus.

## Benchmarking and profiling

`cargo bench` runs the Criterion benchmarks in `benches/parse.rs` on generated zones of up to 100k records. To compare a change against `main`, save a baseline there first:

```sh
cargo bench -- --save-baseline main   # on main
cargo bench -- --baseline main        # on your branch
```

For anything that needs a real file, generate one. A million records comes out at about 38 MB:

```sh
cargo run --release --example genzone -- 1000000 > target/bench-1m.zone
```

The `profiling` profile is `release` with symbols kept, which is what a CPU profiler like [samply](https://github.com/mstange/samply) needs to show readable function names. To time the whole CLI, use [hyperfine](https://github.com/sharkdp/hyperfine):

```sh
cargo build --profile profiling
samply record target/profiling/zoneq . target/bench-1m.zone > /dev/null
samply record cargo bench --bench parse -- --profile-time 10 parse_str/100000

cargo build --release
hyperfine --warmup 3 'target/release/zoneq . target/bench-1m.zone' 'target/release/zoneq --json . target/bench-1m.zone'
```

To count heap allocations, build with the `dhat-heap` feature. It prints a summary and writes `dhat-heap.json` to the current directory, which you can open in [DHAT's viewer](https://nnethercote.github.io/dh_view/dh_view.html):

```sh
cargo run --profile profiling --features dhat-heap -- . target/bench-1m.zone > /dev/null
```

## Fuzzing

There are [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz) targets, which need nightly Rust:

- `parse` checks the parser doesn't panic on any input, and that every zone it accepts prints back out as lines that parse to the same records.
- `resolve` reads a question from the first line, `<qname> [TYPE,TYPE]`, and a zone from the rest, then checks the answer stays inside the zone and comes out the same when asked by its full name.
- `query` reads a query and a `--data` value from the first two lines and a zone from the rest, and checks every owner finds itself and a subtree query finds everything the exact one does.
- `name` reads two names and an origin, one per line, and checks parents, labels and `is_at_or_below` agree with each other, including when every octet is spelt as a `\DDD` escape.

Seed `parse` with the sample zones and the dictionary of zone file syntax:

```sh
cargo install cargo-fuzz
cargo +nightly fuzz run parse fuzz/corpus/parse tests/samples -- -dict=fuzz/zone.dict
```

The other targets want their question lines on top of a zone, so their seeds live in `fuzz/seeds/<target>`:

```sh
cargo +nightly fuzz run resolve fuzz/corpus/resolve fuzz/seeds/resolve -- -dict=fuzz/zone.dict
```

CI runs each target for a minute on every pull request.

Crashes land in `fuzz/artifacts/<target>/`. Pass one in place of the corpus directories to replay it.

## Releases

Every merge to `main` is published to crates.io as `0.2.<commit count>+<sha>`, which is also what `zoneq --version` prints. There's nothing to bump by hand.
