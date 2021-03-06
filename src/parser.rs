use nom::*;
use nom::types::CompleteStr;
use std::result;
use std::error::Error;

type Result<T> = result::Result<T, Box<Error>>;

#[derive(Debug)]
struct Zone<'a> {
    origin: &'a str,
}

pub fn parse(input: &[u8]) -> Result<()> {
    Ok(())
}

fn is_end_of_text(chr: char) -> bool {
  chr == ';' || chr == '\n' || chr == ' '
}

named!(origin<CompleteStr, CompleteStr>,
    do_parse!(
                tag!("$ORIGIN")                         >>
                space                                   >>
        origin: take_till!(is_end_of_text)              >>
                opt!(tag!("."))                         >>
                opt!(space)                             >>
                opt!(pair!(tag!(";"), not_line_ending)) >>
                line_ending                             >>
        (origin)
    )
);

#[test]
fn parse_origin() {
    assert_eq!(
        Ok((CompleteStr(""), CompleteStr("example.com"))),
        origin(CompleteStr("$ORIGIN example.com\n"))
    );

    assert_eq!(
        Ok((CompleteStr(""), CompleteStr("example.com"))),
        origin(CompleteStr("$ORIGIN example.com.\n"))
    );

    assert_eq!(
        Ok((CompleteStr(""), CompleteStr("example.com"))),
        origin(CompleteStr("$ORIGIN example.com; this is a comment\n"))
    );

    assert_eq!(
        Ok((CompleteStr(""), CompleteStr("example.com"))),
        origin(CompleteStr("$ORIGIN example.com ; this is a comment\n"))
    );
}
