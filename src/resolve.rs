use std::collections::{HashMap, HashSet};

use crate::error::Error;
use crate::name::{Name, ends_with_unescaped_dot};
use crate::zone::{Record, Zone};

/// The most CNAMEs to follow in one answer, the same as BIND.
const MAX_CHAIN: usize = 16;

/// How an answer ended, like a response code with a bit more detail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The records asked for. A CNAME chain that leaves the zone ends here
    /// too, with only the CNAMEs, since the rest is another server's job.
    Answer,
    /// The name is at or below a delegation, so the answer is the NS
    /// records at the cut and any glue for them.
    Referral,
    /// The name exists but has none of the types asked for.
    NoData,
    /// The name doesn't exist.
    NxDomain,
    /// The CNAME chain came back on itself, or ran past [`MAX_CHAIN`].
    Loop,
}

#[derive(Debug)]
pub struct Response {
    /// The CNAMEs followed, in order, then the answer or referral.
    pub records: Vec<Record>,
    pub outcome: Outcome,
}

/// Answers `raw` the way the zone's authoritative server would, following
/// RFC 1034 § 4.3.2 and RFC 4592 for wildcards. `types` are the types asked
/// for, and empty asks for all of them, like ANY.
pub fn run(zone: &Zone, raw: &str, types: &[String]) -> Result<Response, Error> {
    let apex = zone.origin.as_ref().ok_or_else(|| {
        Error::Query("--resolve needs the zone's apex: add an SOA record or pass --origin".into())
    })?;
    let qname = parse_qname(raw, apex)?;
    Ok(resolve(zone, apex, &qname, types))
}

/// Parses a single name inside the zone. Without a trailing dot, the name
/// is taken as written if that's inside the zone, and relative to the apex
/// if not, so `www` and `www.example.com` are the same question.
pub fn parse_qname(raw: &str, apex: &Name) -> Result<Name, Error> {
    if raw.is_empty() {
        return Err(Error::Query("the query is empty".into()));
    }
    // In the root zone, `.` is the apex rather than everything.
    if raw.starts_with('.') && !(raw == "." && apex == &Name::root()) {
        return Err(Error::Query(format!(
            "--resolve takes a single name, not a subtree like {raw}"
        )));
    }

    let name = if raw == "@" || ends_with_unescaped_dot(raw) {
        Name::parse(raw, Some(apex))
    } else {
        match Name::parse(raw, Some(&Name::root())) {
            Ok(name) if name.is_at_or_below(apex) => Ok(name),
            _ => Name::parse(raw, Some(apex)),
        }
    }
    .map_err(Error::Query)?;

    if !name.is_at_or_below(apex) {
        return Err(Error::Query(format!("{name} is outside the zone {apex}")));
    }
    Ok(name)
}

pub fn resolve(zone: &Zone, apex: &Name, qname: &Name, types: &[String]) -> Response {
    let index = Index::new(zone, apex);
    let wants =
        |rtype: &str| types.is_empty() || types.iter().any(|t| t.eq_ignore_ascii_case(rtype));
    // The DS records at a cut belong to the parent, so asking for them
    // there is answered rather than referred.
    let ds_only = !types.is_empty() && types.iter().all(|t| t.eq_ignore_ascii_case("DS"));

    let mut records = Vec::new();
    let mut seen: Vec<Name> = Vec::new();
    let mut qname = qname.clone();

    let outcome = loop {
        if !qname.is_at_or_below(apex) {
            break Outcome::Answer;
        }
        if seen.contains(&qname) {
            break Outcome::Loop;
        }
        seen.push(qname.clone());

        if let Some(cut) = index.cut(&qname, ds_only) {
            records.extend(index.referral(&cut));
            break Outcome::Referral;
        }

        let (node, synthesised) = match index.owners.get(&key(&qname)) {
            Some(node) => (node, false),
            None if index.nodes.contains(&key(&qname)) => break Outcome::NoData,
            None => {
                // The wildcard to use is the one directly below the closest
                // encloser, which might be an empty non-terminal. That's
                // how a name with no records still blocks a wildcard above
                // it (RFC 4592 § 2.2.2).
                let wildcard = index.closest_encloser(&qname).wildcard();
                match index.owners.get(&key(&wildcard)) {
                    Some(node) => (node, true),
                    None if index.nodes.contains(&key(&wildcard)) => break Outcome::NoData,
                    None => break Outcome::NxDomain,
                }
            }
        };
        let owned = |r: &Record| {
            let mut r = r.clone();
            if synthesised {
                r.name = qname.clone();
            }
            r
        };

        let cname = node.iter().find(|r| r.rtype.eq_ignore_ascii_case("CNAME"));
        match cname {
            Some(cname) if !types.is_empty() && !wants("CNAME") => {
                if seen.len() > MAX_CHAIN {
                    break Outcome::Loop;
                }
                records.push(owned(cname));
                match cname.rdata.first().map(|t| Name::parse(t, None)) {
                    Some(Ok(target)) => qname = target,
                    _ => break Outcome::Answer,
                }
            }
            _ => {
                let before = records.len();
                records.extend(node.iter().filter(|r| wants(&r.rtype)).map(|r| owned(r)));
                break if records.len() > before {
                    Outcome::Answer
                } else {
                    Outcome::NoData
                };
            }
        }
    };

    Response { records, outcome }
}

/// The zone's records by owner, and every name that exists.
struct Index<'a> {
    apex: &'a Name,
    owners: HashMap<Vec<u8>, Vec<&'a Record>>,
    /// The owners plus the empty non-terminals between them and the apex,
    /// which exist even though they have no records.
    nodes: HashSet<Vec<u8>>,
}

impl<'a> Index<'a> {
    fn new(zone: &'a Zone, apex: &'a Name) -> Index<'a> {
        let mut owners: HashMap<Vec<u8>, Vec<&Record>> = HashMap::new();
        let mut nodes = HashSet::from([key(apex)]);
        // Anything outside the apex isn't ours to answer for.
        for r in zone.records.iter().filter(|r| r.name.is_at_or_below(apex)) {
            owners.entry(key(&r.name)).or_default().push(r);
            let mut name = r.name.clone();
            // Once a name is in, so is everything above it.
            while nodes.insert(key(&name)) {
                match name.parent() {
                    Some(parent) => name = parent,
                    None => break,
                }
            }
        }
        Index {
            apex,
            owners,
            nodes,
        }
    }

    fn has(&self, name: &Name, rtype: &str) -> bool {
        self.owners
            .get(&key(name))
            .is_some_and(|node| node.iter().any(|r| r.rtype.eq_ignore_ascii_case(rtype)))
    }

    /// The highest delegation at or above `qname`, if there is one. The
    /// apex's own NS records don't count.
    fn cut(&self, qname: &Name, ds_only: bool) -> Option<Name> {
        let mut below_apex = Vec::new();
        let mut name = qname.clone();
        while &name != self.apex {
            let parent = name.parent()?;
            below_apex.push(name);
            name = parent;
        }
        below_apex
            .into_iter()
            .rev()
            .find(|n| self.has(n, "NS") && !(ds_only && n == qname))
    }

    /// The NS records at `cut`, then the A and AAAA records for any of
    /// their targets inside the zone.
    fn referral(&self, cut: &Name) -> Vec<Record> {
        let ns: Vec<&Record> = self.owners[&key(cut)]
            .iter()
            .copied()
            .filter(|r| r.rtype.eq_ignore_ascii_case("NS"))
            .collect();

        let mut targets: Vec<Vec<u8>> = Vec::new();
        for r in &ns {
            if let Some(Ok(target)) = r.rdata.first().map(|t| Name::parse(t, None))
                && target.is_at_or_below(self.apex)
                && !targets.contains(&key(&target))
            {
                targets.push(key(&target));
            }
        }
        let glue = targets
            .iter()
            .filter_map(|t| self.owners.get(t))
            .flatten()
            .filter(|r| r.rtype.eq_ignore_ascii_case("A") || r.rtype.eq_ignore_ascii_case("AAAA"));

        ns.into_iter().chain(glue.copied()).cloned().collect()
    }

    /// The nearest ancestor of `qname` that exists. The apex always does.
    fn closest_encloser(&self, qname: &Name) -> Name {
        let mut name = qname.clone();
        while let Some(parent) = name.parent() {
            name = parent;
            if self.nodes.contains(&key(&name)) {
                break;
            }
        }
        name
    }
}

fn key(name: &Name) -> Vec<u8> {
    name.key()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::zone::parse_str;

    const ZONE: &str = r#"$ORIGIN example.com.
$TTL 60
@           SOA   ns1 hostmaster 1 7200 3600 1209600 60
@           NS    ns1
ns1         A     192.0.2.1
www         A     192.0.2.10
www         AAAA  2001:db8::10
alias       CNAME www
chain       CNAME alias
away        CNAME www.example.org.
loop1       CNAME loop2
loop2       CNAME loop1
*.corp      A     192.0.2.20
*.corp      TXT   "wild"
a.ent.corp  A     192.0.2.30
*.cdn       CNAME www
sub         NS    ns.sub
sub         NS    ns.other.
sub         DS    12345 8 2 ABCD
ns.sub      A     192.0.2.53
ns.sub      AAAA  2001:db8::53
into-sub    CNAME host.sub
"#;

    fn zone() -> Zone {
        parse_str(ZONE, Path::new("test.zone"), None).unwrap()
    }

    fn ask(qname: &str, types: &[&str]) -> (Vec<String>, Outcome) {
        let types: Vec<String> = types.iter().map(|t| t.to_string()).collect();
        let response = run(&zone(), qname, &types).unwrap();
        let lines = response.records.iter().map(ToString::to_string).collect();
        (lines, response.outcome)
    }

    #[test]
    fn exact_match() {
        assert_eq!(
            ask("www", &["A"]),
            (
                vec!["www.example.com.\t60\tIN\tA\t192.0.2.10".into()],
                Outcome::Answer
            )
        );
        assert_eq!(ask("www", &[]).0.len(), 2);
        assert_eq!(ask("www", &["A", "AAAA"]).0.len(), 2);
        assert_eq!(ask("WWW.Example.COM.", &["A"]).1, Outcome::Answer);
    }

    #[test]
    fn nodata_and_nxdomain() {
        assert_eq!(ask("www", &["MX"]), (vec![], Outcome::NoData));
        assert_eq!(ask("nope", &["A"]), (vec![], Outcome::NxDomain));
        assert_eq!(ask("x.www", &[]), (vec![], Outcome::NxDomain));
    }

    #[test]
    fn follows_cnames_inside_the_zone() {
        assert_eq!(
            ask("chain", &["A"]),
            (
                vec![
                    "chain.example.com.\t60\tIN\tCNAME\talias.example.com.".into(),
                    "alias.example.com.\t60\tIN\tCNAME\twww.example.com.".into(),
                    "www.example.com.\t60\tIN\tA\t192.0.2.10".into(),
                ],
                Outcome::Answer
            )
        );
    }

    #[test]
    fn cname_chain_can_end_in_nodata() {
        let (lines, outcome) = ask("alias", &["MX"]);
        assert_eq!(lines.len(), 1);
        assert_eq!(outcome, Outcome::NoData);
    }

    #[test]
    fn asking_for_the_cname_stops_there() {
        for types in [&["CNAME"][..], &["A", "CNAME"], &[]] {
            assert_eq!(
                ask("alias", types),
                (
                    vec!["alias.example.com.\t60\tIN\tCNAME\twww.example.com.".into()],
                    Outcome::Answer
                ),
                "{types:?}"
            );
        }
    }

    #[test]
    fn cnames_out_of_the_zone_stop() {
        assert_eq!(
            ask("away", &["A"]),
            (
                vec!["away.example.com.\t60\tIN\tCNAME\twww.example.org.".into()],
                Outcome::Answer
            )
        );
    }

    #[test]
    fn cname_loops_stop() {
        let (lines, outcome) = ask("loop1", &["A"]);
        assert_eq!(lines.len(), 2);
        assert_eq!(outcome, Outcome::Loop);
    }

    #[test]
    fn long_cname_chains_stop() {
        let mut text = "$ORIGIN example.com.\n$TTL 60\n@ SOA ns1 host 1 2 3 4 5\n".to_string();
        for i in 0..20 {
            text.push_str(&format!("c{i} CNAME c{}\n", i + 1));
        }
        text.push_str("c20 A 192.0.2.1\n");
        let zone = parse_str(&text, Path::new("test.zone"), None).unwrap();
        let types = ["A".to_string()];

        let long = run(&zone, "c0", &types).unwrap();
        assert_eq!(long.records.len(), MAX_CHAIN);
        assert_eq!(long.outcome, Outcome::Loop);

        // c4 is exactly MAX_CHAIN CNAMEs away from the A.
        let limit = run(&zone, "c4", &types).unwrap();
        assert_eq!(limit.records.len(), MAX_CHAIN + 1);
        assert_eq!(limit.outcome, Outcome::Answer);
    }

    #[test]
    fn wildcards_are_synthesised() {
        assert_eq!(
            ask("host.corp", &["A"]),
            (
                vec!["host.corp.example.com.\t60\tIN\tA\t192.0.2.20".into()],
                Outcome::Answer
            )
        );
        // More than one label can match the `*`.
        assert_eq!(ask("a.b.corp", &[]).0.len(), 2);
        assert_eq!(ask("host.corp", &["MX"]), (vec![], Outcome::NoData));
        // Asking for the wildcard itself is an exact match.
        assert_eq!(
            ask("*.corp", &["A"]).0,
            ["*.corp.example.com.\t60\tIN\tA\t192.0.2.20"]
        );
    }

    #[test]
    fn empty_non_terminals_block_wildcards() {
        // ent.corp has no records, but a.ent.corp does, so it exists.
        assert_eq!(ask("ent.corp", &[]), (vec![], Outcome::NoData));
        assert_eq!(ask("x.ent.corp", &[]), (vec![], Outcome::NxDomain));
        // And an existing name isn't covered by the wildcard above it.
        assert_eq!(ask("x.a.ent.corp", &[]), (vec![], Outcome::NxDomain));
    }

    #[test]
    fn wildcard_cnames_are_followed() {
        assert_eq!(
            ask("img.cdn", &["AAAA"]),
            (
                vec![
                    "img.cdn.example.com.\t60\tIN\tCNAME\twww.example.com.".into(),
                    "www.example.com.\t60\tIN\tAAAA\t2001:db8::10".into(),
                ],
                Outcome::Answer
            )
        );
    }

    #[test]
    fn delegations_refer_with_glue() {
        let referral = vec![
            "sub.example.com.\t60\tIN\tNS\tns.sub.example.com.".to_string(),
            "sub.example.com.\t60\tIN\tNS\tns.other.".into(),
            "ns.sub.example.com.\t60\tIN\tA\t192.0.2.53".into(),
            "ns.sub.example.com.\t60\tIN\tAAAA\t2001:db8::53".into(),
        ];
        assert_eq!(ask("sub", &["A"]), (referral.clone(), Outcome::Referral));
        assert_eq!(ask("sub", &[]), (referral.clone(), Outcome::Referral));
        // Everything below the cut belongs to the child, glue included.
        assert_eq!(
            ask("deep.host.sub", &["TXT"]),
            (referral.clone(), Outcome::Referral)
        );
        assert_eq!(ask("ns.sub", &["A"]), (referral.clone(), Outcome::Referral));
        assert_eq!(ask("x.ns.sub", &["DS"]), (referral, Outcome::Referral));
    }

    #[test]
    fn ds_at_the_cut_is_answered() {
        assert_eq!(
            ask("sub", &["DS"]),
            (
                vec!["sub.example.com.\t60\tIN\tDS\t12345 8 2 ABCD".into()],
                Outcome::Answer
            )
        );
        assert_eq!(ask("sub", &["DS", "A"]).1, Outcome::Referral);
    }

    #[test]
    fn cnames_into_a_delegation_refer() {
        let (lines, outcome) = ask("into-sub", &["A"]);
        assert_eq!(
            lines[0],
            "into-sub.example.com.\t60\tIN\tCNAME\thost.sub.example.com."
        );
        assert_eq!(lines.len(), 5);
        assert_eq!(outcome, Outcome::Referral);
    }

    #[test]
    fn the_apex_answers_for_itself() {
        assert_eq!(
            ask("@", &["NS"]).0,
            ["example.com.\t60\tIN\tNS\tns1.example.com."]
        );
        assert_eq!(ask("example.com", &["SOA"]).1, Outcome::Answer);
    }

    #[test]
    fn query_names() {
        let apex = Name::parse("example.com.", None).unwrap();
        let parse = |raw| parse_qname(raw, &apex).map(Name::into_string);
        assert_eq!(parse("www").unwrap(), "www.example.com.");
        assert_eq!(parse("www.example.com").unwrap(), "www.example.com.");
        assert_eq!(parse("www.example.com.").unwrap(), "www.example.com.");
        assert_eq!(parse("@").unwrap(), "example.com.");
        // Written as a relative name, this one is inside the zone.
        assert_eq!(
            parse("www.example.org").unwrap(),
            "www.example.org.example.com."
        );
        for bad in ["", ".", ".example.com", "www.example.org.", "a..b"] {
            assert!(matches!(parse(bad), Err(Error::Query(_))), "{bad}");
        }
    }

    #[test]
    fn the_root_zone_answers_for_dot() {
        let zone = parse_str(
            ". 60 SOA a.root. host. 1 2 3 4 5\n. 60 NS a.root.\n",
            Path::new("t"),
            None,
        )
        .unwrap();
        let response = run(&zone, ".", &["NS".to_string()]).unwrap();
        assert_eq!(response.outcome, Outcome::Answer);
        assert!(run(&zone, ".com", &[]).is_err());
    }

    #[test]
    fn escapes_find_the_names_they_spell() {
        let text = "$ORIGIN example.com.\n$TTL 60\n@ SOA ns1 host 1 2 3 4 5\n\\119ww A 192.0.2.1\na\\.b A 192.0.2.2\n";
        let zone = parse_str(text, Path::new("t"), None).unwrap();
        assert_eq!(run(&zone, "WWW", &[]).unwrap().outcome, Outcome::Answer);
        assert_eq!(run(&zone, r"a\046b", &[]).unwrap().outcome, Outcome::Answer);
        // The escaped dot isn't a label break, so there's no b to be below.
        assert_eq!(run(&zone, "b", &[]).unwrap().outcome, Outcome::NxDomain);
    }

    #[test]
    fn needs_an_apex() {
        let zone = parse_str("www.example.com. 60 A 192.0.2.1\n", Path::new("t"), None).unwrap();
        assert!(matches!(
            run(&zone, "www.example.com.", &[]),
            Err(Error::Query(_))
        ));
    }

    #[test]
    fn records_outside_the_zone_are_ignored() {
        let text = "$ORIGIN example.com.\n$TTL 60\n@ SOA ns1 host 1 2 3 4 5\nwww.example.org. A 192.0.2.1\n";
        let zone = parse_str(text, Path::new("t"), None).unwrap();
        assert!(run(&zone, "www.example.org.", &[]).is_err());
        assert_eq!(run(&zone, "org", &[]).unwrap().outcome, Outcome::NxDomain);
    }
}
