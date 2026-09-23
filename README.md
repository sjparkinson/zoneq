# zoneq

Query zone files.

## Install

```sh
cargo install zoneq
```

Every merge to `main` is published to crates.io as `0.2.<commit count>+<sha>`, which is also what `zoneq --version` prints.

## Usage

```
Usage: zoneq [OPTIONS] <QUERY> <FILE>

Arguments:
  <QUERY>  Owner name to match: `www`, `.example.com` or `.`
  <FILE>   Zone file to read, or - for stdin

Options:
      --type <TYPE>    Filter by record type, e.g. MX, or several like A,AAAA
      --origin <NAME>  Origin to start from, until the file sets its own $ORIGIN
      --json           Print matches as a JSON array
  -h, --help           Print help (see more with '--help')
  -V, --version        Print version
```

The query is an owner name. `www` and `www.example.com.` both match that exact name. Start it with a dot (`.example.com`) to match the name and everything below it, and `.` on its own matches every record. Names without a trailing dot work relative to the zone's origin (its SOA owner, or the first `$ORIGIN` when there's no SOA) or as written, whichever matches.

`--type` narrows the matches to one or more record types, in any case. Give it a comma-separated list (`--type a,aaaa`), repeat the flag (`--type a --type aaaa`), or both.

Matches print one per line in zone file format, with absolute names and the TTL and class filled in, so the output is easy to `cut` or `awk`. Pass `--json` if you'd rather hand it to `jq`.

Like `grep`, it exits 0 when something matched, 1 when nothing did, and 2 on errors.

## Examples

```sh
$ zoneq www example.zone
www.example.com.	86400	IN	CNAME	services.example.com.

$ zoneq --type mx @ tests/samples/example.com.zone
example.com.	3600	IN	MX	10 mail.example.com.
example.com.	3600	IN	MX	20 mail2.example.com.
example.com.	3600	IN	MX	50 mail3.example.com.

$ zoneq --type aaaa .example.com example.zone | wc -l
6

$ zoneq --type a,aaaa mail example.zone
mail.example.com.	86400	IN	A	10.0.1.5
mail.example.com.	86400	IN	AAAA	aaaa:bbbb::5

$ zoneq --json mail example.zone | jq -r '.[].rdata[0]'
10.0.1.5
aaaa:bbbb::5

$ zoneq --origin 0.0.127.in-addr.arpa 1 tests/samples/localhost-reverse.zone
1.0.0.127.in-addr.arpa.	1814400	IN	PTR	localhost.

$ cat example.zone | zoneq --type ns . -
```

## What it understands

Everything in RFC 1035's master file format (`$ORIGIN`, `$INCLUDE`, parentheses, comments, quoting, escapes, inherited owners and classes), plus `$TTL` from RFC 2308 and BIND's TTL units like `1h30m`. `$GENERATE` isn't supported.

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

The parser has a [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz) target, which needs nightly Rust. It checks the parser doesn't panic on any input, and that every zone it accepts prints back out as lines that parse to the same records. Seed it with the sample zones and the dictionary of zone file syntax:

```sh
cargo install cargo-fuzz
cargo +nightly fuzz run parse fuzz/corpus/parse tests/samples -- -dict=fuzz/zone.dict
```

Crashes land in `fuzz/artifacts/parse/`. Pass one in place of the corpus directories to replay it.

## Further Reading

* [The zone file format, RFC 1035 § 5](https://tools.ietf.org/html/rfc1035#section-5)
* [The `$TTL` directive, RFC 2308 § 4](https://tools.ietf.org/html/rfc2308#section-4)
