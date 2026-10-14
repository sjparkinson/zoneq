use std::borrow::Cow;
use std::fmt::{self, Write as _};
use std::fs;
use std::io::Read;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::path::Path;

use crate::error::Error;
use crate::lexer::{self, Line, Token};
use crate::name::Name;
use crate::ttl::{parse_seconds, parse_ttl};

const MAX_INCLUDE_DEPTH: usize = 16;

const CLASSES: &[&str] = &["IN", "CH", "HS", "CS"];

/// The classes RFC 3597's `CLASS<n>` can stand for.
const CLASS_NUMBERS: &[(u16, &str)] = &[(1, "IN"), (2, "CS"), (3, "CH"), (4, "HS")];

/// Types common enough to be worth not allocating, most common first.
const TYPES: &[&str] = &[
    "A",
    "AAAA",
    "CNAME",
    "TXT",
    "MX",
    "NS",
    "PTR",
    "SRV",
    "SOA",
    "CAA",
    "HTTPS",
    "SVCB",
    "DS",
    "DNSKEY",
    "RRSIG",
    "NSEC",
    "NSEC3",
    "NSEC3PARAM",
    "TLSA",
    "SSHFP",
    "NAPTR",
    "DNAME",
    "SPF",
    "HINFO",
    "LOC",
    "CDS",
    "CDNSKEY",
];

/// The types RFC 3597's `TYPE<n>` can stand for: everything in [`TYPES`],
/// plus the rarer ones [`name_fields`] knows about.
const TYPE_NUMBERS: &[(u16, &str)] = &[
    (1, "A"),
    (2, "NS"),
    (5, "CNAME"),
    (6, "SOA"),
    (7, "MB"),
    (8, "MG"),
    (9, "MR"),
    (12, "PTR"),
    (13, "HINFO"),
    (14, "MINFO"),
    (15, "MX"),
    (16, "TXT"),
    (17, "RP"),
    (18, "AFSDB"),
    (21, "RT"),
    (26, "PX"),
    (28, "AAAA"),
    (29, "LOC"),
    (33, "SRV"),
    (35, "NAPTR"),
    (36, "KX"),
    (39, "DNAME"),
    (43, "DS"),
    (44, "SSHFP"),
    (46, "RRSIG"),
    (47, "NSEC"),
    (48, "DNSKEY"),
    (50, "NSEC3"),
    (51, "NSEC3PARAM"),
    (52, "TLSA"),
    (59, "CDS"),
    (60, "CDNSKEY"),
    (64, "SVCB"),
    (65, "HTTPS"),
    (99, "SPF"),
    (107, "LP"),
    (257, "CAA"),
];

#[derive(Clone, Debug)]
pub struct Record {
    pub name: Name,
    pub ttl: u32,
    pub class: Cow<'static, str>,
    pub rtype: Cow<'static, str>,
    pub rdata: Vec<String>,
}

/// A tab-separated zone file line, with the rdata fields space-separated.
impl fmt::Display for Record {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // An owner starting with `$` would read back as a directive, and
        // one starting with a byte order mark would lose it on line one.
        if self.name.as_str().starts_with(['$', '\u{feff}']) {
            f.write_str("\\")?;
        }
        write_escaped(f, self.name.as_str())?;
        write!(f, "\t{}\t{}\t{}\t", self.ttl, self.class, self.rtype)?;
        for (i, field) in self.rdata.iter().enumerate() {
            if i > 0 {
                f.write_str(" ")?;
            }
            write_escaped(f, field)?;
        }
        Ok(())
    }
}

/// Writes `text` with control characters spelt as `\DDD` escapes. A zone
/// file can hold raw escape sequences and carriage returns, and printing
/// them would let it drive the terminal or paint over other output. The
/// escapes mean the same octets, so the text still reads back the same.
pub(crate) fn write_escaped(f: &mut impl fmt::Write, text: &str) -> fmt::Result {
    // Every control character starts with one of these bytes, and checking
    // bytes is quicker than decoding characters for the usual case of none.
    if !text.bytes().any(|b| b < 0x20 || b == 0x7f || b == 0xc2) {
        return f.write_str(text);
    }
    // Whether the previous character was an unescaped backslash, so the
    // control character is already escaped and only needs its digits.
    let mut escaping = false;
    for c in text.chars() {
        if c.is_control() {
            let mut buf = [0; 4];
            for (i, byte) in c.encode_utf8(&mut buf).bytes().enumerate() {
                if i > 0 || !escaping {
                    f.write_char('\\')?;
                }
                write!(f, "{byte:03}")?;
            }
            escaping = false;
        } else {
            f.write_char(c)?;
            escaping = c == '\\' && !escaping;
        }
    }
    Ok(())
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
    last_class: Option<Cow<'static, str>>,
    /// Index into `Parser::records`, so inheriting an owner is the only
    /// time it gets cloned.
    last_owner: Option<usize>,
    depth: usize,
}

struct Parser {
    records: Vec<Record>,
    origin: Option<Name>,
}

impl Parser {
    fn parse(&mut self, input: &str, source: &Path, state: &mut State) -> Result<(), Error> {
        for line in lexer::Lines::new(input) {
            let line = &line.map_err(|e| Error::parse(source, e.line, e.message))?;
            let first = &line.tokens[0];
            if !line.leading_blank && !first.quoted && first.text.starts_with('$') {
                self.directive(line, source, state)?;
            } else {
                let record = record(line, state, &self.records)
                    .map_err(|message| Error::parse(source, line.number, message))?;
                state.last_owner = Some(self.records.len());
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
                let origin = directive_origin(name, state).map_err(fail)?;
                if state.depth == 0 && self.origin.is_none() {
                    self.origin = Some(origin.clone());
                }
                state.origin = Some(origin);
            }
            "$TTL" => {
                let [ttl] = args else {
                    return Err(fail("$TTL takes one value".into()));
                };
                let ttl =
                    parse_ttl(ttl.text).ok_or_else(|| fail(format!("invalid TTL {}", ttl.text)))?;
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
                    child.origin = Some(directive_origin(origin, state).map_err(fail)?);
                }

                let base = source.parent().unwrap_or(Path::new(""));
                let path = base.join(file.text);
                // Devices and pipes might never end, like `/dev/zero`, or
                // never start. A top-level FILE can still be one, since
                // whoever runs zoneq picked it.
                let meta = fs::metadata(&path).map_err(|e| Error::io(&path, e))?;
                if !meta.is_file() {
                    return Err(fail(format!(
                        "$INCLUDE {} isn't a regular file",
                        path.display()
                    )));
                }
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
fn directive_origin(token: &Token, state: &State) -> Result<Name, String> {
    let root = Name::root();
    parse_name(token, Some(state.origin.as_ref().unwrap_or(&root)))
}

/// Quotes are for text. A quoted name could hold spaces, and then it
/// wouldn't print back out as a single token.
fn parse_name(token: &Token, origin: Option<&Name>) -> Result<Name, String> {
    if token.quoted {
        return Err(format!("expected a name, found \"{}\"", token.text));
    }
    Name::parse(token.text, origin)
}

fn record(line: &Line, state: &mut State, records: &[Record]) -> Result<Record, String> {
    let tokens = &line.tokens;
    let mut idx = 0;

    let owner = if line.leading_blank {
        state
            .last_owner
            .map(|i| records[i].name.clone())
            .ok_or("record has no owner name and there's no previous one to inherit")?
    } else {
        idx += 1;
        parse_name(&tokens[0], state.origin.as_ref())?
    };

    let mut ttl = None;
    let mut class = None;
    while let Some(token) = tokens.get(idx) {
        if ttl.is_none()
            && let Some(t) = parse_ttl(token.text)
        {
            ttl = Some(t);
        } else if class.is_none()
            && let Some(c) = parse_class(token.text)
        {
            class = Some(c);
        } else {
            break;
        }
        idx += 1;
    }

    let rtype = tokens.get(idx).ok_or("missing record type")?;
    // Types start with a letter, so this is a TTL that didn't parse, such
    // as one over 2^32 - 1.
    if rtype.text.starts_with(|c: char| c.is_ascii_digit()) {
        return Err(format!("invalid TTL {}", rtype.text));
    }
    if !is_type_like(rtype.text) {
        return Err(format!("expected a record type, found {}", rtype.text));
    }
    let rtype = type_name(rtype.text);
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
        let minimum = parse_ttl(fields[6].text)
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
        .unwrap_or(Cow::Borrowed("IN"));

    if ttl.is_some() {
        state.last_ttl = ttl;
    }
    let ttl = ttl
        .or(state.default_ttl)
        .or(state.last_ttl)
        .ok_or("no TTL given and no $TTL, previous TTL or SOA to fall back on")?;

    let rdata = match generic.then(|| from_generic(&rtype, fields)).flatten() {
        Some(rdata) => rdata,
        None => {
            let name_fields = if generic { &[] } else { name_fields(&rtype) };
            let mut rdata = fields
                .iter()
                .enumerate()
                .map(|(i, token)| rdata_field(token, name_fields.contains(&i), state))
                .collect::<Result<Vec<_>, _>>()?;
            if rtype == "SOA" && !generic {
                soa_timers_in_seconds(&mut rdata)?;
            }
            rdata
        }
    };

    Ok(Record {
        name: owner,
        ttl,
        class,
        rtype,
        rdata,
    })
}

fn parse_class(raw: &str) -> Option<Cow<'static, str>> {
    if CLASSES.iter().any(|c| c.eq_ignore_ascii_case(raw)) {
        return Some(known_upper(raw, CLASSES));
    }
    let number = generic_number(raw, "CLASS")?;
    Some(
        match CLASS_NUMBERS.iter().find(|(n, _)| Some(*n) == number) {
            Some((_, name)) => Cow::Borrowed(name),
            None => Cow::Owned(raw.to_ascii_uppercase()),
        },
    )
}

/// A record type in upper case, with RFC 3597's `TYPE<n>` swapped for the
/// type's name when it has one, so `TYPE1` is `A`.
pub fn type_name(raw: &str) -> Cow<'static, str> {
    let number = generic_number(raw, "TYPE").flatten();
    match TYPE_NUMBERS.iter().find(|(n, _)| Some(*n) == number) {
        Some((_, name)) => known_upper(name, TYPES),
        None => known_upper(raw, TYPES),
    }
}

/// For `TYPE<n>` or `CLASS<n>`, `Some` with the number if it fits in 16
/// bits, or `Some(None)` if it doesn't. `None` if `raw` isn't of that form.
fn generic_number(raw: &str, prefix: &str) -> Option<Option<u16>> {
    let digits = raw
        .get(..prefix.len())
        .filter(|p| p.eq_ignore_ascii_case(prefix))
        .map(|_| &raw[prefix.len()..])
        .filter(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()))?;
    Some(digits.parse().ok())
}

/// Rewrites an SOA's refresh, retry, expire and minimum in seconds, so
/// `1d` comes out as `86400`.
fn soa_timers_in_seconds(rdata: &mut [String]) -> Result<(), String> {
    for (field, what) in rdata[3..]
        .iter_mut()
        .zip(["refresh", "retry", "expire", "minimum"])
    {
        let seconds = parse_seconds(field).ok_or_else(|| format!("invalid SOA {what} {field}"))?;
        *field = seconds.to_string();
    }
    Ok(())
}

/// RFC 3597 generic rdata for a type that has a usual form, rewritten in
/// that form, so `A \# 4 c0000201` is `A 192.0.2.1`. Only types whose
/// form is easy to get right are rewritten, and anything that doesn't
/// decode cleanly is left as it was.
fn from_generic(rtype: &str, fields: &[Token]) -> Option<Vec<String>> {
    let [_, len, hex @ ..] = fields else {
        return None;
    };
    if hex.iter().any(|t| t.quoted) {
        return None;
    }
    let hex: String = hex.iter().map(|t| t.text).collect();
    let bytes = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok())
        .collect::<Option<Vec<u8>>>()?;
    if len.text.parse() != Ok(bytes.len()) {
        return None;
    }
    let rdata = match (rtype, bytes.as_slice()) {
        ("A", &[a, b, c, d]) => vec![Ipv4Addr::new(a, b, c, d).to_string()],
        ("AAAA", bytes) => vec![Ipv6Addr::from(<[u8; 16]>::try_from(bytes).ok()?).to_string()],
        ("NS" | "CNAME" | "PTR" | "DNAME", bytes) => vec![wire_name(bytes)?],
        ("MX", [hi, lo, name @ ..]) => {
            vec![u16::from_be_bytes([*hi, *lo]).to_string(), wire_name(name)?]
        }
        _ => return None,
    };
    Some(rdata)
}

/// An uncompressed name in wire format, which must fill `bytes` exactly,
/// spelt as an absolute name with escapes where the octets need them.
fn wire_name(bytes: &[u8]) -> Option<String> {
    let mut name = String::new();
    let mut i = 0;
    loop {
        let len = usize::from(*bytes.get(i)?);
        i += 1;
        if len == 0 {
            break;
        }
        // Over 63 is a compression pointer or an unused label type.
        if len > 63 {
            return None;
        }
        for &b in bytes.get(i..i + len)? {
            match b {
                b'.' | b'\\' | b'"' | b'(' | b')' | b';' => {
                    name.push('\\');
                    name.push(char::from(b));
                }
                b'!'..=b'~' => name.push(char::from(b)),
                _ => {
                    let _ = write!(name, "\\{b:03}");
                }
            }
        }
        name.push('.');
        i += len;
    }
    if i != bytes.len() {
        return None;
    }
    if name.is_empty() {
        name.push('.');
    }
    Name::parse(&name, None).ok().map(Name::into_string)
}

/// `raw` in upper case, borrowed from `table` when it's listed there.
fn known_upper(raw: &str, table: &[&'static str]) -> Cow<'static, str> {
    match table.iter().find(|t| t.eq_ignore_ascii_case(raw)) {
        Some(t) => Cow::Borrowed(t),
        None => Cow::Owned(raw.to_ascii_uppercase()),
    }
}

fn is_type_like(raw: &str) -> bool {
    raw.starts_with(|c: char| c.is_ascii_alphabetic())
        && raw.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// Which rdata fields hold domain names that should be made absolute.
pub(crate) fn name_fields(rtype: &str) -> &'static [usize] {
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
    if is_name {
        Ok(parse_name(token, state.origin.as_ref())?.into_string())
    } else if token.quoted {
        Ok(format!("\"{}\"", token.text))
    } else {
        Ok(token.text.to_string())
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
    fn owner_starting_with_a_dollar_is_escaped() {
        let zone = parse("$ORIGIN $x.\n@ 60 A 192.0.2.1\n").unwrap();
        assert_eq!(lines(&zone), ["\\$x.\t60\tIN\tA\t192.0.2.1"]);
    }

    #[test]
    fn owner_starting_with_a_byte_order_mark_is_escaped() {
        let zone = parse("$ORIGIN \u{feff}x.\n@ 60 A 192.0.2.1\n").unwrap();
        let printed = lines(&zone);
        assert_eq!(printed, ["\\\u{feff}x.\t60\tIN\tA\t192.0.2.1"]);
        let reparsed = parse(&printed.join("\n")).unwrap();
        assert_eq!(lines(&reparsed), printed);
    }

    #[test]
    fn control_characters_print_as_escapes() {
        let zone = parse(concat!(
            "$ORIGIN example.com.\n$TTL 60\n",
            "a\x1bb TXT \"x\ry\x1b]0;hi\x07\" \"\ttab\"\n",
            "c TXT \"\\\x1b \\\\\x1b \u{9b}\\\u{9b}\"\n",
        ))
        .unwrap();
        let printed = lines(&zone);
        assert_eq!(
            printed,
            [
                "a\\027b.example.com.\t60\tIN\tTXT\t\"x\\013y\\027]0;hi\\007\" \"\\009tab\"",
                "c.example.com.\t60\tIN\tTXT\t\"\\027 \\\\\\027 \\194\\155\\194\\155\"",
            ]
        );
        // The same records come back, spelt with escapes.
        let reparsed = parse(&printed.join("\n")).unwrap();
        assert_eq!(lines(&reparsed), printed);
        assert_eq!(reparsed.records[0].name, zone.records[0].name);
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
        let zone = parse("$ORIGIN x.\n$TTL 60\na CLASS731 TYPE731 \\# 2 abcd\n").unwrap();
        assert_eq!(lines(&zone), ["a.x.\t60\tCLASS731\tTYPE731\t\\# 2 abcd"]);
    }

    #[test]
    fn generic_names_for_known_types_and_classes() {
        let zone = parse(
            "$ORIGIN x.\n$TTL 60\n\
             a CLASS1 TYPE1 192.0.2.1\n\
             c TYPE6 \\# 1 00\n\
             d TYPE70000 \\# 0\n\
             b CLASS3 type16 \"t\"\n",
        )
        .unwrap();
        assert_eq!(
            lines(&zone),
            [
                "a.x.\t60\tIN\tA\t192.0.2.1",
                // There's no usual form to rewrite an SOA in from hex.
                "c.x.\t60\tIN\tSOA\t\\# 1 00",
                // Past 16 bits it's no type at all, so it's left alone.
                "d.x.\t60\tIN\tTYPE70000\t\\# 0",
                "b.x.\t60\tCH\tTXT\t\"t\"",
            ]
        );
    }

    #[test]
    fn generic_rdata_for_known_types_is_decoded() {
        let zone = parse(
            "$ORIGIN x.\n$TTL 60\n\
             a TYPE1 \\# 4 c0000201\n\
             b A \\# 4 ( c000 0202 )\n\
             c AAAA \\# 16 20010db8000000000000000000000001\n\
             d NS \\# 7 026e7302657800\n\
             e TYPE5 \\# 1 00\n\
             f MX \\# 8 000a046d61696c00\n\
             g PTR \\# 6 04612e283b00\n\
             h PTR \\# 4 02010200\n",
        )
        .unwrap();
        assert_eq!(
            lines(&zone),
            [
                "a.x.\t60\tIN\tA\t192.0.2.1",
                "b.x.\t60\tIN\tA\t192.0.2.2",
                "c.x.\t60\tIN\tAAAA\t2001:db8::1",
                "d.x.\t60\tIN\tNS\tns.ex.",
                "e.x.\t60\tIN\tCNAME\t.",
                "f.x.\t60\tIN\tMX\t10 mail.",
                // Octets that mean something in a zone file are escaped.
                "g.x.\t60\tIN\tPTR\ta\\.\\(\\;.",
                "h.x.\t60\tIN\tPTR\t\\001\\002.",
            ]
        );
        // And the output reads back as the same records.
        let text = lines(&zone).join("\n");
        assert_eq!(lines(&parse(&text).unwrap()), lines(&zone));
    }

    #[test]
    fn malformed_generic_rdata_is_kept() {
        for (rtype, rdata) in [
            // Too short, too long, a length that doesn't match, and bad hex.
            ("A", "\\# 3 c00002"),
            ("A", "\\# 5 c000020100"),
            ("A", "\\# 4 c00002"),
            ("A", "\\# 4 c00002zz"),
            // A compression pointer, a name without its root, and octets
            // after the root.
            ("NS", "\\# 2 c00c"),
            ("NS", "\\# 3 026e73"),
            ("NS", "\\# 2 0000"),
        ] {
            let zone = parse(&format!("$ORIGIN x.\n$TTL 60\na {rtype} {rdata}\n")).unwrap();
            assert_eq!(
                lines(&zone),
                [format!("a.x.\t60\tIN\t{rtype}\t{rdata}")],
                "{rdata}"
            );
        }
    }

    #[test]
    fn soa_timers_print_in_seconds() {
        let zone = parse("$ORIGIN x.\n@ 60 SOA ns host 2024010101 1d 2h 4w 1H30M\n").unwrap();
        assert_eq!(
            lines(&zone),
            ["x.\t60\tIN\tSOA\tns.x. host.x. 2024010101 86400 7200 2419200 5400"]
        );
    }

    #[test]
    fn ttls_past_2_31_are_zero() {
        let zone = parse("$ORIGIN x.\n$TTL 4294967295\na A 192.0.2.1\nb 2147483648 A 192.0.2.2\n")
            .unwrap();
        assert_eq!(
            lines(&zone),
            ["a.x.\t0\tIN\tA\t192.0.2.1", "b.x.\t0\tIN\tA\t192.0.2.2"]
        );
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
            // The first error in the file wins, even when a later one is
            // the lexer's.
            (
                "$ORIGIN x.\nwww 60 A\nb TXT \"open\n",
                2,
                "A record has no data",
            ),
            ("  A 192.0.2.1\n", 1, "no owner name"),
            ("www 60 A 192.0.2.1\n", 1, "relative name www"),
            ("\"a b.\" 60 A 192.0.2.1\n", 1, "expected a name"),
            ("$ORIGIN \"a b.\"\n", 1, "expected a name"),
            ("$ORIGIN x.\n@ 60 CNAME \"a b\"\n", 2, "expected a name"),
            (
                "$ORIGIN x.\n@ 60 SOA ns host 1 2 3\n",
                2,
                "SOA needs 7 fields",
            ),
            (
                "$ORIGIN x.\n@ 60 SOA ns host 1 soon 3 4 5\n",
                2,
                "invalid SOA refresh soon",
            ),
            ("$TTL forever\n", 1, "invalid TTL"),
            (
                "$ORIGIN x.\nwww 4294967296 A 192.0.2.1\n",
                2,
                "invalid TTL 4294967296",
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

    #[cfg(unix)]
    #[test]
    fn include_only_reads_regular_files() {
        let dir = scratch_dir("special");
        fs::write(dir.join("parent.zone"), "$INCLUDE /dev/zero\n").unwrap();
        let err = parse_file(&dir.join("parent.zone"), None).unwrap_err();
        assert!(
            err.to_string()
                .ends_with("parent.zone:1: $INCLUDE /dev/zero isn't a regular file"),
            "{err}"
        );

        fs::write(dir.join("dir.zone"), "$INCLUDE .\n").unwrap();
        assert!(parse_file(&dir.join("dir.zone"), None).is_err());
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
