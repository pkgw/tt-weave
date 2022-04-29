//! A declaration of a constant.
//!
//! In Pascal these happen inside `const` blocks but in typical WEB programs
//! it's easiest to treat them as toplevels.

use nom::{
    combinator::{map, opt},
    sequence::tuple,
};

use super::{base::*, WebToplevel};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebConstantDeclaration<'a> {
    /// The name of the constant.
    name: StringSpan<'a>,

    /// The value of the constant.
    value: PascalToken<'a>,

    /// Optional comment.
    comment: Option<WebComment<'a>>,
}

pub fn parse_constant_declaration<'a>(input: ParseInput<'a>) -> ParseResult<'a, WebToplevel<'a>> {
    map(
        tuple((
            identifier,
            pascal_token(PascalToken::Equals),
            int_literal,
            pascal_token(PascalToken::Semicolon),
            opt(comment),
        )),
        |tup| {
            WebToplevel::ConstDeclaration(WebConstantDeclaration {
                name: tup.0,
                value: tup.2,
                comment: tup.4,
            })
        },
    )(input)
}
