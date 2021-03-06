//! Higher-level WEB language processing.
//!
//! This is *mostly* Pascal, but with a few additions. We implement parsing with
//! `nom` where the underlying datatype is a sequence of tokens.

use nom::{
    branch::alt,
    bytes::complete::take_while,
    combinator::opt,
    multi::{many1, separated_list1},
    Finish, InputLength,
};

pub mod base;
mod comment;
mod const_declaration;
mod define;
mod expr;
mod format;
mod forward_declaration;
mod function_definition;
mod label_declaration;
pub mod module_reference;
mod modulified_declaration;
mod preprocessor_directive;
mod program_definition;
mod standalone;
mod statement;
mod type_declaration;
mod var_declaration;
mod webtype;

use crate::prettify::{self, Prettifier, RenderInline, TexInsert, COMMENT_SCOPE};

use self::{
    base::*,
    expr::{parse_expr, WebExpr},
    statement::WebStatement,
};

pub use self::base::{WebSyntax, WebToken};

/// A top-level WEB production.
///
/// A "top-level" is whatever it takes to make it true that any WEB Pascal block
/// can be expressed as a series of toplevels, including `@define` and `@format`
/// statements. Because we're not actually compiling the WEB language in any
/// meaningful way, we're not very intellectually rigorous.
///
/// Toplevel module references are captured as Statements.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WebToplevel<'a> {
    /// A `@d` definition.
    Define(define::WebDefine<'a>),

    /// A `@f` format definition.
    Format(format::WebFormat<'a>),

    /// A single Pascal token (with optional comment).
    Standalone(standalone::WebStandalone<'a>),

    /// The program definition.
    ProgramDefinition(program_definition::WebProgramDefinition<'a>),

    /// A label declaration.
    LabelDeclaration(label_declaration::WebLabelDeclaration<'a>),

    /// Declarations that are done by referencing a module.
    ModulifiedDeclaration(modulified_declaration::WebModulifiedDeclaration<'a>),

    /// Definition of a procedure or function
    FunctionDefinition(function_definition::WebFunctionDefinition<'a>),

    /// Declaration of a constant.
    ConstDeclaration(const_declaration::WebConstantDeclaration<'a>),

    /// Declaration of a variable.
    VarDeclaration(var_declaration::WebVarDeclaration<'a>),

    /// Declaration of a type.
    TypeDeclaration(type_declaration::WebTypeDeclaration<'a>),

    /// Forward declaration of a function or procedure.
    ForwardDeclaration(forward_declaration::WebForwardDeclaration<'a>),

    /// A Pascal statement.
    Statement(WebStatement<'a>, Option<WebComment<'a>>),

    /// No code at all, needed for XeTeX(2022.0):23.
    Empty,

    /// `( $ident $ident )`, needed for WEAVE:143
    SpecialParenTwoIdent(StringSpan<'a>, StringSpan<'a>),

    /// `[]`, needed for WEAVE:143
    SpecialEmptyBrackets,

    /// `$relational_op $expr`, needed for WEAVE:144
    SpecialRelationalExpr(PascalToken<'a>, WebExpr<'a>),

    /// `$expr .. $expr`, needed for WEAVE:144, XeTeX(2022.0):83
    SpecialRange(WebExpr<'a>, WebExpr<'a>),

    /// `$begin_like $function $end_like`, needed for WEAVE:260
    SpecialIfdefFunction(
        PascalToken<'a>,
        function_definition::WebFunctionDefinition<'a>,
        PascalToken<'a>,
    ),

    /// `$begin_like $forward_declaration $end_like`, needed for WEAVE:30 and others
    SpecialIfdefForward(
        PascalToken<'a>,
        forward_declaration::WebForwardDeclaration<'a>,
        PascalToken<'a>,
    ),

    /// `$begin_like $var_declaration $end_like`, needed for WEAVE:244
    SpecialIfdefVarDeclaration(
        Option<WebComment<'a>>,
        PascalToken<'a>,
        Vec<var_declaration::WebVarDeclaration<'a>>,
        PascalToken<'a>,
        Option<WebComment<'a>>,
    ),

    /// `$start_meta_comment $statement $end_meta_comment`, needed for XeTeX(2022.0):31.
    SpecialCommentedOut(WebStatement<'a>),

    /// `$[$term0, $term1a .. $term1b, ...]`, needed for XeTeX(2022.0):49, and similar
    /// with parentheses, for XeTeX(2022.0):620. The bool is true
    SpecialListLiteral {
        is_square: bool,
        terms: Vec<SpecialListLiteralTerm<'a>>,
    },

    /// `$ident in [$term0, $term1a .. $term1b, ...]`, needed for XeTeX(2022.0):49.
    SpecialIdentInListLiteral(StringSpan<'a>, Vec<SpecialListLiteralTerm<'a>>),

    /// `($ident1, $ident2, ...) := ($term1, $term2, ...)`, needed for
    /// XeTeX(2022.0):621.
    SpecialListLiteralAssignment {
        lhs: Vec<SpecialListLiteralTerm<'a>>,
        rhs: Vec<SpecialListLiteralTerm<'a>>,
    },

    /// `$expr == $expr`, needed for XeTeX(2022.0):134.
    SpecialInlineDefine(WebExpr<'a>, WebExpr<'a>),

    /// `$expr, $expr, $expr {,}?`, needed for XeTeX(2022.0):375, with optional
    /// trailing comma, needed for XeTeX(2022.0):1102 and friends.
    SpecialCommaExprs {
        exprs: Vec<Box<WebExpr<'a>>>,
        trailing_comma: bool,
    },

    /// `[$expr..$expr]$ident`, needed for XeTeX(2022.0):576, which uses some
    /// macros to create a specialized array table.
    SpecialArrayMacro(WebExpr<'a>, WebExpr<'a>, StringSpan<'a>),

    /// `$ident=.25`, needed for XeTeX(2022.0):582.
    SpecialFloatEquality(StringSpan<'a>, PascalToken<'a>),

    /// `$ident [ $int $ident ]`, needed for XeTeX(2022.0):621.
    SpecialCoeffArray {
        name: StringSpan<'a>,
        coeff: PascalToken<'a>,
        base: PascalToken<'a>,
    },

    /// `end ; <end-of-tokens>`, needed for XeTeX(2022.0):639.
    /// Module 638 has an imbalanced `begin` and ends by including
    /// this module, which has an imbalanced end.
    SpecialImbalancedEnd,

    /// `$expr .`, needed for XeTeX(2022.0):684.
    SpecialExprPeriod(WebExpr<'a>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SpecialListLiteralTerm<'a> {
    Single(PascalToken<'a>),
    Range(PascalToken<'a>, PascalToken<'a>),
    Unary(PascalToken<'a>, PascalToken<'a>),
}

impl<'a> RenderInline for SpecialListLiteralTerm<'a> {
    fn measure_inline(&self) -> usize {
        match self {
            SpecialListLiteralTerm::Single(t) => t.measure_inline(),
            SpecialListLiteralTerm::Range(t1, t2) => t1.measure_inline() + 4 + t2.measure_inline(),
            SpecialListLiteralTerm::Unary(t1, t2) => t1.measure_inline() + t2.measure_inline(),
        }
    }

    fn render_inline(&self, dest: &mut Prettifier) {
        match self {
            SpecialListLiteralTerm::Single(t) => t.render_inline(dest),
            SpecialListLiteralTerm::Range(t1, t2) => {
                t1.render_inline(dest);
                dest.noscope_push(" .. ");
                t2.render_inline(dest);
            }
            SpecialListLiteralTerm::Unary(t1, t2) => {
                t1.render_inline(dest);
                t2.render_inline(dest);
            }
        }
    }
}

/// A block of WEB code: a sequence of parsed-out WEB toplevels
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebCode<'a>(pub Vec<WebToplevel<'a>>);

impl<'a> WebCode<'a> {
    /// Parse a sequence of WEB tokens into sequence of toplevels.
    pub fn parse(syntax: &'a WebSyntax<'a>) -> Option<WebCode<'a>> {
        let input = ParseInput(&syntax.0[..]);

        if input.input_len() == 0 {
            return Some(WebCode(vec![WebToplevel::Empty]));
        }

        match many1(parse_toplevel)(input).finish() {
            Ok((remainder, value)) => {
                if remainder.input_len() > 0 {
                    eprintln!("\nincomplete parse");
                    return None;
                } else {
                    return Some(WebCode(value));
                }
            }

            Err((_remainder, e)) => {
                eprintln!("parse error: {:?}", e);
                return None;
            }
        }
    }
}

fn is_ignored_token(t: WebToken) -> bool {
    match t {
        WebToken::Pascal(PascalToken::Formatting)
        | WebToken::Pascal(PascalToken::ForcedEol)
        | WebToken::Pascal(PascalToken::TexString(..)) => true,
        _ => false,
    }
}

fn parse_toplevel<'a>(input: ParseInput<'a>) -> ParseResult<'a, WebToplevel<'a>> {
    let (input, _) = take_while(is_ignored_token)(input)?;

    // We have so many possibilities that we need to use multiple alt() calls to
    // avoid the limit of 20-item tuples!
    let result = alt((
        // Define comes first since its tail is a toplevel in and of itself.
        define::parse_define,
        format::parse_format,
        program_definition::parse_program_definition,
        label_declaration::parse_label_declaration,
        modulified_declaration::parse_modulified_declaration,
        forward_declaration::parse_forward_declaration,
        function_definition::parse_function_definition,
        const_declaration::parse_constant_declaration,
        var_declaration::parse_var_declaration,
        type_declaration::parse_type_declaration,
        alt((
            tl_specials::parse_special_ifdef_forward,
            tl_specials::parse_special_ifdef_function,
            tl_specials::parse_special_ifdef_var_decl,
            tl_specials::parse_special_paren_two_ident,
            tl_specials::parse_special_empty_brackets,
            tl_specials::parse_special_relational_expr,
            tl_specials::parse_special_range,
            tl_specials::parse_special_commented_out,
            tl_specials::parse_special_array_macro,
            tl_specials::parse_special_list_assignment,
            tl_specials::parse_special_int_list,
            tl_specials::parse_special_ident_in_int_list,
            tl_specials::parse_special_inline_define,
            tl_specials::parse_special_comma_exprs,
            tl_specials::parse_special_float_equality,
            tl_specials::parse_special_coeff_array,
            tl_specials::parse_special_imbalanced_end,
            tl_specials::parse_special_expr_period,
        )),
        statement::parse_statement,
        standalone::parse_standalone,
    ))(input);

    //match &result {
    //    Ok((input, v)) => {
    //        eprintln!("TL OK: {:?}", v);
    //        let n = usize::min(input.input_len(), 8);
    //        for tok in &input.0[..n] {
    //            eprintln!("- {:?}", tok);
    //        }
    //    }
    //
    //    Err(nom::Err::Error((input, kind))) => {
    //        if kind != &WebErrorKind::Eof {
    //            eprintln!("TL error {:?}", kind);
    //            let n = usize::min(input.input_len(), 20);
    //            for tok in &input.0[..n] {
    //                eprintln!("- {:?}", tok);
    //            }
    //        }
    //    }
    //
    //    _ => {
    //        eprintln!("TL other failure???");
    //    }
    //}

    result
}

mod tl_specials {
    use nom::{combinator::map, sequence::tuple};

    use super::*;

    pub fn parse_special_paren_two_ident<'a>(
        input: ParseInput<'a>,
    ) -> ParseResult<'a, WebToplevel<'a>> {
        map(
            tuple((
                open_delimiter(DelimiterKind::Paren),
                identifier,
                identifier,
                close_delimiter(DelimiterKind::Paren),
            )),
            |t| WebToplevel::SpecialParenTwoIdent(t.1, t.2),
        )(input)
    }

    pub fn parse_special_empty_brackets<'a>(
        input: ParseInput<'a>,
    ) -> ParseResult<'a, WebToplevel<'a>> {
        map(
            tuple((
                open_delimiter(DelimiterKind::SquareBracket),
                close_delimiter(DelimiterKind::SquareBracket),
            )),
            |_| WebToplevel::SpecialEmptyBrackets,
        )(input)
    }

    pub fn parse_special_relational_expr<'a>(
        input: ParseInput<'a>,
    ) -> ParseResult<'a, WebToplevel<'a>> {
        map(tuple((relational_ident_op, parse_expr)), |t| {
            WebToplevel::SpecialRelationalExpr(t.0, t.1)
        })(input)
    }

    fn relational_ident_op<'a>(input: ParseInput<'a>) -> ParseResult<'a, PascalToken<'a>> {
        let (input, wt) = next_token(input)?;

        if let WebToken::Pascal(pt) = wt {
            match pt {
                PascalToken::Greater
                | PascalToken::GreaterEquals
                | PascalToken::Less
                | PascalToken::LessEquals
                | PascalToken::Equals
                | PascalToken::NotEquals => return Ok((input, pt)),

                _ => {}
            }
        }

        return new_parse_err(input, WebErrorKind::Eof);
    }

    pub fn parse_special_range<'a>(input: ParseInput<'a>) -> ParseResult<'a, WebToplevel<'a>> {
        map(
            tuple((parse_expr, pascal_token(PascalToken::DoubleDot), parse_expr)),
            |t| WebToplevel::SpecialRange(t.0, t.2),
        )(input)
    }

    pub fn parse_special_ifdef_function<'a>(
        input: ParseInput<'a>,
    ) -> ParseResult<'a, WebToplevel<'a>> {
        map(
            tuple((
                formatted_identifier_like(PascalReservedWord::Begin),
                function_definition::parse_function_definition_base,
                formatted_identifier_like(PascalReservedWord::End),
            )),
            |t| WebToplevel::SpecialIfdefFunction(t.0, t.1, t.2),
        )(input)
    }

    pub fn parse_special_ifdef_forward<'a>(
        input: ParseInput<'a>,
    ) -> ParseResult<'a, WebToplevel<'a>> {
        map(
            tuple((
                formatted_identifier_like(PascalReservedWord::Begin),
                forward_declaration::parse_forward_declaration_base,
                formatted_identifier_like(PascalReservedWord::End),
            )),
            |t| WebToplevel::SpecialIfdefForward(t.0, t.1, t.2),
        )(input)
    }

    pub fn parse_special_ifdef_var_decl<'a>(
        input: ParseInput<'a>,
    ) -> ParseResult<'a, WebToplevel<'a>> {
        map(
            tuple((
                opt(comment),
                formatted_identifier_like(PascalReservedWord::Begin),
                many1(var_declaration::parse_var_declaration_base),
                formatted_identifier_like(PascalReservedWord::End),
                opt(comment),
            )),
            |t| WebToplevel::SpecialIfdefVarDeclaration(t.0, t.1, t.2, t.3, t.4),
        )(input)
    }

    pub fn parse_special_commented_out<'a>(
        input: ParseInput<'a>,
    ) -> ParseResult<'a, WebToplevel<'a>> {
        map(
            tuple((
                pascal_token(PascalToken::OpenDelimiter(DelimiterKind::MetaComment)),
                statement::parse_statement_base,
                pascal_token(PascalToken::CloseDelimiter(DelimiterKind::MetaComment)),
            )),
            |t| WebToplevel::SpecialCommentedOut(t.1),
        )(input)
    }

    pub fn parse_special_list_assignment<'a>(
        input: ParseInput<'a>,
    ) -> ParseResult<'a, WebToplevel<'a>> {
        map(
            tuple((
                pascal_token(PascalToken::OpenDelimiter(DelimiterKind::Paren)),
                separated_list1(pascal_token(PascalToken::Comma), int_list_term),
                pascal_token(PascalToken::CloseDelimiter(DelimiterKind::Paren)),
                pascal_token(PascalToken::Gets),
                pascal_token(PascalToken::OpenDelimiter(DelimiterKind::Paren)),
                separated_list1(pascal_token(PascalToken::Comma), int_list_term),
                pascal_token(PascalToken::CloseDelimiter(DelimiterKind::Paren)),
            )),
            |t| WebToplevel::SpecialListLiteralAssignment { lhs: t.1, rhs: t.5 },
        )(input)
    }

    pub fn parse_special_int_list<'a>(input: ParseInput<'a>) -> ParseResult<'a, WebToplevel<'a>> {
        alt((
            map(
                tuple((
                    pascal_token(PascalToken::OpenDelimiter(DelimiterKind::SquareBracket)),
                    separated_list1(pascal_token(PascalToken::Comma), int_list_term),
                    pascal_token(PascalToken::CloseDelimiter(DelimiterKind::SquareBracket)),
                )),
                |t| WebToplevel::SpecialListLiteral {
                    is_square: true,
                    terms: t.1,
                },
            ),
            map(
                tuple((
                    pascal_token(PascalToken::OpenDelimiter(DelimiterKind::Paren)),
                    separated_list1(pascal_token(PascalToken::Comma), int_list_term),
                    pascal_token(PascalToken::CloseDelimiter(DelimiterKind::Paren)),
                )),
                |t| WebToplevel::SpecialListLiteral {
                    is_square: false,
                    terms: t.1,
                },
            ),
        ))(input)
    }

    pub fn parse_special_ident_in_int_list<'a>(
        input: ParseInput<'a>,
    ) -> ParseResult<'a, WebToplevel<'a>> {
        map(
            tuple((
                identifier,
                reserved_word(PascalReservedWord::In),
                pascal_token(PascalToken::OpenDelimiter(DelimiterKind::SquareBracket)),
                separated_list1(pascal_token(PascalToken::Comma), int_list_term),
                pascal_token(PascalToken::CloseDelimiter(DelimiterKind::SquareBracket)),
            )),
            |t| WebToplevel::SpecialIdentInListLiteral(t.0, t.3),
        )(input)
    }

    fn int_list_term<'a>(input: ParseInput<'a>) -> ParseResult<'a, SpecialListLiteralTerm<'a>> {
        alt((
            map(
                tuple((
                    int_literal,
                    pascal_token(PascalToken::DoubleDot),
                    int_literal,
                )),
                |t| SpecialListLiteralTerm::Range(t.0, t.2),
            ),
            map(alt((int_literal, identifier_as_token)), |i| {
                SpecialListLiteralTerm::Single(i)
            }),
            map(
                tuple((
                    // TODO: make more generic as needed.
                    pascal_token(PascalToken::Minus),
                    identifier_as_token,
                )),
                |t| SpecialListLiteralTerm::Unary(t.0, t.1),
            ),
        ))(input)
    }

    pub fn parse_special_inline_define<'a>(
        input: ParseInput<'a>,
    ) -> ParseResult<'a, WebToplevel<'a>> {
        map(
            tuple((
                parse_expr,
                pascal_token(PascalToken::Equivalence),
                parse_expr,
            )),
            |t| WebToplevel::SpecialInlineDefine(t.0, t.2),
        )(input)
    }

    // We have to peek at the end of the input here because otherwise we'll
    // accept (e.g) the LHS of `a := b`. (We can't parse the generic statement
    // mode first since it would accept the `a` of `a, b, c`.)
    pub fn parse_special_comma_exprs<'a>(
        input: ParseInput<'a>,
    ) -> ParseResult<'a, WebToplevel<'a>> {
        let (input, t) = tuple((
            separated_list1(pascal_token(PascalToken::Comma), map(parse_expr, Box::new)),
            opt(pascal_token(PascalToken::Comma)),
            self::define::peek_end_of_define,
        ))(input)?;

        if t.0.len() < 2 && !t.1.is_some() {
            // Don't eat single expressions -- we want those to be expr statements.
            new_parse_err(input, WebErrorKind::Eof)
        } else {
            Ok((
                input,
                WebToplevel::SpecialCommaExprs {
                    exprs: t.0,
                    trailing_comma: t.1.is_some(),
                },
            ))
        }
    }

    pub fn parse_special_array_macro<'a>(
        input: ParseInput<'a>,
    ) -> ParseResult<'a, WebToplevel<'a>> {
        map(
            tuple((
                pascal_token(PascalToken::OpenDelimiter(DelimiterKind::SquareBracket)),
                parse_expr,
                pascal_token(PascalToken::DoubleDot),
                parse_expr,
                pascal_token(PascalToken::CloseDelimiter(DelimiterKind::SquareBracket)),
                identifier,
            )),
            |t| WebToplevel::SpecialArrayMacro(t.1, t.3, t.5),
        )(input)
    }

    pub fn parse_special_float_equality<'a>(
        input: ParseInput<'a>,
    ) -> ParseResult<'a, WebToplevel<'a>> {
        map(
            tuple((
                identifier,
                pascal_token(PascalToken::Equals),
                pascal_token(PascalToken::Period),
                int_literal,
            )),
            |t| WebToplevel::SpecialFloatEquality(t.0, t.3),
        )(input)
    }

    pub fn parse_special_coeff_array<'a>(
        input: ParseInput<'a>,
    ) -> ParseResult<'a, WebToplevel<'a>> {
        map(
            tuple((
                identifier,
                pascal_token(PascalToken::OpenDelimiter(DelimiterKind::SquareBracket)),
                int_literal,
                identifier_as_token,
                pascal_token(PascalToken::CloseDelimiter(DelimiterKind::SquareBracket)),
            )),
            |t| WebToplevel::SpecialCoeffArray {
                name: t.0,
                coeff: t.2,
                base: t.3,
            },
        )(input)
    }

    pub fn parse_special_imbalanced_end<'a>(
        input: ParseInput<'a>,
    ) -> ParseResult<'a, WebToplevel<'a>> {
        map(
            tuple((
                reserved_word(PascalReservedWord::End),
                pascal_token(PascalToken::Semicolon),
                define::peek_end_of_define,
            )),
            |_| WebToplevel::SpecialImbalancedEnd,
        )(input)
    }

    pub fn parse_special_expr_period<'a>(
        input: ParseInput<'a>,
    ) -> ParseResult<'a, WebToplevel<'a>> {
        map(
            tuple((
                parse_expr,
                pascal_token(PascalToken::Period),
                define::peek_end_of_define,
            )),
            |t| WebToplevel::SpecialExprPeriod(t.0),
        )(input)
    }
}

impl<'a> WebToplevel<'a> {
    pub fn prettify(&self, dest: &mut Prettifier) {
        match self {
            WebToplevel::Statement(stmt, comment) => tl_prettify::statement(stmt, comment, dest),
            WebToplevel::Standalone(s) => s.render_inline(dest),
            WebToplevel::Define(d) => d.prettify(dest),
            WebToplevel::Format(f) => f.prettify(dest),
            WebToplevel::LabelDeclaration(ld) => ld.prettify(dest),
            WebToplevel::ProgramDefinition(pd) => pd.prettify(dest),
            WebToplevel::ModulifiedDeclaration(md) => md.prettify(dest),
            WebToplevel::FunctionDefinition(fd) => fd.prettify(dest),
            WebToplevel::ConstDeclaration(cd) => cd.prettify(dest),
            WebToplevel::VarDeclaration(vd) => vd.prettify(dest),
            WebToplevel::TypeDeclaration(td) => td.prettify(dest),
            WebToplevel::ForwardDeclaration(fd) => fd.prettify(dest),
            WebToplevel::Empty => dest.scope_push(*COMMENT_SCOPE, "/*nothing*/"),

            WebToplevel::SpecialParenTwoIdent(id1, id2) => {
                tl_prettify::special_paren_two_ident(id1, id2, dest)
            }
            WebToplevel::SpecialEmptyBrackets => tl_prettify::special_empty_brackets(dest),
            WebToplevel::SpecialRelationalExpr(op, expr) => {
                tl_prettify::special_relational_expr(op, expr, dest)
            }
            WebToplevel::SpecialRange(e1, e2) => tl_prettify::special_range(e1, e2, dest),
            WebToplevel::SpecialIfdefFunction(beg, fd, end) => {
                tl_prettify::special_ifdef_function(beg, fd, end, dest)
            }
            WebToplevel::SpecialIfdefForward(beg, fd, end) => {
                tl_prettify::special_ifdef_forward(beg, fd, end, dest)
            }
            WebToplevel::SpecialIfdefVarDeclaration(c1, beg, vd, end, c2) => {
                tl_prettify::special_ifdef_var_declaration(c1, beg, vd, end, c2, dest)
            }
            WebToplevel::SpecialCommentedOut(stmt) => {
                tl_prettify::special_commented_out(stmt, dest)
            }
            WebToplevel::SpecialIdentInListLiteral(id, vals) => {
                tl_prettify::special_ident_in_int_list(id, vals, dest)
            }
            WebToplevel::SpecialListLiteral { is_square, terms } => {
                tl_prettify::special_int_list(*is_square, terms, dest)
            }
            WebToplevel::SpecialListLiteralAssignment { lhs, rhs } => {
                tl_prettify::special_list_literal_assignment(lhs, rhs, dest)
            }
            WebToplevel::SpecialInlineDefine(lhs, rhs) => {
                tl_prettify::special_inline_define(lhs, rhs, dest)
            }
            WebToplevel::SpecialCommaExprs {
                exprs,
                trailing_comma,
            } => tl_prettify::special_comma_exprs(exprs, *trailing_comma, dest),
            WebToplevel::SpecialArrayMacro(e1, e2, id) => {
                tl_prettify::special_array_macro(e1, e2, id, dest)
            }
            WebToplevel::SpecialFloatEquality(id, frac) => {
                tl_prettify::special_float_equality(id, frac, dest)
            }
            WebToplevel::SpecialCoeffArray { name, coeff, base } => {
                tl_prettify::special_coeff_array(name, coeff, base, dest)
            }
            WebToplevel::SpecialImbalancedEnd => {}
            WebToplevel::SpecialExprPeriod(expr) => tl_prettify::special_expr_period(expr, dest),
        }
    }
}

mod tl_prettify {
    use super::*;

    pub fn statement<'a>(
        stmt: &WebStatement<'a>,
        comment: &Option<WebComment<'a>>,
        dest: &mut Prettifier,
    ) {
        // Most statements won't be able to be rendered inline, but a few can.
        let clen = comment.as_ref().map(|c| c.measure_inline()).unwrap_or(0);
        let slen = stmt.measure_inline();

        if dest.fits(clen + slen + 1) {
            stmt.render_inline(dest);

            if let Some(c) = comment.as_ref() {
                dest.space();
                c.render_inline(dest);
            }
        } else if dest.fits(slen) {
            if let Some(c) = comment.as_ref() {
                c.render_inline(dest);
                dest.newline_needed();
            }

            stmt.render_inline(dest);
        } else {
            if let Some(c) = comment.as_ref() {
                c.render_inline(dest);
                dest.newline_needed();
            }

            stmt.render_flex(dest);
        }

        dest.newline_needed();
    }

    pub fn special_paren_two_ident<'a>(
        id1: &StringSpan<'a>,
        id2: &StringSpan<'a>,
        dest: &mut Prettifier,
    ) {
        dest.noscope_push('(');
        dest.noscope_push(id1);
        dest.noscope_push(id2);
        dest.noscope_push(')');
    }

    pub fn special_empty_brackets<'a>(dest: &mut Prettifier) {
        dest.noscope_push("[]");
    }

    pub fn special_relational_expr<'a>(
        op: &PascalToken<'a>,
        expr: &WebExpr<'a>,
        dest: &mut Prettifier,
    ) {
        op.render_inline(dest);
        expr.render_inline(dest);
    }

    pub fn special_range<'a>(e1: &WebExpr<'a>, e2: &WebExpr<'a>, dest: &mut Prettifier) {
        e1.render_inline(dest);
        dest.noscope_push(" .. ");
        e2.render_inline(dest);
    }

    pub fn special_ifdef_function<'a>(
        beg: &PascalToken<'a>,
        fd: &function_definition::WebFunctionDefinition<'a>,
        _end: &PascalToken<'a>,
        dest: &mut Prettifier,
    ) {
        beg.render_inline(dest);
        dest.noscope_push("!{");
        dest.indent_block();
        dest.newline_indent();
        fd.prettify(dest);
        dest.dedent_block();
        dest.newline_indent();
        dest.noscope_push('}');
    }

    pub fn special_ifdef_forward<'a>(
        beg: &PascalToken<'a>,
        fd: &forward_declaration::WebForwardDeclaration<'a>,
        _end: &PascalToken<'a>,
        dest: &mut Prettifier,
    ) {
        beg.render_inline(dest);
        dest.noscope_push("!{");
        dest.indent_block();
        dest.newline_indent();
        fd.prettify(dest);
        dest.dedent_block();
        dest.newline_indent();
        dest.noscope_push('}');
    }

    pub fn special_ifdef_var_declaration<'a>(
        c1: &Option<WebComment<'a>>,
        beg: &PascalToken<'a>,
        vds: &Vec<var_declaration::WebVarDeclaration<'a>>,
        _end: &PascalToken<'a>,
        c2: &Option<WebComment<'a>>,
        dest: &mut Prettifier,
    ) {
        if let Some(c) = c1.as_ref() {
            c.render_inline(dest);
            dest.newline_needed();
        }

        if let Some(c) = c2.as_ref() {
            c.render_inline(dest);
            dest.newline_needed();
        }

        beg.render_inline(dest);
        dest.noscope_push("!{");
        dest.indent_block();
        dest.newline_indent();

        for vd in vds {
            vd.prettify(dest);
            dest.newline_needed();
        }

        dest.dedent_block();
        dest.newline_indent();
        dest.noscope_push('}');
    }

    pub fn special_commented_out<'a>(stmt: &WebStatement<'a>, dest: &mut Prettifier) {
        dest.with_scope(*COMMENT_SCOPE, |d| {
            d.noscope_push("/*");
            d.indent_block();
            d.newline_needed();
            stmt.render_flex(d);
            stmt.maybe_semicolon(d);
            d.dedent_block();
            d.newline_needed();
            d.noscope_push("*/");
        });
    }

    pub fn special_int_list<'a>(
        is_square: bool,
        vals: &Vec<SpecialListLiteralTerm<'a>>,
        dest: &mut Prettifier,
    ) {
        let (open, close) = if is_square { ('[', ']') } else { ('(', ')') };

        dest.noscope_push(open);
        prettify::render_inline_seq(vals, ", ", dest);
        dest.noscope_push(close);
    }

    pub fn special_list_literal_assignment<'a>(
        lhs: &Vec<SpecialListLiteralTerm<'a>>,
        rhs: &Vec<SpecialListLiteralTerm<'a>>,
        dest: &mut Prettifier,
    ) {
        dest.noscope_push('(');
        prettify::render_inline_seq(lhs, ", ", dest);
        dest.noscope_push(") = (");
        prettify::render_inline_seq(rhs, ", ", dest);
        dest.noscope_push(')');
    }

    pub fn special_ident_in_int_list<'a>(
        id: &StringSpan<'a>,
        vals: &Vec<SpecialListLiteralTerm<'a>>,
        dest: &mut Prettifier,
    ) {
        dest.noscope_push(id);
        dest.space();
        dest.keyword("in");
        dest.space();
        dest.noscope_push("[");
        prettify::render_inline_seq(vals, ", ", dest);
        dest.noscope_push("]");
    }

    pub fn special_inline_define<'a>(lhs: &WebExpr<'a>, rhs: &WebExpr<'a>, dest: &mut Prettifier) {
        lhs.render_inline(dest);
        dest.noscope_push(" => ");
        rhs.render_inline(dest);
    }

    pub fn special_comma_exprs<'a>(
        exprs: &Vec<Box<WebExpr<'a>>>,
        trailing_comma: bool,
        dest: &mut Prettifier,
    ) {
        if dest.fits(prettify::measure_inline_seq(exprs, 2)) {
            prettify::render_inline_seq(exprs, ", ", dest);
        } else {
            let mut first = true;

            for expr in exprs {
                if first {
                    first = false;
                } else {
                    dest.noscope_push(',');
                }

                dest.newline_needed();
                expr.render_flex(dest);
            }

            if trailing_comma {
                dest.noscope_push(',');
            }
        }
    }

    /// This has a weird layout because it's used in the midst of of a custom
    /// TeX table macro used to lay out some pseudo-code.
    pub fn special_array_macro<'a>(
        e1: &WebExpr<'a>,
        e2: &WebExpr<'a>,
        id: &StringSpan<'a>,
        dest: &mut Prettifier,
    ) {
        dest.insert(TexInsert::XetexArrayMacroHackMarker, false);
        e1.render_inline(dest);
        dest.noscope_push(" .. ");
        e2.render_inline(dest);
        dest.insert(TexInsert::XetexArrayMacroHackBracket, false);

        // Push this with a different scope so that the syntect highlighting
        // will cause a span break between the previous piece of text and this
        // one, so that the bracket above can occur outside of wrapping braces,
        // which is necessary for it to be picked up by the \arr macro
        // expansion.
        dest.scope_push(*super::prettify::KEYWORD_SCOPE, id.value.as_ref());
    }

    pub fn special_float_equality<'a>(
        id: &StringSpan<'a>,
        frac: &PascalToken<'a>,
        dest: &mut Prettifier,
    ) {
        dest.noscope_push(id.value.as_ref());
        dest.noscope_push(" = 0.");
        frac.render_inline(dest);
    }

    pub fn special_coeff_array<'a>(
        name: &StringSpan<'a>,
        coeff: &PascalToken<'a>,
        base: &PascalToken<'a>,
        dest: &mut Prettifier,
    ) {
        dest.noscope_push(name.value.as_ref());
        dest.noscope_push('[');
        coeff.render_inline(dest);
        base.render_inline(dest);
        dest.noscope_push(']');
    }

    pub fn special_expr_period<'a>(expr: &WebExpr<'a>, dest: &mut Prettifier) {
        expr.render_inline(dest);
        dest.noscope_push('.');
    }
}
