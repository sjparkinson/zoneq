#![no_main]

use libfuzzer_sys::fuzz_target;
use zoneq::name::Name;

fn ancestors(name: &Name) -> Vec<Name> {
    let mut chain = vec![name.clone()];
    while let Some(parent) = chain.last().unwrap().parent() {
        chain.push(parent);
    }
    chain
}

/// Rewrites every octet outside an escape as `\DDD`, keeping the dots
/// between labels.
fn escape(name: &Name) -> String {
    if name.as_str() == "." {
        return ".".into();
    }
    let s = name.as_str();
    let bytes = s.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            // Keep a `\DDD` escape as it is.
            b'\\' if bytes[i + 1].is_ascii_digit() => {
                let digits = bytes[i + 1..]
                    .iter()
                    .take(3)
                    .take_while(|b| b.is_ascii_digit())
                    .count();
                out.push_str(&s[i..i + 1 + digits]);
                i += 1 + digits;
            }
            // `\X` is the octet X, then any continuation bytes get spelled
            // out like the rest.
            b'\\' => {
                out.push_str(&format!("\\{:03}", bytes[i + 1]));
                i += 2;
            }
            b'.' => {
                out.push('.');
                i += 1;
            }
            b => {
                out.push_str(&format!("\\{b:03}"));
                i += 1;
            }
        }
    }
    out
}

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    // Two names and an origin, one per line.
    let mut parts = text.splitn(3, '\n');
    let (Some(a), Some(b)) = (parts.next(), parts.next()) else {
        return;
    };
    let origin = parts
        .next()
        .and_then(|o| Name::parse(o, Some(&Name::root())).ok());
    let (Ok(a), Ok(b)) = (
        Name::parse(a, origin.as_ref()),
        Name::parse(b, origin.as_ref()),
    ) else {
        return;
    };

    for name in [&a, &b] {
        assert_eq!(&Name::parse(name.as_str(), None).unwrap(), name);
        let chain = ancestors(name);
        assert_eq!(chain.last().unwrap(), &Name::root(), "{name}");
        assert_eq!(chain.len(), name.labels().len() + 1, "{name}");
        for (i, ancestor) in chain.iter().enumerate() {
            assert!(
                name.is_at_or_below(ancestor),
                "{name} isn't below {ancestor}"
            );
            assert_eq!(ancestor.labels(), name.labels()[i..], "{name}");
        }
        assert_eq!(name.wildcard().parent().as_ref(), Some(name));
    }

    // Spelling every octet as `\DDD` is the same name, with the same key.
    let escaped = Name::parse(&escape(&a), None).unwrap();
    assert_eq!(escaped, a, "{escaped}");
    assert_eq!(escaped.key(), a.key(), "{escaped}");
    assert_eq!(
        escaped.is_at_or_below(&b),
        a.is_at_or_below(&b),
        "{escaped} {b}"
    );
    assert_eq!(a == b, a.key() == b.key(), "{a} {b}");

    // Being below a name is the same as having it as an ancestor.
    assert_eq!(a.is_at_or_below(&b), ancestors(&a).contains(&b), "{a} {b}");
    assert_eq!(b.is_at_or_below(&a), ancestors(&b).contains(&a), "{a} {b}");
});
