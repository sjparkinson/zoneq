//! Splits zone file text into logical entries of tokens.
//!
//! Handles comments, parentheses spanning lines, quoted strings and
//! backslash escapes. Escapes are kept raw, so every token is a slice of the
//! input and names and rdata print back out exactly as written.
//!
//! It works on bytes: everything with a meaning here is ASCII, and UTF-8
//! never uses ASCII bytes inside a multi-byte character, so token edges are
//! always character boundaries.

#[derive(Debug, PartialEq)]
pub struct Token<'a> {
    pub text: &'a str,
    pub quoted: bool,
}

#[derive(Debug)]
pub struct Line<'a> {
    /// The physical line the entry starts on.
    pub number: usize,
    /// True when the entry starts with whitespace, meaning it has no owner.
    pub leading_blank: bool,
    pub tokens: Vec<Token<'a>>,
}

#[derive(Debug)]
pub struct LexError {
    pub line: usize,
    pub message: String,
}

/// The entries in `input`, one at a time. Stops after the first error.
pub struct Lines<'a> {
    input: &'a str,
    pos: usize,
    line: usize,
    done: bool,
}

impl<'a> Lines<'a> {
    pub fn new(input: &'a str) -> Lines<'a> {
        // Editors on Windows like to start files with a byte order mark.
        let input = input.strip_prefix('\u{feff}').unwrap_or(input);
        Lines {
            input,
            pos: 0,
            line: 1,
            done: false,
        }
    }

    /// Reads the next entry. Always called at the start of a physical line.
    fn entry(&mut self) -> Result<Option<Line<'a>>, LexError> {
        let bytes = self.input.as_bytes();
        let mut tokens = Vec::with_capacity(8);
        let mut start = None;
        let mut depth = 0usize;
        let mut open_line = 0;
        let mut number = self.line;
        let mut leading_blank = false;
        let mut at_line_start = true;

        while let Some(&b) = bytes.get(self.pos) {
            let i = self.pos;
            self.pos += 1;

            if at_line_start {
                at_line_start = false;
                if depth == 0 {
                    number = self.line;
                    leading_blank = b == b' ' || b == b'\t';
                }
            }

            match b {
                b';' => {
                    self.flush(&mut start, i, &mut tokens);
                    self.pos = bytes[i..]
                        .iter()
                        .position(|&b| b == b'\n')
                        .map_or(bytes.len(), |n| i + n);
                }
                b'(' => {
                    self.flush(&mut start, i, &mut tokens);
                    if depth == 0 {
                        open_line = self.line;
                    }
                    depth += 1;
                }
                b')' => {
                    self.flush(&mut start, i, &mut tokens);
                    if depth == 0 {
                        return Err(err(self.line, "unexpected )"));
                    }
                    depth -= 1;
                }
                b' ' | b'\t' | b'\r' => self.flush(&mut start, i, &mut tokens),
                b'\n' => {
                    self.flush(&mut start, i, &mut tokens);
                    self.line += 1;
                    at_line_start = true;
                    if depth == 0 && !tokens.is_empty() {
                        return Ok(Some(Line {
                            number,
                            leading_blank,
                            tokens,
                        }));
                    }
                }
                b'"' => {
                    self.flush(&mut start, i, &mut tokens);
                    let text = self.quoted()?;
                    tokens.push(Token { text, quoted: true });
                }
                // A backslash can't escape a line ending, CRLF included.
                b'\\' => match bytes.get(self.pos) {
                    Some(b'\n' | b'\r') | None => return Err(err(self.line, "trailing backslash")),
                    Some(_) => {
                        start.get_or_insert(i);
                        self.pos += 1;
                    }
                },
                _ => {
                    start.get_or_insert(i);
                }
            }
        }

        if depth > 0 {
            return Err(err(open_line, "unclosed ("));
        }
        self.flush(&mut start, bytes.len(), &mut tokens);
        Ok((!tokens.is_empty()).then_some(Line {
            number,
            leading_blank,
            tokens,
        }))
    }

    /// Reads a quoted string, starting just after the opening quote.
    fn quoted(&mut self) -> Result<&'a str, LexError> {
        let bytes = self.input.as_bytes();
        let start = self.pos;
        while let Some(&b) = bytes.get(self.pos) {
            match b {
                b'"' => {
                    self.pos += 1;
                    return Ok(&self.input[start..self.pos - 1]);
                }
                b'\\' if !matches!(bytes.get(self.pos + 1), Some(b'\n') | None) => self.pos += 2,
                b'\\' | b'\n' => break,
                _ => self.pos += 1,
            }
        }
        Err(err(self.line, "unterminated quoted string"))
    }

    fn flush(&self, start: &mut Option<usize>, end: usize, tokens: &mut Vec<Token<'a>>) {
        if let Some(start) = start.take() {
            tokens.push(Token {
                text: &self.input[start..end],
                quoted: false,
            });
        }
    }
}

impl<'a> Iterator for Lines<'a> {
    type Item = Result<Line<'a>, LexError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let entry = self.entry().transpose();
        self.done = !matches!(entry, Some(Ok(_)));
        entry
    }
}

#[cfg(test)]
fn tokenise(input: &str) -> Result<Vec<Line<'_>>, LexError> {
    Lines::new(input).collect()
}

fn err(line: usize, message: &str) -> LexError {
    LexError {
        line,
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts<'a>(line: &Line<'a>) -> Vec<&'a str> {
        line.tokens.iter().map(|t| t.text).collect()
    }

    #[test]
    fn splits_on_whitespace_and_drops_comments() {
        let lines = tokenise("www  IN\tA 192.0.2.1 ; a comment\n\n; only a comment\n").unwrap();
        assert_eq!(lines.len(), 1);
        assert_eq!(texts(&lines[0]), ["www", "IN", "A", "192.0.2.1"]);
        assert!(!lines[0].leading_blank);
    }

    #[test]
    fn marks_leading_blank() {
        let lines = tokenise("  ; comment\n\tA 192.0.2.1\n").unwrap();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].leading_blank);
        assert_eq!(lines[0].number, 2);
    }

    #[test]
    fn parens_span_lines() {
        let input = "@ SOA ns host (\n 1 ; serial\n 2 3 4\n 5 )\nwww A 192.0.2.1\n";
        let lines = tokenise(input).unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(
            texts(&lines[0]),
            ["@", "SOA", "ns", "host", "1", "2", "3", "4", "5"]
        );
        assert_eq!(lines[1].number, 5);
    }

    #[test]
    fn semicolon_glued_to_token_starts_a_comment() {
        let lines = tokenise("$INCLUDE redhat.zone; this is a comment").unwrap();
        assert_eq!(texts(&lines[0]), ["$INCLUDE", "redhat.zone"]);
    }

    #[test]
    fn quoted_strings_keep_spaces_and_semicolons() {
        let lines = tokenise(r#"@ TXT "v=spf1 ~all; x" """#).unwrap();
        assert_eq!(
            lines[0].tokens[2],
            Token {
                text: "v=spf1 ~all; x",
                quoted: true
            }
        );
        assert_eq!(lines[0].tokens[3].text, "");
    }

    #[test]
    fn escapes_are_kept_raw() {
        let lines = tokenise(r#"@ SOA venera Action\.domains "say \"hi\"" a\;b"#).unwrap();
        assert_eq!(
            texts(&lines[0]),
            [
                "@",
                "SOA",
                "venera",
                r"Action\.domains",
                r#"say \"hi\""#,
                r"a\;b"
            ]
        );
    }

    #[test]
    fn reports_unbalanced_parens() {
        assert_eq!(tokenise("a (\nb\n").unwrap_err().line, 1);
        assert_eq!(tokenise("a\nb )\n").unwrap_err().line, 2);
    }

    #[test]
    fn reports_unterminated_quote_and_trailing_backslash() {
        assert!(tokenise("a \"b\n").is_err());
        assert!(tokenise("a b\\").is_err());
        assert_eq!(tokenise("a\r\nb c\\\r\n").unwrap_err().line, 2);
    }

    #[test]
    fn skips_a_byte_order_mark() {
        let lines = tokenise("\u{feff}$ORIGIN example.com.\n").unwrap();
        assert_eq!(texts(&lines[0]), ["$ORIGIN", "example.com."]);
    }
}
