use std::fmt;

/// An absolute domain name, always with a trailing dot. Escapes such as
/// `\.` are kept raw, and comparisons ignore ASCII case.
#[derive(Clone, Debug)]
pub struct Name(String);

impl Name {
    pub fn root() -> Name {
        Name(".".to_string())
    }

    /// Parses `raw` as written in a zone file. `@` is the origin, names
    /// ending in an unescaped dot are absolute, and anything else is
    /// relative to `origin`.
    pub fn parse(raw: &str, origin: Option<&Name>) -> Result<Name, String> {
        let name = if raw == "@" {
            origin
                .cloned()
                .ok_or_else(|| "@ used with no origin set".to_string())?
        } else if raw == "." {
            Name::root()
        } else if ends_with_unescaped_dot(raw) {
            Name(raw.to_string())
        } else {
            match origin {
                Some(o) if o.is_root() => Name(format!("{raw}.")),
                Some(o) => Name(format!("{raw}.{o}")),
                None => return Err(format!("relative name {raw} with no origin set")),
            }
        };

        if name.labels().iter().any(|l| l.is_empty()) {
            return Err(format!("invalid name {raw}"));
        }
        Ok(name)
    }

    /// The labels from left to right, not including the root.
    pub fn labels(&self) -> Vec<&str> {
        if self.is_root() {
            return Vec::new();
        }
        let s = &self.0[..self.0.len() - 1];
        let bytes = s.as_bytes();
        let mut labels = Vec::new();
        let mut start = 0;
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'\\' => i += 2,
                b'.' => {
                    labels.push(&s[start..i]);
                    i += 1;
                    start = i;
                }
                _ => i += 1,
            }
        }
        labels.push(&s[start..]);
        labels
    }

    pub fn is_at_or_below(&self, ancestor: &Name) -> bool {
        let ours = self.labels();
        let theirs = ancestor.labels();
        ours.len() >= theirs.len()
            && ours
                .iter()
                .rev()
                .zip(theirs.iter().rev())
                .all(|(a, b)| a.eq_ignore_ascii_case(b))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn is_root(&self) -> bool {
        self.0 == "."
    }
}

/// True when `raw` is an absolute name, ending in a dot that isn't escaped.
pub(crate) fn ends_with_unescaped_dot(raw: &str) -> bool {
    let Some(rest) = raw.strip_suffix('.') else {
        return false;
    };
    let backslashes = rest.bytes().rev().take_while(|&b| b == b'\\').count();
    backslashes % 2 == 0
}

impl PartialEq for Name {
    fn eq(&self, other: &Name) -> bool {
        self.0.eq_ignore_ascii_case(&other.0)
    }
}

impl Eq for Name {}

impl fmt::Display for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl serde::Serialize for Name {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(raw: &str) -> Name {
        Name::parse(raw, None).unwrap()
    }

    #[test]
    fn at_is_the_origin() {
        let origin = name("example.com.");
        assert_eq!(Name::parse("@", Some(&origin)).unwrap(), origin);
        assert!(Name::parse("@", None).is_err());
    }

    #[test]
    fn relative_names_take_the_origin() {
        let origin = name("example.com.");
        assert_eq!(
            Name::parse("www", Some(&origin)).unwrap().as_str(),
            "www.example.com."
        );
        assert_eq!(
            Name::parse("www", Some(&Name::root())).unwrap().as_str(),
            "www."
        );
        assert!(Name::parse("www", None).is_err());
    }

    #[test]
    fn absolute_names_ignore_the_origin() {
        let origin = name("example.com.");
        assert_eq!(
            Name::parse("ns.other.", Some(&origin)).unwrap().as_str(),
            "ns.other."
        );
        assert_eq!(Name::parse(".", Some(&origin)).unwrap(), Name::root());
    }

    #[test]
    fn escaped_dots_stay_in_the_label() {
        let origin = name("isi.edu.");
        let n = Name::parse(r"Action\.domains", Some(&origin)).unwrap();
        assert_eq!(n.labels(), [r"Action\.domains", "isi", "edu"]);
        // An escaped trailing dot isn't absolute.
        assert_eq!(
            Name::parse(r"a\.", Some(&origin)).unwrap().as_str(),
            r"a\..isi.edu."
        );
    }

    #[test]
    fn rejects_empty_labels() {
        assert!(Name::parse("a..b.", None).is_err());
    }

    #[test]
    fn equality_ignores_case() {
        assert_eq!(name("WWW.Example.COM."), name("www.example.com."));
    }

    #[test]
    fn subtree_membership() {
        let zone = name("example.com.");
        assert!(name("example.com.").is_at_or_below(&zone));
        assert!(name("a.b.EXAMPLE.com.").is_at_or_below(&zone));
        assert!(!name("badexample.com.").is_at_or_below(&zone));
        assert!(!name("com.").is_at_or_below(&zone));
        assert!(name("com.").is_at_or_below(&Name::root()));
    }
}
