use std::result::Result;
use pest::*;

#[cfg(debug_assertions)]
const _GRAMMAR: &'static str = include_str!("zone.pest");

#[derive(Parser)]
#[grammar = "zone.pest"]
struct ZoneParser;

#[derive(Debug,PartialEq)]
pub struct Zone {
   origin: Option<String>
}

pub fn parse(input: &String) -> Result<Zone, String> {
    let mut pairs = match ZoneParser::parse(Rule::zone, input.as_str()) {
        Ok(p) => p,
        Err(e) => return Err(format!("{}", e)) // todo: return a better error message
    };

    let pair = pairs.next().unwrap();

    println!("{}", pair);

    Ok(consume(pair))
}

fn consume(pair: Pair<Rule>) -> Zone {
    fn value(pair: Pair<Rule>) -> Zone {
        let pair = pair.into_inner().next().unwrap();

        match pair.as_rule() {
            Rule::origin => {
                
            },
            _ => unreachable!()
        }
    }

    value(pair)
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
                origin(0, 20, [
                    domain(8, 20)
                ])
            ]
        };

        parses_to! {
            parser: ZoneParser,
            input: "$ORIGIN example.com; this is a comment",        
            rule: Rule::origin,     
            tokens: [
                origin(0, 20, [
                    domain(8, 20)
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

        assert_eq!(Zone { origin: Some("example.com".to_string()) }, parse("$ORIGIN example.com").unwrap());
    }
}