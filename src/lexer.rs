//! Splits zone file text into logical entries of tokens.
//!
//! Handles comments, parentheses spanning lines, quoted strings and
//! backslash escapes. Escapes are kept raw in the token text so names and
//! rdata print back out exactly as written.

#[derive(Debug, PartialEq)]
pub struct Token {
    pub text: String,
    pub quoted: bool,
}

#[derive(Debug)]
pub struct Line {
    /// The physical line the entry starts on.
    pub number: usize,
    /// True when the entry starts with whitespace, meaning it has no owner.
    pub leading_blank: bool,
    pub tokens: Vec<Token>,
}

#[derive(Debug)]
pub struct LexError {
    pub line: usize,
    pub message: String,
}

#[derive(Default)]
struct Lexer {
    lines: Vec<Line>,
    tokens: Vec<Token>,
    buf: String,
    in_token: bool,
    quoted: bool,
    entry_line: usize,
    leading_blank: bool,
}

impl Lexer {
    fn flush_token(&mut self) {
        if self.in_token {
            self.tokens.push(Token {
                text: std::mem::take(&mut self.buf),
                quoted: self.quoted,
            });
            self.in_token = false;
            self.quoted = false;
        }
    }

    fn flush_line(&mut self) {
        self.flush_token();
        if !self.tokens.is_empty() {
            self.lines.push(Line {
                number: self.entry_line,
                leading_blank: self.leading_blank,
                tokens: std::mem::take(&mut self.tokens),
            });
        }
    }
}

pub fn tokenise(input: &str) -> Result<Vec<Line>, LexError> {
    // Editors on Windows like to start files with a byte order mark.
    let input = input.strip_prefix('\u{feff}').unwrap_or(input);
    let mut lx = Lexer::default();
    let mut chars = input.chars().peekable();
    let mut line = 1;
    let mut depth = 0usize;
    let mut open_line = 0;
    let mut at_line_start = true;

    while let Some(c) = chars.next() {
        if at_line_start {
            at_line_start = false;
            if depth == 0 {
                lx.entry_line = line;
                lx.leading_blank = c == ' ' || c == '\t';
            }
        }

        if lx.quoted {
            match c {
                '"' => lx.flush_token(),
                '\\' => match chars.next() {
                    Some(next) if next != '\n' => {
                        lx.buf.push(c);
                        lx.buf.push(next);
                    }
                    _ => return Err(err(line, "unterminated quoted string")),
                },
                '\n' => return Err(err(line, "unterminated quoted string")),
                _ => lx.buf.push(c),
            }
            continue;
        }

        match c {
            ';' => {
                lx.flush_token();
                while chars.next_if(|&n| n != '\n').is_some() {}
            }
            '(' => {
                lx.flush_token();
                if depth == 0 {
                    open_line = line;
                }
                depth += 1;
            }
            ')' => {
                lx.flush_token();
                if depth == 0 {
                    return Err(err(line, "unexpected )"));
                }
                depth -= 1;
            }
            ' ' | '\t' | '\r' => lx.flush_token(),
            '\n' => {
                if depth == 0 {
                    lx.flush_line();
                } else {
                    lx.flush_token();
                }
                line += 1;
                at_line_start = true;
            }
            '"' => {
                lx.flush_token();
                lx.in_token = true;
                lx.quoted = true;
            }
            // A backslash can't escape a line ending, CRLF included.
            '\\' => match chars.next() {
                Some(next) if next != '\n' && next != '\r' => {
                    lx.in_token = true;
                    lx.buf.push(c);
                    lx.buf.push(next);
                }
                _ => return Err(err(line, "trailing backslash")),
            },
            _ => {
                lx.in_token = true;
                lx.buf.push(c);
            }
        }
    }

    if lx.quoted {
        return Err(err(line, "unterminated quoted string"));
    }
    if depth > 0 {
        return Err(err(open_line, "unclosed ("));
    }
    lx.flush_line();
    Ok(lx.lines)
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

    fn texts(line: &Line) -> Vec<&str> {
        line.tokens.iter().map(|t| t.text.as_str()).collect()
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
                text: "v=spf1 ~all; x".into(),
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
