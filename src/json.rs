use std::fmt::{self, Write as _};
use std::io::{self, Write};

use crate::zone::Record;

/// Writes records as a JSON array, indented two spaces the way
/// `serde_json::to_writer_pretty` lays it out.
pub fn write_records(out: &mut impl Write, records: &[&Record]) -> io::Result<()> {
    if records.is_empty() {
        return writeln!(out, "[]");
    }
    writeln!(out, "[")?;
    for (i, r) in records.iter().enumerate() {
        writeln!(out, "  {{")?;
        writeln!(out, "    \"name\": {},", Str(r.name.as_str()))?;
        writeln!(out, "    \"ttl\": {},", r.ttl)?;
        writeln!(out, "    \"class\": {},", Str(&r.class))?;
        writeln!(out, "    \"type\": {},", Str(&r.rtype))?;
        if r.rdata.is_empty() {
            writeln!(out, "    \"rdata\": []")?;
        } else {
            writeln!(out, "    \"rdata\": [")?;
            for (j, field) in r.rdata.iter().enumerate() {
                writeln!(out, "      {}{}", Str(field), comma(j, r.rdata.len()))?;
            }
            writeln!(out, "    ]")?;
        }
        writeln!(out, "  }}{}", comma(i, records.len()))?;
    }
    writeln!(out, "]")
}

fn comma(i: usize, len: usize) -> &'static str {
    if i + 1 < len { "," } else { "" }
}

/// A JSON string literal, with quotes, backslashes and control characters
/// escaped. JSON only needs the C0 controls escaped, but DEL and the C1
/// controls are too, so `--json` can't drive a terminal any more than the
/// text output can.
struct Str<'a>(&'a str);

impl fmt::Display for Str<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_char('"')?;
        let mut start = 0;
        for (i, c) in self.0.char_indices() {
            let escape = match c {
                '"' => "\\\"",
                '\\' => "\\\\",
                '\n' => "\\n",
                '\r' => "\\r",
                '\t' => "\\t",
                '\u{8}' => "\\b",
                '\u{c}' => "\\f",
                c if c.is_control() => "",
                _ => continue,
            };
            f.write_str(&self.0[start..i])?;
            if escape.is_empty() {
                write!(f, "\\u{:04x}", u32::from(c))?;
            } else {
                f.write_str(escape)?;
            }
            start = i + c.len_utf8();
        }
        f.write_str(&self.0[start..])?;
        f.write_char('"')
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::name::Name;

    #[test]
    fn strings_escape_what_json_needs() {
        assert_eq!(Str("").to_string(), r#""""#);
        assert_eq!(Str("é😀").to_string(), "\"é😀\"");
        assert_eq!(
            Str("\u{7f}\u{80}\u{9b}\u{a0}").to_string(),
            "\"\\u007f\\u0080\\u009b\u{a0}\""
        );
        assert_eq!(Str(r#"say "hi"\"#).to_string(), r#""say \"hi\"\\""#);
        assert_eq!(
            Str("\u{0}\u{8}\t\n\u{b}\u{c}\r\u{1f} ").to_string(),
            r#""\u0000\b\t\n\u000b\f\r\u001f ""#
        );
    }

    fn record(rdata: &[&str]) -> Record {
        Record {
            name: Name::parse("www.example.com.", None).unwrap(),
            ttl: 60,
            class: "IN".into(),
            rtype: "TXT".into(),
            rdata: rdata.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn write(records: &[&Record]) -> String {
        let mut out = Vec::new();
        write_records(&mut out, records).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn records_are_pretty_printed() {
        let (one, none) = (record(&["\"a\"", "\"b\""]), record(&[]));
        assert_eq!(write(&[]), "[]\n");
        assert_eq!(
            write(&[&one, &none]),
            concat!(
                "[\n",
                "  {\n",
                "    \"name\": \"www.example.com.\",\n",
                "    \"ttl\": 60,\n",
                "    \"class\": \"IN\",\n",
                "    \"type\": \"TXT\",\n",
                "    \"rdata\": [\n",
                "      \"\\\"a\\\"\",\n",
                "      \"\\\"b\\\"\"\n",
                "    ]\n",
                "  },\n",
                "  {\n",
                "    \"name\": \"www.example.com.\",\n",
                "    \"ttl\": 60,\n",
                "    \"class\": \"IN\",\n",
                "    \"type\": \"TXT\",\n",
                "    \"rdata\": []\n",
                "  }\n",
                "]\n",
            )
        );
    }
}
