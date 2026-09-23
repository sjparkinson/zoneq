#![no_main]

use std::path::Path;

use libfuzzer_sys::fuzz_target;
use zoneq::zone::{self, Zone};

fn lines(zone: &Zone) -> Vec<String> {
    zone.records.iter().map(zoneq::format_record).collect()
}

fuzz_target!(|data: &[u8]| {
    // $INCLUDE reads from disk, and `$INCLUDE /dev/zero` never finishes.
    // Escapes are kept raw, so only these exact letters make the directive.
    if data.windows(8).any(|w| w.eq_ignore_ascii_case(b"$INCLUDE")) {
        return;
    }

    let Ok(zone) = zone::parse_reader(data, Path::new("fuzz.zone"), None) else {
        return;
    };

    // Printed records are absolute and spell out their TTL and class, so
    // they should parse back to exactly the same lines.
    let printed = lines(&zone);
    let text = printed.join("\n");
    let reparsed = zone::parse_str(&text, Path::new("printed.zone"), None)
        .unwrap_or_else(|e| panic!("printed zone doesn't parse: {e}\n{text}"));
    assert_eq!(printed, lines(&reparsed));
});
