//! Holds the type table in `src/zone.rs` to IANA's registry of record
//! types, a copy of which is in `tests/iana`. CONTRIBUTING.md says how to
//! refresh it.

use std::collections::BTreeMap;

use zoneq::zone::{same_type, type_name};

const REGISTRY: &str = include_str!("iana/dns-parameters-4.csv");

/// The types in the registry that can't be in a zone, because they only
/// mean something in a query or a message.
const META: &[(u16, &str)] = &[
    (41, "OPT"),
    (128, "NXNAME"),
    (249, "TKEY"),
    (250, "TSIG"),
    (251, "IXFR"),
    (252, "AXFR"),
    (253, "MAILB"),
    (254, "MAILA"),
    (255, "*"),
];

/// The first two fields of each row, which are a type's name and number.
/// Quoted fields further along hold commas and line breaks, so rows can't
/// be split on either without keeping track of the quotes. A doubled quote
/// toggles twice and is lost, which still leaves every field ending in the
/// right place, and that's all this needs.
fn rows(csv: &str) -> Vec<[String; 2]> {
    let mut rows = Vec::new();
    let mut row = [String::new(), String::new()];
    let (mut field, mut quoted) = (0, false);
    for c in csv.chars() {
        match c {
            '"' => quoted = !quoted,
            ',' if !quoted => field += 1,
            '\n' if !quoted => {
                rows.push(std::mem::take(&mut row));
                field = 0;
            }
            // IANA serves the file with CRLF line endings.
            '\r' if !quoted => {}
            _ => {
                if let Some(f) = row.get_mut(field) {
                    f.push(c);
                }
            }
        }
    }
    assert!(!quoted, "the registry ends inside a quoted field");
    if row.iter().any(|f| !f.is_empty()) {
        rows.push(row);
    }
    rows
}

#[test]
fn type_table_matches_the_registry() {
    let mut rows = rows(REGISTRY).into_iter();
    assert_eq!(
        rows.next(),
        Some(["TYPE".to_string(), "Value".to_string()]),
        "the registry's columns have moved"
    );

    let mut registry = BTreeMap::new();
    for [name, value] in rows {
        // Numbers nobody has, which are ranges apart from the two ends.
        if matches!(&*name, "Unassigned" | "Private use" | "Reserved") {
            continue;
        }
        let number: u16 = value
            .parse()
            .unwrap_or_else(|_| panic!("{name} has the value {value:?}, which isn't one number"));
        assert_eq!(
            registry.insert(number, name),
            None,
            "TYPE{number} is in the registry twice"
        );
    }

    // What's left once the meta types are out is what a zone can hold. They
    // have to be in the registry as listed, or the list above has gone stale.
    for &(number, name) in META {
        assert_eq!(
            registry.remove(&number).as_deref(),
            Some(name),
            "TYPE{number} isn't {name} in the registry"
        );
    }

    // Anything not left in the registry has to be without a name in the
    // table, the meta types included. Every disagreement is collected, so
    // one run after a refresh says everything the table needs.
    let mut wrong = Vec::new();
    for number in 0..=u16::MAX {
        let generic = format!("TYPE{number}");
        let found = type_name(&generic);
        let table = (found != generic).then_some(&*found);
        let registry = registry.get(&number).map(String::as_str);
        if table != registry {
            wrong.push(format!("{generic}: table {table:?}, registry {registry:?}"));
        // `--type` goes the other way, from the name to the number.
        } else if let Some(name) = registry
            && !same_type(name, &generic)
        {
            wrong.push(format!("{name} isn't the same type as {generic}"));
        }
    }
    assert!(
        wrong.is_empty(),
        "the type table and IANA's registry disagree:\n{}",
        wrong.join("\n")
    );
}
