use std::fs;
use std::path::{Path, PathBuf};

use zoneq::Error;
use zoneq::format_record;
use zoneq::zone::{ParseOptions, Zone, parse_file};

const NO_INCLUDE: ParseOptions = ParseOptions { no_include: true };

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

#[test]
fn include_fan_out_is_capped() {
    // Shallow, with no loop, but 100 * 100 includes in all.
    let dir = scratch_dir("fan-out");
    fs::write(dir.join("c.zone"), "").unwrap();
    fs::write(dir.join("b.zone"), "$INCLUDE c.zone\n".repeat(100)).unwrap();
    fs::write(dir.join("a.zone"), "$INCLUDE b.zone\n".repeat(100)).unwrap();
    let err = parse_file(&dir.join("a.zone"), None).unwrap_err();
    assert!(
        err.to_string().contains("more than 4096 $INCLUDEs"),
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

#[test]
fn no_include_makes_include_a_parse_error() {
    let dir = scratch_dir("no-include");
    fs::write(dir.join("child.zone"), "host 60 A 192.0.2.9\n").unwrap();
    let parent = dir.join("parent.zone");
    fs::write(
        &parent,
        "$ORIGIN example.com.\nwww 60 A 192.0.2.1\n$INCLUDE child.zone\n",
    )
    .unwrap();

    // Followed unless asked not to.
    let zone = parse_file(&parent, None).unwrap();
    assert_eq!(zone.records.len(), 2);

    let err = NO_INCLUDE.parse_file(&parent, None).unwrap_err();
    assert!(matches!(&err, Error::Parse { line: 3, .. }), "{err}");
    assert!(
        err.to_string()
            .ends_with("parent.zone:3: $INCLUDE is switched off by --no-include"),
        "{err}"
    );
}

#[test]
fn no_include_never_looks_at_the_path() {
    // A missing file, a device and the wrong number of arguments would each
    // fail differently if the directive got any further.
    for directive in [
        "$INCLUDE nope.zone",
        "$include /dev/zero",
        "$Include",
        "$INCLUDE a b c",
    ] {
        let text = format!("$TTL 60\n{directive}\n");
        let err = NO_INCLUDE
            .parse_str(&text, Path::new("test.zone"), None)
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "test.zone:2: $INCLUDE is switched off by --no-include",
            "{directive}"
        );
    }
}

#[test]
fn no_include_only_minds_the_directive() {
    // Quoted, escaped or after the owner, it's text like any other.
    let text = concat!(
        "$ORIGIN example.com.\n$TTL 60\n",
        "www A 192.0.2.1\n",
        "txt TXT \"$INCLUDE /etc/passwd\"\n",
        "\\$INCLUDE TXT $INCLUDE\n",
    );
    let zone = NO_INCLUDE
        .parse_reader(text.as_bytes(), Path::new("test.zone"), None)
        .unwrap();
    assert_eq!(
        lines(&zone),
        [
            "www.example.com.\t60\tIN\tA\t192.0.2.1",
            "txt.example.com.\t60\tIN\tTXT\t\"$INCLUDE /etc/passwd\"",
            "\\$INCLUDE.example.com.\t60\tIN\tTXT\t$INCLUDE",
        ]
    );
}
