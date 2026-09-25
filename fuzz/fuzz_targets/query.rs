#![no_main]

use std::path::Path;

use libfuzzer_sys::fuzz_target;
use zoneq::data::DataQuery;
use zoneq::query::Query;
use zoneq::zone;

fuzz_target!(|data: &[u8]| {
    if data.windows(8).any(|w| w.eq_ignore_ascii_case(b"$INCLUDE")) {
        return;
    }
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    // The first line is the query, then the `--data` value, then the zone.
    let mut parts = text.splitn(3, '\n');
    let (Some(raw), Some(data), Some(zone)) = (parts.next(), parts.next(), parts.next()) else {
        return;
    };
    let Ok(zone) = zone::parse_str(zone, Path::new("fuzz.zone"), None) else {
        return;
    };
    let origin = zone.origin.as_ref();

    let query = Query::parse(raw, origin).ok();
    let subtree = query
        .as_ref()
        .filter(|_| !raw.starts_with('.'))
        .map(|_| Query::parse(&format!(".{raw}"), origin).expect("subtree of a good query"));
    for r in &zone.records {
        // Asking for an owner by its full name always finds it.
        let exact = Query::parse(r.name.as_str(), origin).expect("owner parses as a query");
        assert!(exact.matches(&r.name), "{} doesn't find itself", r.name);

        if let (Some(query), Some(subtree)) = (&query, &subtree) {
            assert!(
                !query.matches(&r.name) || subtree.matches(&r.name),
                "{raw} matches {} but .{raw} doesn't",
                r.name
            );
        }
    }

    if let Ok(query) = DataQuery::parse(data, origin) {
        for r in &zone.records {
            query.matches(r);
        }
    }
});
