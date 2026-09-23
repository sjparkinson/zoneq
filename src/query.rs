use crate::error::Error;
use crate::name::{Name, ends_with_unescaped_dot};

/// What a `<query>` argument selects.
///
/// `www` or `www.example.com.` matches that owner exactly, a leading dot
/// (`.example.com`) matches that name and everything below it, and `.`
/// matches everything. A name without a trailing dot is tried both as
/// absolute and relative to the zone origin, so `www.example.com` and `www`
/// both find `www.example.com.`.
#[derive(Debug)]
pub struct Query {
    names: Vec<Name>,
    subtree: bool,
}

impl Query {
    pub fn parse(raw: &str, origin: Option<&Name>) -> Result<Query, Error> {
        let (raw, subtree) = match raw.strip_prefix('.') {
            Some("") => (".", true),
            Some(rest) => (rest, true),
            None => (raw, false),
        };
        if raw.is_empty() {
            return Err(Error::Query("the query is empty".into()));
        }

        let names = if raw == "@" || ends_with_unescaped_dot(raw) {
            vec![Name::parse(raw, origin).map_err(Error::Query)?]
        } else {
            let mut names = vec![Name::parse(raw, Some(&Name::root())).map_err(Error::Query)?];
            if let Some(Ok(relative)) = origin.map(|o| Name::parse(raw, Some(o))) {
                names.push(relative);
            }
            names
        };

        Ok(Query { names, subtree })
    }

    pub fn matches(&self, name: &Name) -> bool {
        self.names.iter().any(|q| {
            if self.subtree {
                name.is_at_or_below(q)
            } else {
                name == q
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(raw: &str) -> Name {
        Name::parse(raw, None).unwrap()
    }

    fn matches(query: &str, owner: &str) -> bool {
        let origin = name("example.com.");
        Query::parse(query, Some(&origin))
            .unwrap()
            .matches(&name(owner))
    }

    #[test]
    fn exact() {
        assert!(matches("www", "www.example.com."));
        assert!(matches("WWW.example.com", "www.example.com."));
        assert!(matches("www.example.com.", "www.example.com."));
        assert!(matches("@", "example.com."));
        assert!(!matches("www", "a.www.example.com."));
        assert!(!matches("www.", "www.example.com."));
    }

    #[test]
    fn subtree() {
        assert!(matches(".example.com", "example.com."));
        assert!(matches(".example.com", "a.b.example.com."));
        assert!(matches(".www", "a.www.example.com."));
        assert!(matches(".@", "mail.example.com."));
        assert!(!matches(".example.com", "example.org."));
    }

    #[test]
    fn everything() {
        assert!(matches(".", "example.com."));
        assert!(matches(".", "anything.else."));
    }

    #[test]
    fn needs_an_origin_for_at() {
        assert!(matches!(Query::parse("@", None), Err(Error::Query(_))));
        assert!(Query::parse("www", None).unwrap().matches(&name("www.")));
    }

    #[test]
    fn rejects_bad_names() {
        assert!(Query::parse("a..b", None).is_err());
        assert!(Query::parse("", None).is_err());
    }

    #[test]
    fn escaped_trailing_dot_is_relative() {
        assert!(matches(r"a\.", r"a\..example.com."));
        assert!(Query::parse(r"a\.", None).unwrap().matches(&name(r"a\..")));
    }
}
