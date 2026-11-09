use std::ffi::OsStr;
use std::io::Write;
use std::process::{Command, Stdio};
use std::thread;

fn zoneq() -> Zoneq {
    Zoneq {
        cmd: Command::new(env!("CARGO_BIN_EXE_zoneq")),
        stdin: String::new(),
    }
}

struct Zoneq {
    cmd: Command,
    stdin: String,
}

impl Zoneq {
    fn arg(mut self, arg: impl AsRef<OsStr>) -> Self {
        self.cmd.arg(arg);
        self
    }

    fn args(mut self, args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> Self {
        self.cmd.args(args);
        self
    }

    fn write_stdin(mut self, input: impl Into<String>) -> Self {
        self.stdin = input.into();
        self
    }

    fn assert(mut self) -> Assert {
        let mut child = self
            .cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        // On another thread, so a big input can't fill the pipe while zoneq
        // waits for us to read its output. zoneq may exit without reading
        // it all, so a write error is fine.
        let writer = thread::spawn(move || {
            let _ = stdin.write_all(self.stdin.as_bytes());
        });
        let out = child.wait_with_output().unwrap();
        writer.join().unwrap();
        Assert {
            code: out.status.code(),
            stdout: String::from_utf8(out.stdout).unwrap(),
            stderr: String::from_utf8(out.stderr).unwrap(),
        }
    }
}

struct Assert {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

impl Assert {
    #[track_caller]
    fn success(self) -> Self {
        self.code(0)
    }

    #[track_caller]
    fn code(self, code: i32) -> Self {
        assert_eq!(self.code, Some(code), "stderr: {}", self.stderr);
        self
    }

    #[track_caller]
    fn stdout(self, expected: &str) -> Self {
        assert_eq!(self.stdout, expected);
        self
    }

    #[track_caller]
    fn stderr(self, expected: &str) -> Self {
        assert_eq!(self.stderr, expected);
        self
    }

    #[track_caller]
    fn stdout_matches(self, check: impl FnOnce(&str) -> bool) -> Self {
        assert!(check(&self.stdout), "unexpected stdout: {:?}", self.stdout);
        self
    }

    #[track_caller]
    fn stderr_matches(self, check: impl FnOnce(&str) -> bool) -> Self {
        assert!(check(&self.stderr), "unexpected stderr: {:?}", self.stderr);
        self
    }
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
        .stdout_matches(|out| {
            out.lines().count() == 6 && out.lines().all(|l| l.contains("\tAAAA\t"))
        });
}

#[test]
fn json_output() {
    zoneq()
        .args([
            "--json",
            "--type",
            "MX",
            "@",
            "tests/samples/example.com.zone",
        ])
        .assert()
        .success()
        .stdout(concat!(
            "[\n",
            "  {\n",
            "    \"name\": \"example.com.\",\n",
            "    \"ttl\": 3600,\n",
            "    \"class\": \"IN\",\n",
            "    \"type\": \"MX\",\n",
            "    \"rdata\": [\n",
            "      \"10\",\n",
            "      \"mail.example.com.\"\n",
            "    ]\n",
            "  },\n",
            "  {\n",
            "    \"name\": \"example.com.\",\n",
            "    \"ttl\": 3600,\n",
            "    \"class\": \"IN\",\n",
            "    \"type\": \"MX\",\n",
            "    \"rdata\": [\n",
            "      \"20\",\n",
            "      \"mail2.example.com.\"\n",
            "    ]\n",
            "  },\n",
            "  {\n",
            "    \"name\": \"example.com.\",\n",
            "    \"ttl\": 3600,\n",
            "    \"class\": \"IN\",\n",
            "    \"type\": \"MX\",\n",
            "    \"rdata\": [\n",
            "      \"50\",\n",
            "      \"mail3.example.com.\"\n",
            "    ]\n",
            "  }\n",
            "]\n",
        ));
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
        .stderr_matches(|out| out.starts_with("zoneq: nope.zone: "));
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
        .stderr_matches(|out| out.contains("'--origin <NAME>'"));
}

#[test]
fn empty_origin_is_rejected() {
    zoneq()
        .args(["--origin=", "1", "tests/samples/localhost-reverse.zone"])
        .assert()
        .code(2)
        .stderr_matches(|out| out.contains("invalid value '' for '--origin <NAME>'"));
}

#[test]
fn type_any_means_every_type() {
    let all = zoneq()
        .args(["dns1", "example.zone"])
        .assert()
        .success()
        .stdout;
    for any in ["ANY", "any", "TYPE255", "a,any"] {
        zoneq()
            .args(["--type", any, "dns1", "example.zone"])
            .assert()
            .success()
            .stdout(all.as_str());
    }
}

#[test]
fn resolve_type_any_returns_the_cname() {
    zoneq()
        .args(["--resolve", "--type", "any", "www", "example.zone"])
        .assert()
        .success()
        .stdout("www.example.com.\t86400\tIN\tCNAME\tservices.example.com.\n");
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
        .stdout_matches(|out| {
            out.lines().count() == 6
                && out.lines().all(|l| {
                    ["\tMX\t", "\tNS\t", "\tCNAME\t"]
                        .iter()
                        .any(|t| l.contains(t))
                })
        });
}

#[test]
fn type_takes_generic_names() {
    zoneq()
        .args(["--type", "TYPE1", "mail", "example.zone"])
        .assert()
        .success()
        .stdout("mail.example.com.\t86400\tIN\tA\t10.0.1.5\n");
}

#[test]
fn empty_type_exits_2() {
    for types in ["a,,mx", "a,", ""] {
        zoneq()
            .args(["--type", types, ".", "example.zone"])
            .assert()
            .code(2)
            .stderr_matches(|out| out.contains("empty record type"));
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
        .stdout_matches(|out| out.lines().count() == 2);
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
        ))
        .stderr(REFERRAL);
}

// What --resolve says on stderr when it ends in something other than a
// plain answer.
const NXDOMAIN: &str = "zoneq: NXDOMAIN: the lookup ends at a name that doesn't exist\n";
const NODATA: &str =
    "zoneq: NODATA: the lookup ends at a name that exists, but has none of the records asked for\n";
const REFERRAL: &str =
    "zoneq: REFERRAL: the lookup ends at a delegation, so its name servers are the ones to ask\n";
const LOOP: &str = "zoneq: LOOP: the CNAME chain loops, or runs past 16 CNAMEs\n";

/// The places a CNAME can lead that resolve.zone doesn't have.
const CNAMES: &str = concat!(
    "$ORIGIN example.com.\n$TTL 60\n",
    "@ SOA ns1 hostmaster 1 7200 3600 1209600 60\n",
    "loop1 CNAME loop2\n",
    "loop2 CNAME loop1\n",
    "dangling CNAME missing\n",
    "away CNAME www.example.org.\n",
    "sub NS ns.sub\n",
    "ns.sub A 192.0.2.53\n",
    "into-sub CNAME host.sub\n",
);

#[test]
fn resolve_nxdomain_and_nodata_exit_1() {
    // staff.users is an empty non-terminal, so it exists with no data and
    // hides the wildcard from the names below it.
    for (query, note) in [
        ("bob.staff.users", NXDOMAIN),
        ("staff.users", NODATA),
        ("nope", NXDOMAIN),
    ] {
        zoneq()
            .args(["--resolve", query, "tests/samples/resolve.zone"])
            .assert()
            .code(1)
            .stdout("")
            .stderr(note);
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
        .stdout("[]\n")
        .stderr(NODATA);
}

#[test]
fn resolve_answers_leave_stderr_empty() {
    zoneq()
        .args([
            "--resolve",
            "--type",
            "a",
            "www",
            "tests/samples/resolve.zone",
        ])
        .assert()
        .success()
        .stdout("www.example.net.\t3600\tIN\tA\t192.0.2.10\n")
        .stderr("");
    // Following a CNAME to an answer is still an answer.
    zoneq()
        .args([
            "--resolve",
            "--type",
            "a",
            "bob.users",
            "tests/samples/resolve.zone",
        ])
        .assert()
        .success()
        .stderr("");
    // So is a CNAME out of the zone, since the rest is another server's job.
    zoneq()
        .args(["--resolve", "--type", "a", "away", "-"])
        .write_stdin(CNAMES)
        .assert()
        .success()
        .stdout("away.example.com.\t60\tIN\tCNAME\twww.example.org.\n")
        .stderr("");
}

#[test]
fn only_resolve_says_how_it_ended() {
    zoneq()
        .args(["nope", "tests/samples/resolve.zone"])
        .assert()
        .code(1)
        .stdout("")
        .stderr("");
}

#[test]
fn resolve_flags_cname_loops() {
    zoneq()
        .args(["--resolve", "--type", "a", "loop1", "-"])
        .write_stdin(CNAMES)
        .assert()
        .success()
        .stdout(concat!(
            "loop1.example.com.\t60\tIN\tCNAME\tloop2.example.com.\n",
            "loop2.example.com.\t60\tIN\tCNAME\tloop1.example.com.\n",
        ))
        .stderr(LOOP);
}

#[test]
fn resolve_flags_long_cname_chains() {
    // c0 is 17 CNAMEs away from the A record, and c1 is 16.
    let mut zone = String::from("$ORIGIN example.com.\n$TTL 60\n@ SOA ns1 host 1 2 3 4 5\n");
    for i in 0..17 {
        zone.push_str(&format!("c{i} CNAME c{}\n", i + 1));
    }
    zone.push_str("c17 A 192.0.2.1\n");

    zoneq()
        .args(["--resolve", "--type", "a", "c0", "-"])
        .write_stdin(zone.as_str())
        .assert()
        .success()
        .stdout_matches(|out| out.lines().count() == 16 && !out.contains("\tA\t"))
        .stderr(LOOP);
    // The 16 in the message is the most that gets followed.
    zoneq()
        .args(["--resolve", "--type", "a", "c1", "-"])
        .write_stdin(zone.as_str())
        .assert()
        .success()
        .stdout_matches(|out| out.lines().count() == 17 && out.ends_with("\tA\t192.0.2.1\n"))
        .stderr("");
}

#[test]
fn resolve_flags_cnames_to_missing_names() {
    // The CNAME is still an answer of sorts, so it exits 0 like a server
    // sending it with NXDOMAIN.
    zoneq()
        .args(["--resolve", "--type", "a", "dangling", "-"])
        .write_stdin(CNAMES)
        .assert()
        .success()
        .stdout("dangling.example.com.\t60\tIN\tCNAME\tmissing.example.com.\n")
        .stderr(NXDOMAIN);
}

#[test]
fn resolve_flags_cnames_without_the_type() {
    zoneq()
        .args([
            "--resolve",
            "--type",
            "mx",
            "bob.users",
            "tests/samples/resolve.zone",
        ])
        .assert()
        .success()
        .stdout("bob.users.example.net.\t3600\tIN\tCNAME\twww.example.net.\n")
        .stderr(NODATA);
}

#[test]
fn resolve_flags_cnames_into_delegations() {
    zoneq()
        .args(["--resolve", "--type", "a", "into-sub", "-"])
        .write_stdin(CNAMES)
        .assert()
        .success()
        .stdout(concat!(
            "into-sub.example.com.\t60\tIN\tCNAME\thost.sub.example.com.\n",
            "sub.example.com.\t60\tIN\tNS\tns.sub.example.com.\n",
            "ns.sub.example.com.\t60\tIN\tA\t192.0.2.53\n",
        ))
        .stderr(REFERRAL);
}

#[test]
fn resolve_json_keeps_the_note_out_of_stdout() {
    zoneq()
        .args(["--resolve", "--json", "--type", "a", "dangling", "-"])
        .write_stdin(CNAMES)
        .assert()
        .success()
        .stdout(concat!(
            "[\n",
            "  {\n",
            "    \"name\": \"dangling.example.com.\",\n",
            "    \"ttl\": 60,\n",
            "    \"class\": \"IN\",\n",
            "    \"type\": \"CNAME\",\n",
            "    \"rdata\": [\n",
            "      \"missing.example.com.\"\n",
            "    ]\n",
            "  }\n",
            "]\n",
        ))
        .stderr(NXDOMAIN);
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
            .stderr_matches(|out| out.contains(message));
    }
}

#[test]
fn resolve_and_data_conflict() {
    zoneq()
        .args(["--resolve", "--data", "services", "www", "example.zone"])
        .assert()
        .code(2)
        .stderr_matches(|out| out.contains("can't be used with"));
}

#[test]
fn help_and_version_exit_0() {
    for flag in ["-h", "--help"] {
        zoneq()
            .arg(flag)
            .assert()
            .success()
            .stdout_matches(|out| out.starts_with("Query zone files.\n"));
    }
    for flag in ["-V", "--version"] {
        zoneq()
            .arg(flag)
            .assert()
            .success()
            .stdout_matches(|out| out.starts_with("zoneq 0.2."));
    }
}

#[test]
fn options_take_values_after_equals() {
    zoneq()
        .args(["--type=a,aaaa", "--data=10.0.1.5", "mail", "example.zone"])
        .assert()
        .success()
        .stdout("mail.example.com.\t86400\tIN\tA\t10.0.1.5\n");
}

#[test]
fn options_can_follow_positionals() {
    zoneq()
        .args(["mail", "example.zone", "--type", "a,aaaa"])
        .assert()
        .success()
        .stdout(MAIL_A_AND_AAAA);
}

#[test]
fn double_dash_ends_options() {
    zoneq()
        .args(["--", "--json", "example.zone"])
        .assert()
        .code(1)
        .stdout("");
}

#[test]
fn bad_arguments_exit_2() {
    for (args, message) in [
        (
            &["--bogus", ".", "example.zone"][..],
            "unexpected argument '--bogus'",
        ),
        (
            &[".", "example.zone", "extra"],
            "unexpected argument 'extra'",
        ),
        (&[], "<QUERY> and <FILE> are required"),
        (&["."], "<FILE> is required"),
        (
            &[".", "example.zone", "--type"],
            "'--type <TYPE>' needs a value",
        ),
        (
            &["--json=yes", ".", "example.zone"],
            "'--json' doesn't take a value",
        ),
        (
            &["--data", "--resolve", "www", "example.zone"],
            "'--data <NAME|IP>' needs a value",
        ),
        (
            &["--json", ".", "--json", "example.zone"],
            "'--json' can only be given once",
        ),
        (
            &["--origin", "a.", "--origin", "b.", ".", "example.zone"],
            "'--origin <NAME>' can only be given once",
        ),
    ] {
        zoneq()
            .args(args)
            .assert()
            .code(2)
            .stdout("")
            .stderr_matches(|out| out.starts_with(&format!("zoneq: {message}\n")));
    }
}

#[test]
fn readme_shows_the_help() {
    let help = zoneq().arg("--help").assert().success().stdout;
    // The title, usage, arguments and options, before the longer notes.
    let summary = help.split("\n\n").take(4).collect::<Vec<_>>().join("\n\n");
    // Windows checkouts can turn the README's line endings into CRLF.
    let readme = std::fs::read_to_string("README.md")
        .unwrap()
        .replace("\r\n", "\n");
    assert!(
        readme.contains(&format!("```\n{summary}\n```")),
        "README.md's usage block should match `zoneq --help`:\n{summary}"
    );
}
