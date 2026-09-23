use std::fmt::Write as _;
use std::fs;
use std::io::Read;
use std::path::Path;

use crate::error::Error;
use crate::lexer::{self, Line, Token};
use crate::name::Name;
use crate::ttl::parse_ttl;

const MAX_INCLUDE_DEPTH: usize = 16;

#[derive(Clone, Debug, serde::Serialize)]
pub struct Record {
    pub name: Name,
    pub ttl: u32,
    pub class: String,
    #[serde(rename = "type")]
    pub rtype: String,
    pub rdata: Vec<String>,
}

#[derive(Debug)]
pub struct Zone {
    /// The zone's apex: the owner of the first SOA, falling back to the
    /// first top-level `$ORIGIN` and then `--origin`. BIND writes its zone
    /// files starting with `$ORIGIN .`, so the SOA is the better guide.
    pub origin: Option<Name>,
    pub records: Vec<Record>,
}

pub fn parse_file(path: &Path, origin: Option<Name>) -> Result<Zone, Error> {
    let input = read(path)?;
    parse_str(&input, path, origin)
}

/// Reads zone text from `reader`, such as stdin, then parses it like
/// [`parse_str`].
pub fn parse_reader(
    mut reader: impl Read,
    source: &Path,
    origin: Option<Name>,
) -> Result<Zone, Error> {
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .map_err(|e| Error::io(source, e))?;
    parse_str(&decode(bytes), source, origin)
}

/// Parses zone text. `source` is used in error messages and as the base for
/// `$INCLUDE` paths.
pub fn parse_str(input: &str, source: &Path, origin: Option<Name>) -> Result<Zone, Error> {
    let mut parser = Parser {
        records: Vec::new(),
        origin: None,
    };
    let mut state = State {
        origin: origin.clone(),
        ..State::default()
    };
    parser.parse(input, source, &mut state)?;
    let apex = parser
        .records
        .iter()
        .find(|r| r.rtype == "SOA")
        .map(|r| r.name.clone());
    Ok(Zone {
        origin: apex.or(parser.origin).or(origin),
        records: parser.records,
    })
}

fn read(path: &Path) -> Result<String, Error> {
    let bytes = fs::read(path).map_err(|e| Error::io(path, e))?;
    Ok(decode(bytes))
}

/// Zone files are octets, not UTF-8. Anything that isn't valid UTF-8
/// becomes a `\DDD` escape, which means the same octet, so a stray
/// Latin-1 byte in a comment or TXT record doesn't sink the whole file.
fn decode(bytes: Vec<u8>) -> String {
    let bytes = match String::from_utf8(bytes) {
        Ok(text) => return text,
        Err(e) => e.into_bytes(),
    };
    let mut text = String::with_capacity(bytes.len());
    for chunk in bytes.utf8_chunks() {
        text.push_str(chunk.valid());
        for byte in chunk.invalid() {
            // After an unescaped backslash, the digits alone finish the escape.
            let backslashes = text.bytes().rev().take_while(|&b| b == b'\\').count();
            if backslashes % 2 == 0 {
                text.push('\\');
            }
            let _ = write!(text, "{byte:03}");
        }
    }
    text
}

#[derive(Clone, Default)]
struct State {
    origin: Option<Name>,
    default_ttl: Option<u32>,
    last_ttl: Option<u32>,
    last_class: Option<String>,
    last_owner: Option<Name>,
    depth: usize,
}

struct Parser {
    records: Vec<Record>,
    origin: Option<Name>,
}

impl Parser {
    fn parse(&mut self, input: &str, source: &Path, state: &mut State) -> Result<(), Error> {
        let lines = lexer::tokenise(input).map_err(|e| Error::parse(source, e.line, e.message))?;
        for line in &lines {
            let first = &line.tokens[0];
            if !line.leading_blank && !first.quoted && first.text.starts_with('$') {
                self.directive(line, source, state)?;
            } else {
                let record = record(line, state)
                    .map_err(|message| Error::parse(source, line.number, message))?;
                self.records.push(record);
            }
        }
        Ok(())
    }

    fn directive(&mut self, line: &Line, source: &Path, state: &mut State) -> Result<(), Error> {
        let fail = |message: String| Error::parse(source, line.number, message);
        let directive = line.tokens[0].text.to_ascii_uppercase();
        let args = &line.tokens[1..];

        match directive.as_str() {
            "$ORIGIN" => {
                let [name] = args else {
                    return Err(fail("$ORIGIN takes one name".into()));
                };
                let origin = directive_origin(&name.text, state).map_err(fail)?;
                if state.depth == 0 && self.origin.is_none() {
                    self.origin = Some(origin.clone());
                }
                state.origin = Some(origin);
            }
            "$TTL" => {
                let [ttl] = args else {
                    return Err(fail("$TTL takes one value".into()));
                };
                let ttl = parse_ttl(&ttl.text)
                    .ok_or_else(|| fail(format!("invalid TTL {}", ttl.text)))?;
                state.default_ttl = Some(ttl);
            }
            "$INCLUDE" => {
                let (file, origin) = match args {
                    [file] => (file, None),
                    [file, origin] => (file, Some(origin)),
                    _ => return Err(fail("$INCLUDE takes a file and an optional origin".into())),
                };
                if state.depth >= MAX_INCLUDE_DEPTH {
                    return Err(fail(format!(
                        "$INCLUDE nested more than {MAX_INCLUDE_DEPTH} deep, is there a loop?"
                    )));
                }

                // The included file works on a copy, so nothing it does leaks
                // back into this one.
                let mut child = state.clone();
                child.depth += 1;
                if let Some(origin) = origin {
                    child.origin = Some(directive_origin(&origin.text, state).map_err(fail)?);
                }

                let base = source.parent().unwrap_or(Path::new(""));
                let path = base.join(&file.text);
                let input = read(&path)?;
                self.parse(&input, &path, &mut child)?;
            }
            _ => {
                return Err(fail(format!(
                    "unsupported directive {}",
                    line.tokens[0].text
                )));
            }
        }
        Ok(())
    }
}

/// Resolves a name given to `$ORIGIN` or `$INCLUDE`. Strictly, a relative
/// name with no origin set is an error, but plenty of real files (like
/// `tests/samples/redhat.zone`) write `$ORIGIN domain.com` without the dot,
/// so with no origin it's read as absolute.
fn directive_origin(raw: &str, state: &State) -> Result<Name, String> {
    let root = Name::root();
    Name::parse(raw, Some(state.origin.as_ref().unwrap_or(&root)))
}

fn record(line: &Line, state: &mut State) -> Result<Record, String> {
    let tokens = &line.tokens;
    let mut idx = 0;

    let owner = if line.leading_blank {
        state
            .last_owner
            .clone()
            .ok_or("record has no owner name and there's no previous one to inherit")?
    } else {
        idx += 1;
        Name::parse(&tokens[0].text, state.origin.as_ref())?
    };

    let mut ttl = None;
    let mut class = None;
    while let Some(token) = tokens.get(idx) {
        if ttl.is_none()
            && let Some(t) = parse_ttl(&token.text)
        {
            ttl = Some(t);
        } else if class.is_none()
            && let Some(c) = parse_class(&token.text)
        {
            class = Some(c);
        } else {
            break;
        }
        idx += 1;
    }

    let rtype = tokens.get(idx).ok_or("missing record type")?;
    // Types start with a letter, so this is a TTL that didn't parse, such
    // as one over 2^31 - 1.
    if rtype.text.starts_with(|c: char| c.is_ascii_digit()) {
        return Err(format!("invalid TTL {}", rtype.text));
    }
    if !is_type_like(&rtype.text) {
        return Err(format!("expected a record type, found {}", rtype.text));
    }
    let rtype = rtype.text.to_ascii_uppercase();
    let fields = &tokens[idx + 1..];
    if fields.is_empty() {
        return Err(format!("{rtype} record has no data"));
    }

    // RFC 3597 generic rdata (`\# <len> <hex>`) is opaque, so there are no
    // names to qualify and no SOA fields to read.
    let generic = fields[0].text == r"\#";

    if rtype == "SOA" && !generic {
        if fields.len() != 7 {
            return Err(format!("SOA needs 7 fields, found {}", fields.len()));
        }
        let minimum = parse_ttl(&fields[6].text)
            .ok_or_else(|| format!("invalid SOA minimum {}", fields[6].text))?;
        // Like BIND, with no TTL to go on, the SOA minimum becomes the
        // default for this record and the ones after it.
        if ttl.is_none() && state.default_ttl.is_none() && state.last_ttl.is_none() {
            state.default_ttl = Some(minimum);
        }
    }

    if let Some(c) = &class {
        state.last_class = Some(c.clone());
    }
    let class = class
        .or_else(|| state.last_class.clone())
        .unwrap_or_else(|| "IN".to_string());

    if ttl.is_some() {
        state.last_ttl = ttl;
    }
    let ttl = ttl
        .or(state.default_ttl)
        .or(state.last_ttl)
        .ok_or("no TTL given and no $TTL, previous TTL or SOA to fall back on")?;

    let name_fields = if generic { &[] } else { name_fields(&rtype) };
    let rdata = fields
        .iter()
        .enumerate()
        .map(|(i, token)| rdata_field(token, name_fields.contains(&i), state))
        .collect::<Result<_, _>>()?;

    state.last_owner = Some(owner.clone());
    Ok(Record {
        name: owner,
        ttl,
        class,
        rtype,
        rdata,
    })
}

fn parse_class(raw: &str) -> Option<String> {
    let upper = raw.to_ascii_uppercase();
    let known = matches!(upper.as_str(), "IN" | "CH" | "HS" | "CS");
    let generic = upper
        .strip_prefix("CLASS")
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
    (known || generic).then_some(upper)
}

fn is_type_like(raw: &str) -> bool {
    raw.starts_with(|c: char| c.is_ascii_alphabetic())
        && raw.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// Which rdata fields hold domain names that should be made absolute.
fn name_fields(rtype: &str) -> &'static [usize] {
    match rtype {
        "NS" | "CNAME" | "PTR" | "DNAME" | "MB" | "MG" | "MR" | "NSEC" => &[0],
        "MX" | "AFSDB" | "RT" | "KX" | "LP" | "SVCB" | "HTTPS" => &[1],
        "SOA" | "RP" | "MINFO" => &[0, 1],
        "PX" => &[1, 2],
        "SRV" => &[3],
        "NAPTR" => &[5],
        "RRSIG" => &[7],
        _ => &[],
    }
}

fn rdata_field(token: &Token, is_name: bool, state: &State) -> Result<String, String> {
    if token.quoted {
        Ok(format!("\"{}\"", token.text))
    } else if is_name {
        Ok(Name::parse(&token.text, state.origin.as_ref())?.to_string())
    } else {
        Ok(token.text.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(input: &str) -> Result<Zone, Error> {
        parse_str(input, Path::new("test.zone"), None)
    }

    fn lines(zone: &Zone) -> Vec<String> {
        zone.records.iter().map(crate::format_record).collect()
    }

    #[test]
    fn origin_directive() {
        // The cases from the original nom parser.
        for input in [
            "$ORIGIN example.com\n",
            "$ORIGIN example.com.\n",
            "$ORIGIN example.com; this is a comment\n",
            "$ORIGIN example.com ; this is a comment\n",
        ] {
            let zone = parse(input).unwrap();
            assert_eq!(zone.origin.unwrap().as_str(), "example.com.", "{input}");
        }
    }

    #[test]
    fn second_origin_is_relative_to_the_first() {
        let zone = parse("$ORIGIN com.\n$ORIGIN example\nwww 60 A 192.0.2.1\n").unwrap();
        assert_eq!(zone.origin.unwrap().as_str(), "com.");
        assert_eq!(zone.records[0].name.as_str(), "www.example.com.");
    }

    #[test]
    fn ttl_and_class_in_either_order() {
        let zone =
            parse("$ORIGIN example.com.\na 60 IN A 192.0.2.1\nb IN 1h A 192.0.2.2\nc ch 5 TXT x\n")
                .unwrap();
        assert_eq!(
            lines(&zone),
            [
                "a.example.com.\t60\tIN\tA\t192.0.2.1",
                "b.example.com.\t3600\tIN\tA\t192.0.2.2",
                "c.example.com.\t5\tCH\tTXT\tx",
            ]
        );
    }

    #[test]
    fn owner_and_class_inherit() {
        let zone =
            parse("$ORIGIN example.com.\nwww 60 CH A 192.0.2.1\n\n; gap\n  AAAA ::1\n").unwrap();
        assert_eq!(zone.records[1].name.as_str(), "www.example.com.");
        assert_eq!(zone.records[1].class, "CH");
    }

    #[test]
    fn ttl_fallbacks() {
        // $TTL wins over the previous explicit TTL.
        let zone = parse("$ORIGIN x.\na 60 A 192.0.2.1\n$TTL 300\nb A 192.0.2.2\n").unwrap();
        assert_eq!(zone.records[1].ttl, 300);

        // Without $TTL, the last explicit TTL carries on.
        let zone = parse("$ORIGIN x.\na 60 A 192.0.2.1\nb A 192.0.2.2\n").unwrap();
        assert_eq!(zone.records[1].ttl, 60);

        // Without either, the SOA minimum, which then acts like $TTL.
        let zone = parse(
            "$ORIGIN x.\n@ SOA ns host 1 2 3 4 1h\nb A 192.0.2.2\nc 60 A 192.0.2.3\nd A 192.0.2.4\n",
        )
        .unwrap();
        assert_eq!(
            zone.records.iter().map(|r| r.ttl).collect::<Vec<_>>(),
            [3600, 3600, 60, 3600]
        );

        let err = parse("$ORIGIN x.\na A 192.0.2.1\n").unwrap_err();
        assert!(err.to_string().starts_with("test.zone:2: no TTL"), "{err}");
    }

    #[test]
    fn qualifies_names_in_rdata() {
        let zone = parse(
            "$ORIGIN example.com.\n$TTL 60\n@ MX 10 mail\n@ NS ns.other.\nwww CNAME @\n_sip._tcp SRV 0 5 5060 sip\n@ TXT mail\n",
        )
        .unwrap();
        assert_eq!(
            zone.records
                .iter()
                .map(|r| r.rdata.join(" "))
                .collect::<Vec<_>>(),
            [
                "10 mail.example.com.",
                "ns.other.",
                "example.com.",
                "0 5 5060 sip.example.com.",
                "mail",
            ]
        );
    }

    #[test]
    fn qualifies_names_in_newer_types() {
        let zone = parse(concat!(
            "$ORIGIN example.com.\n$TTL 60\n",
            "@ HTTPS 1 svc alpn=h2\n",
            "@ NAPTR 100 10 \"u\" \"E2U+sip\" \"\" sip\n",
            "@ RP admin info\n",
            "@ NSEC next A NSEC RRSIG\n",
            "@ RRSIG A 13 2 60 20260101000000 20250101000000 1234 @ c2ln\n",
        ))
        .unwrap();
        assert_eq!(
            zone.records
                .iter()
                .map(|r| r.rdata.join(" "))
                .collect::<Vec<_>>(),
            [
                "1 svc.example.com. alpn=h2",
                "100 10 \"u\" \"E2U+sip\" \"\" sip.example.com.",
                "admin.example.com. info.example.com.",
                "next.example.com. A NSEC RRSIG",
                "A 13 2 60 20260101000000 20250101000000 1234 example.com. c2ln",
            ]
        );
    }

    #[test]
    fn generic_types_pass_through() {
        let zone = parse("$ORIGIN x.\n$TTL 60\na CLASS1 TYPE731 \\# 2 abcd\n").unwrap();
        assert_eq!(lines(&zone), ["a.x.\t60\tCLASS1\tTYPE731\t\\# 2 abcd"]);
    }

    #[test]
    fn generic_rdata_is_not_qualified() {
        let zone = parse(
            "$ORIGIN x.\n$TTL 60\nwww CNAME \\# 3 abcdef\n@ MX \\# 4 000a0000\n@ SOA \\# 1 00\n",
        )
        .unwrap();
        assert_eq!(
            zone.records
                .iter()
                .map(|r| r.rdata.join(" "))
                .collect::<Vec<_>>(),
            ["\\# 3 abcdef", "\\# 4 000a0000", "\\# 1 00"]
        );
    }

    #[test]
    fn file_origin_beats_the_fallback() {
        let fallback = Name::parse("fallback.test.", None).unwrap();
        let zone = parse_str(
            "$ORIGIN example.com.\n",
            Path::new("test.zone"),
            Some(fallback.clone()),
        )
        .unwrap();
        assert_eq!(zone.origin.unwrap().as_str(), "example.com.");

        let zone = parse_str("", Path::new("test.zone"), Some(fallback.clone())).unwrap();
        assert_eq!(zone.origin, Some(fallback));
    }

    #[test]
    fn soa_owner_is_the_origin() {
        // How BIND writes the zone files it keeps.
        let zone = parse(concat!(
            "$ORIGIN .\n$TTL 3600\n",
            "example.com IN SOA ns1.example.com. admin.example.com. ( 1 2 3 4 5 )\n",
            "$ORIGIN example.com.\nwww A 192.0.2.1\n",
        ))
        .unwrap();
        assert_eq!(zone.origin.unwrap().as_str(), "example.com.");

        // A transfer dump, with absolute names and no $ORIGIN at all.
        let zone = parse(concat!(
            "example.com. 60 IN SOA ns.example.com. host.example.com. 1 2 3 4 5\n",
            "www.example.com. 60 IN A 192.0.2.1\n",
        ))
        .unwrap();
        assert_eq!(zone.origin.unwrap().as_str(), "example.com.");
    }

    #[test]
    fn bytes_that_are_not_utf8_become_escapes() {
        assert_eq!(decode(b"caf\xe9".to_vec()), r"caf\233");
        assert_eq!(decode(b"caf\\\xe9".to_vec()), r"caf\233");
        assert_eq!(decode(b"a\\\\\xe9".to_vec()), r"a\\\233");

        let zone = parse_reader(
            &b"$ORIGIN example.com.\n$TTL 60\nwww TXT \"caf\xe9\" ; Andr\xe9\n"[..],
            Path::new("test.zone"),
            None,
        )
        .unwrap();
        assert_eq!(zone.records[0].rdata, [r#""caf\233""#]);
    }

    #[test]
    fn errors_carry_line_numbers() {
        let cases = [
            (
                "$GENERATE 1-10 host$ A 10.0.0.$\n",
                1,
                "unsupported directive",
            ),
            ("$ORIGIN x.\n\nwww 60\n", 3, "missing record type"),
            ("$ORIGIN x.\nwww 60 A\n", 2, "A record has no data"),
            ("  A 192.0.2.1\n", 1, "no owner name"),
            ("www 60 A 192.0.2.1\n", 1, "relative name www"),
            (
                "$ORIGIN x.\n@ 60 SOA ns host 1 2 3\n",
                2,
                "SOA needs 7 fields",
            ),
            ("$TTL forever\n", 1, "invalid TTL"),
            (
                "$ORIGIN x.\nwww 4294967295 A 192.0.2.1\n",
                2,
                "invalid TTL 4294967295",
            ),
        ];
        for (input, line, message) in cases {
            match parse(input) {
                Err(Error::Parse {
                    line: l,
                    message: m,
                    ..
                }) => {
                    assert_eq!(l, line, "{input}");
                    assert!(m.contains(message), "{input}: {m}");
                }
                other => panic!("{input}: {other:?}"),
            }
        }
    }

    fn scratch_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("zoneq-{name}-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn include_does_not_leak_state() {
        let dir = scratch_dir("include");
        fs::write(
            dir.join("child.zone"),
            "$TTL 5\n$ORIGIN elsewhere.\nhost A 192.0.2.9\n",
        )
        .unwrap();
        fs::write(
            dir.join("parent.zone"),
            "$ORIGIN example.com.\n$TTL 60\n$INCLUDE child.zone\n$INCLUDE child.zone sub\nwww A 192.0.2.1\n",
        )
        .unwrap();

        let zone = parse_file(&dir.join("parent.zone"), None).unwrap();
        assert_eq!(
            lines(&zone),
            [
                "host.elsewhere.\t5\tIN\tA\t192.0.2.9",
                "host.elsewhere.\t5\tIN\tA\t192.0.2.9",
                "www.example.com.\t60\tIN\tA\t192.0.2.1",
            ]
        );
        assert_eq!(zone.origin.unwrap().as_str(), "example.com.");
    }

    #[test]
    fn include_takes_an_origin() {
        let dir = scratch_dir("include-origin");
        fs::write(dir.join("child.zone"), "@ 60 A 192.0.2.9\n").unwrap();
        fs::write(
            dir.join("parent.zone"),
            "$ORIGIN example.com.\n$INCLUDE child.zone sub\n",
        )
        .unwrap();
        let zone = parse_file(&dir.join("parent.zone"), None).unwrap();
        assert_eq!(zone.records[0].name.as_str(), "sub.example.com.");
    }

    #[test]
    fn include_loops_are_caught() {
        let dir = scratch_dir("loop");
        fs::write(dir.join("loop.zone"), "$INCLUDE loop.zone\n").unwrap();
        let err = parse_file(&dir.join("loop.zone"), None).unwrap_err();
        assert!(
            err.to_string().contains("nested more than 16 deep"),
            "{err}"
        );
    }

    #[test]
    fn missing_include_is_an_io_error() {
        let dir = scratch_dir("missing");
        fs::write(dir.join("parent.zone"), "$INCLUDE nope.zone\n").unwrap();
        let err = parse_file(&dir.join("parent.zone"), None).unwrap_err();
        assert!(
            matches!(&err, Error::Io { path, .. } if path.ends_with("nope.zone")),
            "{err}"
        );
    }
}
