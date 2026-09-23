use std::path::Path;

use zoneq::format_record;
use zoneq::name::Name;
use zoneq::zone::{Zone, parse_file};

fn parse(path: &str) -> Zone {
    parse_file(Path::new(path), None).unwrap_or_else(|e| panic!("{e}"))
}

fn lines(zone: &Zone) -> Vec<String> {
    zone.records.iter().map(format_record).collect()
}

#[test]
fn record_counts() {
    for (path, count) in [
        ("example.zone", 19),
        ("tests/samples/dyn.zone", 15),
        ("tests/samples/example.com.zone", 15),
        ("tests/samples/localhost.zone", 4),
        ("tests/samples/redhat.zone", 14),
        ("tests/samples/localhost-include.zone", 36),
    ] {
        assert_eq!(parse(path).records.len(), count, "{path}");
    }
}

#[test]
fn reverse_zone_needs_an_origin() {
    let path = Path::new("tests/samples/localhost-reverse.zone");
    assert!(parse_file(path, None).is_err());

    let origin = Name::parse("0.0.127.in-addr.arpa.", None).unwrap();
    let zone = parse_file(path, Some(origin)).unwrap();
    assert_eq!(
        lines(&zone),
        [
            "0.0.127.in-addr.arpa.\t1814400\tIN\tSOA\tlocalhost. root.localhost. 1999010100 3h 15m 1w 1d",
            "0.0.127.in-addr.arpa.\t1814400\tIN\tNS\tlocalhost.",
            "1.0.0.127.in-addr.arpa.\t1814400\tIN\tPTR\tlocalhost.",
        ]
    );
}

#[test]
fn relative_rdata_is_qualified() {
    let zone = parse("tests/samples/example.com.zone");
    let mx: Vec<_> = zone
        .records
        .iter()
        .filter(|r| r.rtype == "MX")
        .map(|r| r.rdata.join(" "))
        .collect();
    assert_eq!(
        mx,
        [
            "10 mail.example.com.",
            "20 mail2.example.com.",
            "50 mail3.example.com."
        ]
    );
}

#[test]
fn txt_keeps_its_quotes() {
    let zone = parse("tests/samples/dyn.zone");
    let txt = zone.records.iter().find(|r| r.rtype == "TXT").unwrap();
    assert_eq!(txt.rdata, ["\"v=spf1 includespf.dynect.net ~all\""]);
}

#[test]
fn owner_less_records_inherit_across_blank_lines() {
    let zone = parse("tests/samples/redhat.zone");
    assert!(lines(&zone).contains(&"domain.com.\t86400\tIN\tA\t10.0.1.5".to_string()));
    assert_eq!(zone.origin.unwrap().as_str(), "domain.com.");
}

#[test]
fn includes_follow_rfc_origin_rules() {
    let zone = parse("tests/samples/localhost-include.zone");
    let names: Vec<_> = zone.records.iter().map(|r| r.name.as_str()).collect();

    assert_eq!(zone.origin.unwrap().as_str(), "localhost.");
    assert!(names.contains(&"example.com."));
    // `$INCLUDE localhost-reverse.zone another-domain.com` is relative to localhost.
    assert!(names.contains(&"1.another-domain.com.localhost."));
    // redhat.zone's `$ORIGIN domain.com` has no trailing dot, so inside an
    // include it's relative to the current origin. That's what the RFC says,
    // even if it's not what the author meant.
    assert!(names.contains(&"server1.domain.com.localhost."));
}
