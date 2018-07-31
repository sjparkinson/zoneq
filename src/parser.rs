use std::result;
use std::error::Error;
use pest::*;

type Result<T> = result::Result<T, Box<Error>>;

#[cfg(debug_assertions)]
const _GRAMMAR: &'static str = include_str!("zone.pest");

#[derive(Parser)]
#[grammar = "zone.pest"]
struct ZoneParser;

#[derive(Debug,PartialEq)]
pub struct Zone {
   origin: Option<String>
}

pub fn parse(input: &str) -> Result<Zone> {
    ZoneParser::parse(Rule::zone, input);

    Ok(Zone{})
}

#[cfg(test)]
mod tests {
    use parser::*;
    use pest::*;

    #[test]
    fn parse_origin() {
        parses_to! {
            parser: ZoneParser,
            input: "$ORIGIN example.com",        
            rule: Rule::origin,     
            tokens: [
                origin(0, 19, [
                    domain(8, 19)
                ])
            ]
        };

        parses_to! {
            parser: ZoneParser,
            input: "$ORIGIN example.com; this is a comment",        
            rule: Rule::origin,     
            tokens: [
                origin(0, 19, [
                    domain(8, 19)
                ])
            ]
        };

        parses_to! {
            parser: ZoneParser,
            input: "$ORIGIN example.com ; some comment",        
            rule: Rule::origin,     
            tokens: [
                origin(0, 19, [
                    domain(8, 19)
                ])
            ]
        };

        assert_eq!(Zone { origin: Some("example.com") }, parse("$ORIGIN example.com").unwrap());
    }
}