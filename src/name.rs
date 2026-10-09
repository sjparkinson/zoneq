use std::fmt;

/// The most octets in a label, from RFC 1035 § 2.3.4.
const MAX_LABEL: usize = 63;
/// The most octets in a name on the wire, length bytes and root included.
const MAX_NAME: usize = 255;

enum Length {
    EmptyLabel,
    LongLabel,
    LongName,
}

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
        } else if !trailing_backslashes(raw).is_multiple_of(2) {
            // The dot added after it would be escaped, so the name would
            // never end.
            return Err(format!("invalid name {raw}: it ends in a lone backslash"));
        } else if ends_with_unescaped_dot(raw) {
            Name(raw.to_string())
        } else {
            match origin {
                Some(o) => {
                    let suffix = if o.is_root() { "" } else { o.as_str() };
                    let mut name = String::with_capacity(raw.len() + 1 + suffix.len());
                    name.push_str(raw);
                    name.push('.');
                    name.push_str(suffix);
                    Name(name)
                }
                None => return Err(format!("relative name {raw} with no origin set")),
            }
        };

        match name.check_lengths() {
            Ok(()) => Ok(name),
            Err(Length::EmptyLabel) => Err(format!("invalid name {raw}")),
            // The name itself could be huge, so it's left out.
            Err(Length::LongLabel) => Err(format!("a label is longer than {MAX_LABEL} octets")),
            Err(Length::LongName) => Err(format!("a name is longer than {MAX_NAME} octets")),
        }
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
        if ancestor.is_root() {
            return true;
        }
        let (ours, theirs) = (self.0.as_bytes(), ancestor.0.as_bytes());
        // The match has to start a label: at the very beginning, or just
        // after a dot that isn't escaped.
        if let Some(split) = ours.len().checked_sub(theirs.len())
            && ours[split..].eq_ignore_ascii_case(theirs)
            && (split == 0 || ends_with_unescaped_dot(&self.0[..split]))
        {
            return true;
        }
        // Spelt differently, they can still be the same octets.
        if !self.has_escapes() && !ancestor.has_escapes() {
            return false;
        }
        let (ours, theirs) = (self.labels(), ancestor.labels());
        let Some(split) = ours.len().checked_sub(theirs.len()) else {
            return false;
        };
        ours[split..]
            .iter()
            .zip(&theirs)
            .all(|(a, b)| octets(a) == octets(b))
    }

    /// The name with its leftmost label removed, or `None` for the root.
    pub fn parent(&self) -> Option<Name> {
        if self.is_root() {
            return None;
        }
        let bytes = self.0.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'\\' => i += 2,
                b'.' => {
                    let rest = &self.0[i + 1..];
                    return Some(if rest.is_empty() {
                        Name::root()
                    } else {
                        Name(rest.to_string())
                    });
                }
                _ => i += 1,
            }
        }
        None
    }

    /// The wildcard directly below this name, `*.<name>`.
    pub fn wildcard(&self) -> Name {
        if self.is_root() {
            Name("*.".to_string())
        } else {
            Name(format!("*.{}", self.0))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }

    /// Checks every label has 1 to 63 octets and the whole name fits in
    /// 255, as RFC 1035 § 2.3.4 asks, counting each escape as the one octet
    /// it stands for. Beyond the spec, it keeps the work per name small:
    /// resolving walks every ancestor of a name, which is quadratic in its
    /// length.
    fn check_lengths(&self) -> Result<(), Length> {
        if self.is_root() {
            return Ok(());
        }
        let bytes = &self.0.as_bytes()[..self.0.len() - 1];
        // The root's zero-length label.
        let mut total = 1;
        let mut label = 0;
        let mut i = 0;
        while i <= bytes.len() {
            match bytes.get(i) {
                Some(b'.') | None => {
                    if label == 0 {
                        return Err(Length::EmptyLabel);
                    }
                    if label > MAX_LABEL {
                        return Err(Length::LongLabel);
                    }
                    total += 1 + label;
                    if total > MAX_NAME {
                        return Err(Length::LongName);
                    }
                    label = 0;
                    i += 1;
                    continue;
                }
                // Mirrors `octets`: three digits up to 255 are one octet.
                Some(b'\\') => {
                    let step = if decimal_escape(&bytes[i..]) { 4 } else { 2 };
                    i = (i + step).min(bytes.len());
                }
                Some(_) => i += 1,
            }
            label += 1;
        }
        Ok(())
    }

    fn is_root(&self) -> bool {
        self.0 == "."
    }

    /// The name lowercased, with escapes decoded except for a dot or
    /// backslash inside a label. Two names are the same exactly when their
    /// keys are, and a name with no escapes is its own key.
    pub fn key(&self) -> Vec<u8> {
        let mut key = self.0.to_ascii_lowercase().into_bytes();
        if !key.contains(&b'\\') {
            return key;
        }
        key.clear();
        for label in self.labels() {
            for byte in octets(label) {
                if matches!(byte, b'.' | b'\\') {
                    key.push(b'\\');
                }
                key.push(byte);
            }
            key.push(b'.');
        }
        key
    }

    fn has_escapes(&self) -> bool {
        self.0.contains('\\')
    }
}

/// A label's octets with its escapes decoded and ASCII lowercased, so
/// `\087`, `\w` and `W` all come out as `w`.
fn octets(label: &str) -> Vec<u8> {
    let bytes = label.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let byte = match bytes[i..] {
            [b'\\', a @ b'0'..=b'9', b @ b'0'..=b'9', c @ b'0'..=b'9', ..]
                if let Ok(n) = u8::try_from(
                    u16::from(a - b'0') * 100 + u16::from(b - b'0') * 10 + u16::from(c - b'0'),
                ) =>
            {
                i += 4;
                n
            }
            [b'\\', escaped, ..] => {
                i += 2;
                escaped
            }
            [byte, ..] => {
                i += 1;
                byte
            }
            [] => break,
        };
        out.push(byte.to_ascii_lowercase());
    }
    out
}

/// Whether `bytes` starts with a `\DDD` escape for one octet, 255 or less.
fn decimal_escape(bytes: &[u8]) -> bool {
    matches!(
        bytes,
        [b'\\', a @ b'0'..=b'9', b @ b'0'..=b'9', c @ b'0'..=b'9', ..]
            if u16::from(a - b'0') * 100 + u16::from(b - b'0') * 10 + u16::from(c - b'0') <= 255
    )
}

/// True when `raw` is an absolute name, ending in a dot that isn't escaped.
pub(crate) fn ends_with_unescaped_dot(raw: &str) -> bool {
    let Some(rest) = raw.strip_suffix('.') else {
        return false;
    };
    trailing_backslashes(rest).is_multiple_of(2)
}

fn trailing_backslashes(raw: &str) -> usize {
    raw.bytes().rev().take_while(|&b| b == b'\\').count()
}

/// Names are the same when their octets are, ignoring ASCII case, however
/// they're escaped.
impl PartialEq for Name {
    fn eq(&self, other: &Name) -> bool {
        self.0.eq_ignore_ascii_case(&other.0)
            || (self.has_escapes() || other.has_escapes()) && self.key() == other.key()
    }
}

impl Eq for Name {}

impl fmt::Display for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
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
        assert!(Name::parse("..", None).is_err());
        assert!(Name::parse(r"a\..b.", None).is_ok());
    }

    #[test]
    fn rejects_names_over_the_length_limits() {
        let label = "a".repeat(63);
        assert!(Name::parse(&format!("{label}."), None).is_ok());
        assert!(Name::parse(&format!("{label}a."), None).is_err());

        // Three 63-octet labels and a 61-octet one, with their length bytes
        // and the root's, make exactly 255.
        let longest = format!("{label}.{label}.{label}.{}.", "a".repeat(61));
        assert!(Name::parse(&longest, None).is_ok());
        assert!(Name::parse(&format!("a{longest}"), None).is_err());
        // A relative name can go over once the origin is added.
        let origin = name(&longest[64..]);
        assert!(Name::parse(&label, Some(&origin)).is_ok());
        assert!(Name::parse(&format!("a.{label}"), Some(&origin)).is_err());

        // Escapes count as the octets they stand for.
        assert!(Name::parse(&format!("{}.", r"\097".repeat(63)), None).is_ok());
        assert!(Name::parse(&format!("{}.", r"\.".repeat(63)), None).is_ok());
        assert!(Name::parse(&format!("{}.", r"\.".repeat(64)), None).is_err());
        // Past 255 it's the digits themselves, three octets.
        assert!(Name::parse(&format!("{}.", r"\256".repeat(21)), None).is_ok());
        assert!(Name::parse(&format!("{}.", r"\256".repeat(22)), None).is_err());

        // A huge name fails without being echoed back.
        let err = Name::parse(&"a.".repeat(100_000), None).unwrap_err();
        assert_eq!(err, "a name is longer than 255 octets");
    }

    #[test]
    fn rejects_a_dangling_escape() {
        assert!(Name::parse(r"a\", Some(&Name::root())).is_err());
        assert!(Name::parse(r"a\\\", Some(&Name::root())).is_err());
        assert_eq!(
            Name::parse(r"a\\", Some(&Name::root())).unwrap().as_str(),
            r"a\\."
        );
    }

    #[test]
    fn equality_ignores_case() {
        assert_eq!(name("WWW.Example.COM."), name("www.example.com."));
    }

    #[test]
    fn escapes_are_the_octets_they_stand_for() {
        assert_eq!(name(r"\119ww.example."), name("WWW.example."));
        assert_eq!(name(r"\w\W\087.example."), name("www.example."));
        assert_eq!(name(r"m\046x.example."), name(r"m\.x.example."));
        assert_ne!(name(r"m\046x.example."), name("m.x.example."));
        assert!(name(r"a.\101xample.").is_at_or_below(&name("example.")));
        assert!(!name(r"a\046example.").is_at_or_below(&name("example.")));
        // Past 255 it's the digits themselves.
        assert_eq!(name(r"\256.example."), name("256.example."));

        assert_eq!(name(r"\119ww.example.").key(), b"www.example.");
        assert_eq!(name(r"M\046\\x.").key(), br"m\.\\x.");
    }

    #[test]
    fn subtree_membership() {
        let zone = name("example.com.");
        assert!(name("example.com.").is_at_or_below(&zone));
        assert!(name("a.b.EXAMPLE.com.").is_at_or_below(&zone));
        assert!(!name("badexample.com.").is_at_or_below(&zone));
        assert!(!name("com.").is_at_or_below(&zone));
        assert!(name("com.").is_at_or_below(&Name::root()));
        assert!(!name(r"a\.example.com.").is_at_or_below(&zone));
        assert!(name(r"a\\.example.com.").is_at_or_below(&zone));
        assert!(!name("example.com.").is_at_or_below(&name("a.example.com.")));
    }

    #[test]
    fn parents() {
        assert_eq!(
            name("www.example.com.").parent(),
            Some(name("example.com."))
        );
        assert_eq!(name("com.").parent(), Some(Name::root()));
        assert_eq!(Name::root().parent(), None);
        assert_eq!(name(r"a\.b.example.").parent(), Some(name("example.")));
        assert_eq!(name(r"a\\.b.").parent(), Some(name("b.")));
    }

    #[test]
    fn wildcards() {
        assert_eq!(name("example.com.").wildcard().as_str(), "*.example.com.");
        assert_eq!(Name::root().wildcard().as_str(), "*.");
        assert_eq!(
            name("*.example.com.").wildcard().labels(),
            ["*", "*", "example", "com"]
        );
    }
}
