#![no_main]

use std::path::Path;

use libfuzzer_sys::fuzz_target;
use zoneq::resolve::{self, Outcome};
use zoneq::zone::{self, Record};

fn lines(records: &[Record]) -> Vec<String> {
    records.iter().map(zoneq::format_record).collect()
}

fuzz_target!(|data: &[u8]| {
    if data.windows(8).any(|w| w.eq_ignore_ascii_case(b"$INCLUDE")) {
        return;
    }
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    // The first line is the question, `<qname> [TYPE,TYPE]`, and the rest
    // is the zone.
    let (question, zone) = text.split_once('\n').unwrap_or((text, ""));
    let (raw, types) = question.split_once(' ').unwrap_or((question, ""));
    let types: Vec<String> = types
        .split(',')
        .filter(|t| !t.is_empty())
        .map(str::to_ascii_uppercase)
        .collect();

    let Ok(zone) = zone::parse_str(zone, Path::new("fuzz.zone"), None) else {
        return;
    };
    let Some(apex) = &zone.origin else {
        return;
    };
    let Ok(qname) = resolve::parse_qname(raw, apex) else {
        return;
    };
    assert!(qname.is_at_or_below(apex));

    let response = resolve::resolve(&zone, apex, &qname, &types);
    for r in &response.records {
        assert!(r.name.is_at_or_below(apex), "{r} is outside {apex}");
    }
    if matches!(response.outcome, Outcome::NxDomain | Outcome::Loop) {
        assert!(
            response.records.iter().all(|r| r.rtype == "CNAME"),
            "{response:?}"
        );
    }

    // Asking again by the full name gets the same answer.
    let again = resolve::parse_qname(qname.as_str(), apex).expect("qname reparses");
    assert_eq!(again, qname);
    let second = resolve::resolve(&zone, apex, &again, &types);
    assert_eq!(second.outcome, response.outcome);
    assert_eq!(lines(&second.records), lines(&response.records));
});
