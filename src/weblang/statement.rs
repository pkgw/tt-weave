//! A WEB statement.

use nom::{
    branch::alt,
    combinator::{map, opt},
    multi::{many0, many1, separated_list1},
    sequence::tuple,
};

use super::{
    base::*,
    expr::{parse_expr, parse_lhs_expr, WebExpr},
    preprocessor_directive, WebToplevel,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WebStatement<'a> {
    /// A reference to a module.
    ModuleReference(StringSpan<'a>),

    /// A block of statements.
    Block(WebBlock<'a>),

    /// An assignment.
    Assignment(WebAssignment<'a>),

    /// A preprocessor directive.
    PreprocessorDirective(preprocessor_directive::WebPreprocessorDirective<'a>),

    /// A goto
    Goto(WebGoto<'a>),

    /// An `if` statement.
    If(WebIf<'a>),

    /// A `while` loop.
    While(WebWhile<'a>),

    /// A `for` loop.
    For(WebFor<'a>),

    /// A `loop` loop, implemented with a @define formatted like `Xclause`
    Loop(WebLoop<'a>),

    /// A label.
    Label(StringSpan<'a>),

    /// A case statement.
    Case(WebCase<'a>),

    /// A statement that's just an expression.
    Expr(WebExpr<'a>, Option<Vec<TypesetComment<'a>>>),

    /// A free-floating case statement, needed for WEAVE#88.
    SpecialFreeCase(SpecialFreeCase<'a>),
}

pub fn parse_statement_base<'a>(input: ParseInput<'a>) -> ParseResult<'a, WebStatement<'a>> {
    alt((
        parse_mod_ref_statement,
        parse_block,
        map(
            preprocessor_directive::parse_preprocessor_directive_base,
            |d| WebStatement::PreprocessorDirective(d),
        ),
        parse_goto,
        parse_if,
        parse_while,
        parse_for,
        parse_case,
        parse_assignment,
        parse_label,
        parse_loop,
        parse_special_free_case,
        parse_expr_statement,
    ))(input)
}

pub fn parse_statement<'a>(input: ParseInput<'a>) -> ParseResult<'a, WebToplevel<'a>> {
    map(tuple((parse_statement_base, opt(comment))), |t| {
        WebToplevel::Statement(t.0, t.1)
    })(input)
}

fn parse_mod_ref_statement<'a>(input: ParseInput<'a>) -> ParseResult<'a, WebStatement<'a>> {
    map(
        tuple((module_reference, opt(pascal_token(PascalToken::Semicolon)))),
        |t| WebStatement::ModuleReference(t.0),
    )(input)
}

fn parse_expr_statement<'a>(input: ParseInput<'a>) -> ParseResult<'a, WebStatement<'a>> {
    map(
        tuple((
            parse_expr,
            opt(pascal_token(PascalToken::Semicolon)),
            opt(comment),
        )),
        |t| WebStatement::Expr(t.0, t.2),
    )(input)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebBlock<'a> {
    /// The token that opens the block.
    opener: PascalToken<'a>,

    /// Inner statements.
    stmts: Vec<Box<WebStatement<'a>>>,

    /// The token that closes the block.
    closer: PascalToken<'a>,

    /// Optional comment after
    post_comment: Option<Vec<TypesetComment<'a>>>,
}

fn parse_block<'a>(input: ParseInput<'a>) -> ParseResult<'a, WebStatement<'a>> {
    let (input, items) = tuple((
        block_opener,
        many0(map(parse_statement_base, |s| Box::new(s))),
        block_closer,
        opt(pascal_token(PascalToken::Semicolon)),
        opt(comment),
    ))(input)?;

    let opener = items.0;
    let stmts = items.1;
    let closer = items.2;
    let post_comment = items.4;

    Ok((
        input,
        WebStatement::Block(WebBlock {
            opener,
            stmts,
            closer,
            post_comment,
        }),
    ))
}

/// Match a token that opens a block: either `begin`, or a formatted identifier
/// that behaves like it.
pub fn block_opener<'a>(input: ParseInput<'a>) -> ParseResult<'a, PascalToken<'a>> {
    let (input, wt) = next_token(input)?;

    if let WebToken::Pascal(ptok) = wt {
        if let PascalToken::ReservedWord(SpanValue {
            value: PascalReservedWord::Begin,
            ..
        }) = ptok
        {
            return Ok((input, ptok));
        } else if let PascalToken::FormattedIdentifier(_, PascalReservedWord::Begin) = ptok {
            return Ok((input, ptok));
        }
    }

    return new_parse_err(input, WebErrorKind::Eof);
}

/// Match a token that closes a block: either `end`, or a formatted identifier
/// that behaves like it.
pub fn block_closer<'a>(input: ParseInput<'a>) -> ParseResult<'a, PascalToken<'a>> {
    let (input, wt) = next_token(input)?;

    if let WebToken::Pascal(ptok) = wt {
        if let PascalToken::ReservedWord(SpanValue {
            value: PascalReservedWord::End,
            ..
        }) = ptok
        {
            return Ok((input, ptok));
        } else if let PascalToken::FormattedIdentifier(_, PascalReservedWord::End) = ptok {
            return Ok((input, ptok));
        }
    }

    return new_parse_err(input, WebErrorKind::Eof);
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebAssignment<'a> {
    /// The left-hand side.
    lhs: Box<WebExpr<'a>>,

    /// The right-hand side.
    rhs: Box<WebExpr<'a>>,

    /// Optional comment.
    comment: Option<Vec<TypesetComment<'a>>>,
}

fn parse_assignment<'a>(input: ParseInput<'a>) -> ParseResult<'a, WebStatement<'a>> {
    let (input, items) = tuple((
        parse_lhs_expr,
        pascal_token(PascalToken::Gets),
        parse_expr,
        opt(pascal_token(PascalToken::Semicolon)),
        opt(comment),
    ))(input)?;

    let lhs = Box::new(items.0);
    let rhs = Box::new(items.2);
    let comment = items.4;

    Ok((
        input,
        WebStatement::Assignment(WebAssignment { lhs, rhs, comment }),
    ))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebGoto<'a> {
    /// The label.
    label: StringSpan<'a>,

    /// Optional comment.
    comment: Option<Vec<TypesetComment<'a>>>,
}

fn parse_goto<'a>(input: ParseInput<'a>) -> ParseResult<'a, WebStatement<'a>> {
    let (input, items) = tuple((
        reserved_word(PascalReservedWord::Goto),
        identifier,
        opt(pascal_token(PascalToken::Semicolon)),
        opt(comment),
    ))(input)?;

    let label = items.1;
    let comment = items.3;

    Ok((input, WebStatement::Goto(WebGoto { label, comment })))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebIf<'a> {
    /// The test expression
    test: Box<WebExpr<'a>>,

    /// The `then` statement, which may be a block.
    then: Box<WebStatement<'a>>,

    /// The optional `else` statement, which may be a block, or may be another
    /// `if` statement.
    else_: Option<Box<WebStatement<'a>>>,
}

fn parse_if<'a>(input: ParseInput<'a>) -> ParseResult<'a, WebStatement<'a>> {
    let (input, items) = tuple((
        reserved_word(PascalReservedWord::If),
        parse_expr,
        reserved_word(PascalReservedWord::Then),
        parse_statement_base,
        opt(tuple((
            reserved_word(PascalReservedWord::Else),
            parse_statement_base,
        ))),
    ))(input)?;

    let test = Box::new(items.1);
    let then = Box::new(items.3);
    let else_ = items.4.map(|t| Box::new(t.1));

    Ok((input, WebStatement::If(WebIf { test, then, else_ })))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebWhile<'a> {
    /// The loop test expression
    test: Box<WebExpr<'a>>,

    /// The `do` statement, which may be a block.
    do_: Box<WebStatement<'a>>,
}

fn parse_while<'a>(input: ParseInput<'a>) -> ParseResult<'a, WebStatement<'a>> {
    let (input, items) = tuple((
        reserved_word(PascalReservedWord::While),
        parse_expr,
        reserved_word(PascalReservedWord::Do),
        parse_statement_base,
    ))(input)?;

    let test = Box::new(items.1);
    let do_ = Box::new(items.3);

    Ok((input, WebStatement::While(WebWhile { test, do_ })))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebFor<'a> {
    /// The loop variable
    var: StringSpan<'a>,

    /// The start expression.
    start: Box<WebExpr<'a>>,

    /// The end expression.
    end: Box<WebExpr<'a>>,

    /// The `do` statement, which may be a block.
    do_: Box<WebStatement<'a>>,
}

fn parse_for<'a>(input: ParseInput<'a>) -> ParseResult<'a, WebStatement<'a>> {
    let (input, items) = tuple((
        reserved_word(PascalReservedWord::For),
        identifier,
        pascal_token(PascalToken::Gets),
        parse_expr,
        reserved_word(PascalReservedWord::To),
        parse_expr,
        reserved_word(PascalReservedWord::Do),
        parse_statement_base,
    ))(input)?;

    let var = items.1;
    let start = Box::new(items.3);
    let end = Box::new(items.5);
    let do_ = Box::new(items.7);

    Ok((
        input,
        WebStatement::For(WebFor {
            var,
            start,
            end,
            do_,
        }),
    ))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebLoop<'a> {
    /// The identifier used in the loop definition
    keyword: StringSpan<'a>,

    /// The `do` statement, which may be a block.
    do_: Box<WebStatement<'a>>,
}

fn parse_loop<'a>(input: ParseInput<'a>) -> ParseResult<'a, WebStatement<'a>> {
    map(tuple((loop_like_identifier, parse_statement_base)), |t| {
        WebStatement::Loop(WebLoop {
            keyword: t.0,
            do_: Box::new(t.1),
        })
    })(input)
}

pub fn loop_like_identifier<'a>(input: ParseInput<'a>) -> ParseResult<'a, StringSpan<'a>> {
    let (input, wt) = next_token(input)?;

    if let WebToken::Pascal(PascalToken::FormattedIdentifier(ss, PascalReservedWord::Xclause)) = wt
    {
        Ok((input, ss))
    } else {
        new_parse_err(input, WebErrorKind::Eof)
    }
}

fn parse_label<'a>(input: ParseInput<'a>) -> ParseResult<'a, WebStatement<'a>> {
    map(tuple((identifier, pascal_token(PascalToken::Colon))), |t| {
        WebStatement::Label(t.0)
    })(input)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebCase<'a> {
    /// The variable of the case statement.
    var: StringSpan<'a>,

    /// Items within the case statement.
    items: Vec<WebCaseItem<'a>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WebCaseItem<'a> {
    ModuleReference(StringSpan<'a>),
    Standard(WebStandardCaseItem<'a>),
    OtherCases(WebOtherCasesItem<'a>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebStandardCaseItem<'a> {
    /// The matched cases. These may be identifiers or string literals
    /// or integer literals.
    matches: Vec<PascalToken<'a>>,

    /// The associated statement.
    stmt: Box<WebStatement<'a>>,

    /// Optional comment.
    comment: Option<Vec<TypesetComment<'a>>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebOtherCasesItem<'a> {
    /// The formatted identifier used to tag this item.
    tag: StringSpan<'a>,

    /// The associated statement.
    stmt: Box<WebStatement<'a>>,

    /// Optional comment.
    comment: Option<Vec<TypesetComment<'a>>>,
}

fn parse_case<'a>(input: ParseInput<'a>) -> ParseResult<'a, WebStatement<'a>> {
    map(
        tuple((
            reserved_word(PascalReservedWord::Case),
            identifier,
            reserved_word(PascalReservedWord::Of),
            many1(debug(
                "CI",
                alt((
                    parse_mod_ref_case_item,
                    parse_other_cases_item,
                    parse_standard_case_item,
                )),
            )),
            parse_case_terminator,
            opt(pascal_token(PascalToken::Semicolon)),
        )),
        |t| {
            WebStatement::Case(WebCase {
                var: t.1,
                items: t.3,
            })
        },
    )(input)
}

/// `endcases` is a formatted identifier formatted like `End`
fn parse_case_terminator<'a>(input: ParseInput<'a>) -> ParseResult<'a, StringSpan<'a>> {
    let (input, wt) = next_token(input)?;

    if let WebToken::Pascal(PascalToken::FormattedIdentifier(ss, PascalReservedWord::End)) = wt {
        Ok((input, ss))
    } else {
        new_parse_err(input, WebErrorKind::Eof)
    }
}

fn parse_mod_ref_case_item<'a>(input: ParseInput<'a>) -> ParseResult<'a, WebCaseItem<'a>> {
    map(
        tuple((module_reference, opt(pascal_token(PascalToken::Semicolon)))),
        |t| WebCaseItem::ModuleReference(t.0),
    )(input)
}

fn parse_other_cases_item<'a>(input: ParseInput<'a>) -> ParseResult<'a, WebCaseItem<'a>> {
    map(
        tuple((
            parse_other_cases_tag,
            parse_statement_base,
            opt(pascal_token(PascalToken::Semicolon)),
            opt(comment),
        )),
        |t| {
            WebCaseItem::OtherCases(WebOtherCasesItem {
                tag: t.0,
                stmt: Box::new(t.1),
                comment: t.3,
            })
        },
    )(input)
}

/// `endcases` is a formatted identifier formatted like `Else`
fn parse_other_cases_tag<'a>(input: ParseInput<'a>) -> ParseResult<'a, StringSpan<'a>> {
    let (input, wt) = next_token(input)?;

    if let WebToken::Pascal(PascalToken::FormattedIdentifier(ss, PascalReservedWord::Else)) = wt {
        Ok((input, ss))
    } else {
        new_parse_err(input, WebErrorKind::Eof)
    }
}

fn parse_standard_case_item<'a>(input: ParseInput<'a>) -> ParseResult<'a, WebCaseItem<'a>> {
    map(
        tuple((
            separated_list1(
                pascal_token(PascalToken::Comma),
                alt((merged_string_literals, case_match_token)),
            ),
            pascal_token(PascalToken::Colon),
            parse_statement_base,
            opt(pascal_token(PascalToken::Semicolon)),
            opt(comment),
        )),
        |t| {
            WebCaseItem::Standard(WebStandardCaseItem {
                matches: t.0,
                stmt: Box::new(t.2),
                comment: t.4,
            })
        },
    )(input)
}

fn case_match_token<'a>(input: ParseInput<'a>) -> ParseResult<'a, PascalToken<'a>> {
    let (input, wt) = next_token(input)?;

    if let WebToken::Pascal(pt) = wt {
        match pt {
            PascalToken::Identifier(..) | PascalToken::IntLiteral(..) => return Ok((input, pt)),

            _ => {}
        }
    }

    return new_parse_err(input, WebErrorKind::Eof);
}

// "Special" statements that we need to have for funky WEB structures

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpecialFreeCase<'a> {
    /// The matched cases.
    matches: Vec<PascalToken<'a>>,

    /// The associated statement.
    stmt: Box<WebStatement<'a>>,
}

fn parse_special_free_case<'a>(input: ParseInput<'a>) -> ParseResult<'a, WebStatement<'a>> {
    map(
        tuple((
            separated_list1(pascal_token(PascalToken::Comma), merged_string_literals),
            pascal_token(PascalToken::Colon),
            parse_statement_base,
            opt(pascal_token(PascalToken::Semicolon)),
        )),
        |t| {
            WebStatement::SpecialFreeCase(SpecialFreeCase {
                matches: t.0,
                stmt: Box::new(t.2),
            })
        },
    )(input)
}
