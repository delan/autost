use cssparser::{
    BasicParseError, BasicParseErrorKind, ParseError, Parser, ParserInput, ToCss, Token,
};
use tracing::warn;

#[derive(Debug)]
pub enum InlineStyleToken {
    Url(String),
    String(String),
    Other(String),
}

#[must_use]
pub fn parse_inline_style(style: &str) -> Vec<InlineStyleToken> {
    let mut input = ParserInput::new(style);
    let mut parser = Parser::new(&mut input);

    parse(&mut parser)
}

#[must_use]
pub fn serialise_inline_style(tokens: &[InlineStyleToken]) -> String {
    tokens
        .iter()
        .map(|t| match t {
            InlineStyleToken::Url(url) => format!("url({})", serialise_string_value(url)),
            InlineStyleToken::String(string) => serialise_string_value(string),
            InlineStyleToken::Other(other) => other.to_owned(),
        })
        .collect::<String>()
}

fn parse(parser: &mut Parser) -> Vec<InlineStyleToken> {
    let mut result = vec![];
    loop {
        let token = match parser.next_including_whitespace_and_comments() {
            Ok(token) => token.clone(),
            Err(ref error @ BasicParseError { ref kind, .. }) => match kind {
                BasicParseErrorKind::UnexpectedToken(token) => {
                    warn!(?error, "css parse error");
                    token.clone()
                }
                BasicParseErrorKind::EndOfInput => break,
                other => {
                    warn!(error = ?other, "css parse error");
                    continue;
                }
            },
        };
        let function_name = match &token {
            Token::Function(name) => Some(&**name),
            _ => None,
        };
        if function_name == Some("url") {
            assert!(matches!(token, Token::Function(..)));
            let nested_result = parser
                .parse_nested_block(|p| Ok::<_, ParseError<()>>(parse(p)))
                .expect("guaranteed by closure");
            for token in nested_result {
                match token {
                    InlineStyleToken::String(value) => {
                        result.push(InlineStyleToken::Url(value));
                    }
                    other => {
                        warn!(token = ?other, "unexpected token in css url()");
                    }
                }
            }
        } else {
            match &token {
                Token::UnquotedUrl(url) => result.push(InlineStyleToken::Url((**url).to_owned())),
                Token::QuotedString(value) => {
                    result.push(InlineStyleToken::String((**value).to_owned()));
                }
                other => result.push(InlineStyleToken::Other(other.to_css_string())),
            }
            if matches!(
                token,
                Token::Function(..)
                    | Token::ParenthesisBlock
                    | Token::CurlyBracketBlock
                    | Token::SquareBracketBlock
            ) {
                let nested_result = parser
                    .parse_nested_block(|p| Ok::<_, ParseError<()>>(parse(p)))
                    .expect("guaranteed by closure");
                result.extend(nested_result);
            }
            match &token {
                Token::Function(..) => result.push(InlineStyleToken::Other(")".to_owned())),
                Token::ParenthesisBlock => result.push(InlineStyleToken::Other(")".to_owned())),
                Token::SquareBracketBlock => result.push(InlineStyleToken::Other("]".to_owned())),
                Token::CurlyBracketBlock => result.push(InlineStyleToken::Other("}".to_owned())),
                _ => {}
            }
        }
    }

    result
}

fn serialise_string_value(string: &str) -> String {
    // newlines are not allowed in <string-token>, but if we just backslash
    // escape the newline, the parser consumes it without appending anything to
    // the string value. instead, we need to escape it in hex.
    // <https://drafts.csswg.org/css-syntax-3/#consume-string-token>
    // <https://drafts.csswg.org/css-syntax-3/#consume-escaped-code-point>
    format!(
        "'{}'",
        string
            .replace('\\', r"\\")
            .replace('\'', r"\'")
            .replace('\n', r"\A ")
    )
}

#[test]
fn test_round_trip_inline_style() {
    let original_style = r#"background:rgb(1 2 3);background:rgb(var(--color-cherry));background:url(http://x/y\'z);background:url('http://x/y\'z');background:url("http://x/y\"z");"#;
    let expected = r#"background:rgb(1 2 3);background:rgb(var(--color-cherry));background:url('http://x/y\'z');background:url('http://x/y\'z');background:url('http://x/y"z');"#;
    let tokens = parse_inline_style(original_style);
    assert_eq!(serialise_inline_style(&tokens), expected);
}

#[test]
fn test_serialise_string_value() {
    assert_eq!(serialise_string_value(r"http://test"), r"'http://test'");
}
