use std::fs;
use std::path::PathBuf;

use zoneq::Error;
use zoneq::format_record;
use zoneq::zone::{Zone, parse_file};

fn scratch_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn lines(zone: &Zone) -> Vec<String> {
    zone.records.iter().map(format_record).collect()
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
