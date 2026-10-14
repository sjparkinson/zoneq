//! What a record's data means, as opposed to how it's spelt, so two
//! records can be told apart or found to be the same record.

use std::net::IpAddr;

use crate::name::{Name, unescape};
use crate::zone::{Record, name_fields, type_name};

/// How a type writes its fields, beyond names and text.
struct Shape {
    /// Unsigned numbers, where `010` is `10`.
    numbers: &'static [usize],
    /// Fields whose case doesn't matter, like hex or a CAA tag.
    caseless: &'static [usize],
    /// Fields that name a record type, so `A` is `TYPE1`.
    types: &'static [usize],
    /// Where the fields run together into one value, and what kind.
    tail: Option<(usize, Tail)>,
}

#[derive(Clone, Copy)]
enum Tail {
    /// Hex, which can be split anywhere and written in either case.
    Hex,
    /// Base64, which can be split anywhere.
    Base64,
    /// A set of record types, in any order or case.
    Types,
}

fn shape(rtype: &str) -> Shape {
    let shape = |numbers, caseless, types, tail| Shape {
        numbers,
        caseless,
        types,
        tail,
    };
    match rtype {
        "MX" | "AFSDB" | "RT" | "KX" | "PX" | "LP" | "SVCB" | "HTTPS" => {
            shape(&[0], &[], &[], None)
        }
        "SRV" => shape(&[0, 1, 2], &[], &[], None),
        "SOA" => shape(&[2, 3, 4, 5, 6], &[], &[], None),
        "NAPTR" => shape(&[0, 1], &[], &[], None),
        "CAA" => shape(&[0], &[1], &[], None),
        "DS" | "CDS" | "DLV" | "TA" | "TLSA" | "SMIMEA" | "ZONEMD" => {
            shape(&[0, 1, 2], &[], &[], Some((3, Tail::Hex)))
        }
        "SSHFP" => shape(&[0, 1], &[], &[], Some((2, Tail::Hex))),
        "DNSKEY" | "CDNSKEY" | "KEY" => shape(&[0, 1, 2], &[], &[], Some((3, Tail::Base64))),
        "RRSIG" | "SIG" => shape(&[1, 2, 3, 6], &[], &[0], Some((8, Tail::Base64))),
        "OPENPGPKEY" | "DHCID" => shape(&[], &[], &[], Some((0, Tail::Base64))),
        "NSEC" => shape(&[], &[], &[], Some((1, Tail::Types))),
        "NSEC3" => shape(&[0, 1, 2], &[3, 4], &[], Some((5, Tail::Types))),
        "NSEC3PARAM" => shape(&[0, 1, 2], &[3], &[], None),
        "CSYNC" => shape(&[0, 1], &[], &[], Some((2, Tail::Types))),
        _ => shape(&[], &[], &[], None),
    }
}

/// A key for a record's data that's the same for two records exactly when
/// their data means the same, however it's spelt: names compared as names,
/// addresses as addresses, numbers as numbers, hex in either case, and text
/// by its octets once quotes and escapes are undone.
pub(crate) fn key(record: &Record) -> Vec<Vec<u8>> {
    let (rtype, rdata) = (&*record.rtype, &record.rdata);
    // RFC 3597 hex, however it's split or cased.
    if rdata.first().is_some_and(|f| f == r"\#") {
        let mut hex = rdata.get(2..).unwrap_or_default().concat().into_bytes();
        hex.make_ascii_lowercase();
        return vec![br"\#".to_vec(), hex];
    }

    let shape = shape(rtype);
    let names = name_fields(rtype);
    let end = shape.tail.map_or(rdata.len(), |(i, _)| i.min(rdata.len()));
    let mut key: Vec<Vec<u8>> = rdata[..end]
        .iter()
        .enumerate()
        .map(|(i, f)| field(rtype, &shape, names.contains(&i), i, f))
        .collect();

    if let Some((_, tail)) = shape.tail {
        let rest = &rdata[end..];
        key.push(match tail {
            Tail::Hex => rest.concat().to_ascii_lowercase().into_bytes(),
            Tail::Base64 => rest.concat().into_bytes(),
            Tail::Types => {
                let mut types: Vec<_> = rest.iter().map(|t| type_name(t)).collect();
                types.sort();
                types.dedup();
                types.join(" ").into_bytes()
            }
        });
    }
    key
}

fn field(rtype: &str, shape: &Shape, is_name: bool, i: usize, f: &str) -> Vec<u8> {
    if is_name && let Ok(name) = Name::parse(f, None) {
        return name.key();
    }
    if i == 0
        && matches!(rtype, "A" | "AAAA")
        && let Ok(addr) = f.parse::<IpAddr>()
    {
        return addr.to_string().into_bytes();
    }
    let raw = f
        .strip_prefix('"')
        .and_then(|f| f.strip_suffix('"'))
        .unwrap_or(f);
    if shape.numbers.contains(&i) && !raw.is_empty() && raw.bytes().all(|b| b.is_ascii_digit()) {
        let digits = raw.trim_start_matches('0');
        return if digits.is_empty() { "0" } else { digits }.into();
    }
    if shape.types.contains(&i) {
        return type_name(raw).as_bytes().to_vec();
    }
    let mut octets = unescape(raw);
    if shape.caseless.contains(&i) {
        octets.make_ascii_lowercase();
    }
    octets
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::zone::parse_str;

    /// Whether two records, written as zone file lines, have the same key.
    fn same(a: &str, b: &str) -> bool {
        let text = format!("$ORIGIN x.\n$TTL 60\n{a}\n{b}\n");
        let zone = parse_str(&text, Path::new("test.zone"), None).unwrap();
        let [a, b] = &zone.records[..] else {
            panic!("{text}");
        };
        a.rtype == b.rtype && key(a) == key(b)
    }

    #[test]
    fn spellings_of_the_same_data() {
        for (a, b) in [
            ("a TXT t", r#"a TXT "t""#),
            (r#"a TXT "\116""#, "a TXT t"),
            ("a AAAA 2001:db8::1", "a AAAA 2001:0DB8:0::1"),
            ("a A 192.0.2.1", r"a TYPE1 \# 4 c0000201"),
            ("a CNAME Host", "a CNAME host.x."),
            ("a MX 10 mail", "a MX 010 MAIL"),
            (r"a TYPE731 \# 2 abcd", r"a TYPE731 \# 2 ( AB cd )"),
            ("a DS 1 8 2 ABCD", "a DS 1 8 2 ( ab cd )"),
            ("a CAA 0 issue \"ca\"", "a CAA 0 ISSUE \"ca\""),
            ("a NSEC b A RRSIG", "a NSEC b rrsig TYPE1"),
            (
                "a RRSIG A 8 2 60 20300101000000 20200101000000 1 x. c2ln bmF0",
                "a RRSIG TYPE1 8 2 60 20300101000000 20200101000000 1 x. c2lnbmF0",
            ),
        ] {
            assert!(same(a, b), "{a} and {b}");
        }
    }

    #[test]
    fn data_that_differs() {
        for (a, b) in [
            // Text is case-sensitive, and digits in it aren't numbers.
            ("a TXT t", "a TXT T"),
            ("a TXT 010", "a TXT 10"),
            ("a TXT \"a b\"", "a TXT a b"),
            ("a MX 10 mail", "a MX 20 mail"),
            // Base64 is case-sensitive.
            ("a DNSKEY 256 3 8 AwEAAQ==", "a DNSKEY 256 3 8 aweaaq=="),
        ] {
            assert!(!same(a, b), "{a} and {b}");
        }
    }
}
