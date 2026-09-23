use assert_cmd::Command;
use predicates::prelude::*;

fn zoneq() -> Command {
    Command::cargo_bin("zoneq").unwrap()
}

#[test]
fn exact_match() {
    zoneq()
        .args(["www", "example.zone"])
        .assert()
        .success()
        .stdout("www.example.com.\t86400\tIN\tCNAME\tservices.example.com.\n");
}

#[test]
fn subtree_with_type_filter() {
    zoneq()
        .args(["--type", "aaaa", ".example.com", "example.zone"])
        .assert()
        .success()
        .stdout(predicate::function(|out: &str| {
            out.lines().count() == 6 && out.lines().all(|l| l.contains("\tAAAA\t"))
        }));
}

#[test]
fn json_output() {
    let output = zoneq()
        .args([
            "--json",
            "--type",
            "MX",
            "@",
            "tests/samples/example.com.zone",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json.as_array().unwrap().len(), 3);
    assert_eq!(
        json[2],
        serde_json::json!({
            "name": "example.com.",
            "ttl": 3600,
            "class": "IN",
            "type": "MX",
            "rdata": ["50", "mail3.example.com."],
        })
    );
}

#[test]
fn no_match_exits_1() {
    zoneq()
        .args(["nope", "example.zone"])
        .assert()
        .code(1)
        .stdout("");
    zoneq()
        .args(["--json", "nope", "example.zone"])
        .assert()
        .code(1)
        .stdout("[]\n");
}

#[test]
fn parse_error_exits_2_with_location() {
    zoneq()
        .args([".", "-"])
        .write_stdin("$ORIGIN example.com.\n$TTL 60\nwww A\n")
        .assert()
        .code(2)
        .stderr("zoneq: <stdin>:3: A record has no data\n");
}

#[test]
fn missing_file_exits_2() {
    zoneq()
        .args([".", "nope.zone"])
        .assert()
        .code(2)
        .stderr(predicate::str::starts_with("zoneq: nope.zone: "));
}

#[test]
fn reads_stdin() {
    zoneq()
        .args(["--type", "ns", ".", "-"])
        .write_stdin(std::fs::read_to_string("example.zone").unwrap())
        .assert()
        .success()
        .stdout("example.com.\t86400\tIN\tNS\tdns1.example.com.\nexample.com.\t86400\tIN\tNS\tdns2.example.com.\n");
}

#[test]
fn origin_flag() {
    zoneq()
        .args([
            "--origin",
            "0.0.127.in-addr.arpa",
            "1",
            "tests/samples/localhost-reverse.zone",
        ])
        .assert()
        .success()
        .stdout("1.0.0.127.in-addr.arpa.\t1814400\tIN\tPTR\tlocalhost.\n");
}

#[test]
fn origin_flag_is_only_a_fallback() {
    zoneq()
        .args(["--origin", "foo.test", "www", "-"])
        .write_stdin("$ORIGIN example.com.\n$TTL 60\nwww A 192.0.2.1\n")
        .assert()
        .success()
        .stdout("www.example.com.\t60\tIN\tA\t192.0.2.1\n");
}

#[test]
fn bad_origin_blames_the_flag() {
    zoneq()
        .args(["--origin", "a..b", "www", "example.zone"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("'--origin <NAME>'"));
}

#[test]
fn relative_queries_use_the_soa_owner() {
    // BIND writes its zone files like this, starting from the root.
    zoneq()
        .args(["www", "-"])
        .write_stdin(concat!(
            "$ORIGIN .\n$TTL 60\n",
            "example.com SOA ns.example.com. host.example.com. 1 2 3 4 5\n",
            "$ORIGIN example.com.\nwww A 192.0.2.1\n",
        ))
        .assert()
        .success()
        .stdout("www.example.com.\t60\tIN\tA\t192.0.2.1\n");
}

#[test]
fn usage_errors_exit_2() {
    zoneq().arg("www").assert().code(2);
}
