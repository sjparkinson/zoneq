# zoneq

Query zone files.

## Usage

```
Usage: zoneq [OPTIONS] <QUERY> <FILE>

Arguments:
  <QUERY>  Owner name to match: `www`, `.example.com` or `.`
  <FILE>   Zone file to read, or - for stdin

Options:
      --type <TYPE>    Filter by record type, e.g. A, MX
      --origin <NAME>  Origin to start from, until the file sets its own $ORIGIN
      --json           Print matches as a JSON array
  -h, --help           Print help (see more with '--help')
  -V, --version        Print version
```

The query is an owner name. `www` and `www.example.com.` both match that exact name. Start it with a dot (`.example.com`) to match the name and everything below it, and `.` on its own matches every record. Names without a trailing dot work relative to the zone's origin (its SOA owner, or the first `$ORIGIN` when there's no SOA) or as written, whichever matches.

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

$ zoneq --json mail example.zone | jq -r '.[].rdata[0]'
10.0.1.5
aaaa:bbbb::5

$ zoneq --origin 0.0.127.in-addr.arpa 1 tests/samples/localhost-reverse.zone
1.0.0.127.in-addr.arpa.	1814400	IN	PTR	localhost.

$ cat example.zone | zoneq --type ns . -
```

## What it understands

Everything in RFC 1035's master file format (`$ORIGIN`, `$INCLUDE`, parentheses, comments, quoting, escapes, inherited owners and classes), plus `$TTL` from RFC 2308 and BIND's TTL units like `1h30m`. `$GENERATE` isn't supported.

## Further Reading

* [The zone file format, RFC 1035 § 5](https://tools.ietf.org/html/rfc1035#section-5)
* [The `$TTL` directive, RFC 2308 § 4](https://tools.ietf.org/html/rfc2308#section-4)
