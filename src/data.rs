use std::net::IpAddr;

use crate::error::Error;
use crate::name::Name;
use crate::query::Query;
use crate::zone::{Record, name_fields};

/// What a `--data` value selects, by looking at a record's rdata instead of
/// its owner.
///
/// An IP address matches A and AAAA records holding the same address, however
/// it's written. Anything else is a name, read like the positional query
/// (so `.example.net` matches everything below it), and matches records with
/// that name in one of their name fields: an MX exchange, a CNAME target, an
/// SRV target and so on, but never a TXT string or an MX preference.
#[derive(Debug)]
pub enum DataQuery {
    Addr(IpAddr),
    Name(Query),
}

impl DataQuery {
    pub fn parse(raw: &str, origin: Option<&Name>) -> Result<DataQuery, Error> {
        match raw.parse() {
            Ok(addr) => Ok(DataQuery::Addr(addr)),
            Err(_) => Query::parse(raw, origin).map(DataQuery::Name),
        }
    }

    pub fn matches(&self, record: &Record) -> bool {
        // RFC 3597 generic rdata is opaque hex.
        if record.rdata.first().is_some_and(|f| f == r"\#") {
            return false;
        }
        match self {
            DataQuery::Addr(addr) => {
                matches!(&*record.rtype, "A" | "AAAA")
                    && record.rdata[0].parse::<IpAddr>().is_ok_and(|a| a == *addr)
            }
            // The next name in an NSEC chain and an RRSIG's signer aren't
            // things the record points at, and every signed record would
            // match the apex if they counted.
            DataQuery::Name(_) if matches!(&*record.rtype, "NSEC" | "RRSIG") => false,
            DataQuery::Name(query) => name_fields(&record.rtype).iter().any(|&i| {
                record
                    .rdata
                    .get(i)
                    .and_then(|f| Name::parse(f, None).ok())
                    .is_some_and(|name| query.matches(&name))
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    const ZONE: &str = r#"$ORIGIN example.com.
$TTL 60
@ SOA ns hostmaster 1 2 3 4 5
@ NS ns
@ MX 10 mail
@ MX 20 mail.example.net.
ns A 10.0.1.1
mail A 10.0.1.5
mail AAAA 2001:db8::5
www CNAME Mail
_sip._tcp SRV 0 5 5060 mail
note TXT "mail.example.com."
ten TXT 10
opaque A \# 4 0a000105
@ RRSIG A 8 2 60 20300101000000 20200101000000 1 example.com. c2ln
@ NSEC mail A NS SOA MX RRSIG NSEC
"#;

    fn owners(data: &str) -> Vec<String> {
        let origin = Name::parse("example.com.", None).unwrap();
        let zone = crate::zone::parse_str(ZONE, Path::new("test.zone"), None).unwrap();
        let query = DataQuery::parse(data, Some(&origin)).unwrap();
        zone.records
            .iter()
            .filter(|r| query.matches(r))
            .map(|r| format!("{} {}", r.name, r.rtype))
            .collect()
    }

    #[test]
    fn names() {
        let pointing_at_mail = [
            "example.com. MX",
            "www.example.com. CNAME",
            "_sip._tcp.example.com. SRV",
        ];
        assert_eq!(owners("mail"), pointing_at_mail);
        assert_eq!(owners("MAIL.example.com"), pointing_at_mail);
        assert_eq!(owners("mail.example.com."), pointing_at_mail);
        assert_eq!(owners("mail.example.net"), ["example.com. MX"]);
        assert_eq!(owners("ns"), ["example.com. SOA", "example.com. NS"]);
        assert_eq!(owners("hostmaster"), ["example.com. SOA"]);
    }

    #[test]
    fn subtree() {
        assert_eq!(owners(".example.net"), ["example.com. MX"]);
        assert_eq!(owners(".example.com").len(), 5);
    }

    #[test]
    fn addresses() {
        assert_eq!(owners("10.0.1.5"), ["mail.example.com. A"]);
        assert_eq!(owners("2001:0db8:0:0::5"), ["mail.example.com. AAAA"]);
        assert!(owners("10.0.1.9").is_empty());
    }

    #[test]
    fn ignores_fields_that_arent_names() {
        assert!(owners("10").is_empty());
        assert!(owners("5060").is_empty());
        assert!(owners("A").is_empty());
    }

    #[test]
    fn rejects_bad_names() {
        assert!(matches!(
            DataQuery::parse("a..b", None),
            Err(Error::Query(_))
        ));
        assert!(DataQuery::parse("", None).is_err());
    }
}
