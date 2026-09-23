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

const MAIL_A_AND_AAAA: &str =
    "mail.example.com.\t86400\tIN\tA\t10.0.1.5\nmail.example.com.\t86400\tIN\tAAAA\taaaa:bbbb::5\n";

#[test]
fn type_takes_a_list() {
    zoneq()
        .args(["--type", "a,Aaaa", "mail", "example.zone"])
        .assert()
        .success()
        .stdout(MAIL_A_AND_AAAA);
}

#[test]
fn type_can_repeat() {
    zoneq()
        .args(["--type", "A", "--type", "aaaa", "mail", "example.zone"])
        .assert()
        .success()
        .stdout(MAIL_A_AND_AAAA);
    zoneq()
        .args(["--type", "mx,ns", "--type", "cname", ".", "example.zone"])
        .assert()
        .success()
        .stdout(predicate::function(|out: &str| {
            out.lines().count() == 6
                && out.lines().all(|l| {
                    ["\tMX\t", "\tNS\t", "\tCNAME\t"]
                        .iter()
                        .any(|t| l.contains(t))
                })
        }));
}

#[test]
fn empty_type_exits_2() {
    for types in ["a,,mx", "a,", ""] {
        zoneq()
            .args(["--type", types, ".", "example.zone"])
            .assert()
            .code(2)
            .stderr(predicate::str::contains("empty record type"));
    }
}

#[test]
fn data_finds_what_points_at_a_name() {
    zoneq()
        .args(["--data", "services", ".", "example.zone"])
        .assert()
        .success()
        .stdout(concat!(
            "ftp.example.com.\t86400\tIN\tCNAME\tservices.example.com.\n",
            "www.example.com.\t86400\tIN\tCNAME\tservices.example.com.\n",
        ));
    zoneq()
        .args([
            "--data",
            ".example.com",
            "--type",
            "mx",
            "@",
            "example.zone",
        ])
        .assert()
        .success()
        .stdout(predicate::function(|out: &str| out.lines().count() == 2));
}

#[test]
fn data_finds_what_points_at_an_address() {
    zoneq()
        .args(["--data", "aaaa:bbbb:0:0::5", ".", "example.zone"])
        .assert()
        .success()
        .stdout("mail.example.com.\t86400\tIN\tAAAA\taaaa:bbbb::5\n");
}

#[test]
fn data_skips_fields_that_arent_names() {
    zoneq()
        .args(["--data", "10", ".", "example.zone"])
        .assert()
        .code(1);
}

#[test]
fn resolve_follows_cnames() {
    zoneq()
        .args(["--resolve", "--type", "a", "www", "example.zone"])
        .assert()
        .success()
        .stdout(concat!(
            "www.example.com.\t86400\tIN\tCNAME\tservices.example.com.\n",
            "services.example.com.\t86400\tIN\tA\t10.0.1.10\n",
            "services.example.com.\t86400\tIN\tA\t10.0.1.11\n",
        ));
}

#[test]
fn resolve_expands_wildcards() {
    zoneq()
        .args([
            "--resolve",
            "--type",
            "aaaa",
            "bob.users",
            "tests/samples/resolve.zone",
        ])
        .assert()
        .success()
        .stdout(concat!(
            "bob.users.example.net.\t3600\tIN\tCNAME\twww.example.net.\n",
            "www.example.net.\t3600\tIN\tAAAA\t2001:db8::10\n",
        ));
}

#[test]
fn resolve_refers_delegations() {
    zoneq()
        .args([
            "--resolve",
            "--type",
            "mx",
            "host.lab",
            "tests/samples/resolve.zone",
        ])
        .assert()
        .success()
        .stdout(concat!(
            "lab.example.net.\t3600\tIN\tNS\tns.lab.example.net.\n",
            "lab.example.net.\t3600\tIN\tNS\tns.example.org.\n",
            "ns.lab.example.net.\t3600\tIN\tA\t192.0.2.53\n",
        ));
}

#[test]
fn resolve_nxdomain_and_nodata_exit_1() {
    // staff.users is an empty non-terminal, so it exists with no data and
    // hides the wildcard from the names below it.
    for query in ["bob.staff.users", "staff.users", "nope"] {
        zoneq()
            .args(["--resolve", query, "tests/samples/resolve.zone"])
            .assert()
            .code(1)
            .stdout("")
            .stderr("");
    }
    zoneq()
        .args([
            "--resolve",
            "--json",
            "--type",
            "mx",
            "www",
            "tests/samples/resolve.zone",
        ])
        .assert()
        .code(1)
        .stdout("[]\n");
}

#[test]
fn resolve_needs_one_name_inside_the_zone() {
    for (query, message) in [
        (".example.net", "not a subtree"),
        (".", "not a subtree"),
        ("www.example.org.", "outside the zone example.net."),
    ] {
        zoneq()
            .args(["--resolve", query, "tests/samples/resolve.zone"])
            .assert()
            .code(2)
            .stderr(predicate::str::contains(message));
    }
}

#[test]
fn resolve_and_data_conflict() {
    zoneq()
        .args(["--resolve", "--data", "services", "www", "example.zone"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("cannot be used with"));
}
