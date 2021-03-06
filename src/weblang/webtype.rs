//! Types in WEB.
//!
//! I.e., Pascal types.

use nom::{
    branch::alt,
    combinator::{map, opt},
    multi::{many1, separated_list0, separated_list1},
    sequence::tuple,
};

use crate::prettify::{self, Prettifier, RenderInline};

use super::base::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WebType<'a> {
    Integer,
    Real,
    Boolean,
    Range(RangeBound<'a>, RangeBound<'a>),
    PackedFileOf(StringSpan<'a>),
    Array(WebArrayType<'a>),
    Record(WebRecordType<'a>),
    UserDefined(StringSpan<'a>),
    Pointer(Box<WebType<'a>>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RangeBound<'a> {
    Literal(PascalToken<'a>),
    Symbolic1(StringSpan<'a>),
    Symbolic2(StringSpan<'a>, PascalToken<'a>, PascalToken<'a>),
    UnarySymbolic(PascalToken<'a>, StringSpan<'a>),
}

pub fn parse_type<'a>(input: ParseInput<'a>) -> ParseResult<'a, WebType<'a>> {
    alt((
        named("integer", WebType::Integer),
        named("real", WebType::Real),
        named("boolean", WebType::Boolean),
        parse_pointer,
        parse_packed_file_of,
        parse_record,
        parse_array,
        parse_range,
        map(identifier, |s| WebType::UserDefined(s)),
    ))(input)
}

fn named<'a>(
    name: &'a str,
    value: WebType<'a>,
) -> impl Fn(ParseInput<'a>) -> ParseResult<'a, WebType<'a>> + 'a {
    move |input: ParseInput<'a>| {
        let (input, sv) = identifier(input)?;

        if sv.value == name {
            Ok((input, value.clone()))
        } else {
            new_parse_err(input, WebErrorKind::ExpectedIdentifier)
        }
    }
}

fn parse_pointer<'a>(input: ParseInput<'a>) -> ParseResult<'a, WebType<'a>> {
    map(tuple((pascal_token(PascalToken::Caret), parse_type)), |t| {
        WebType::Pointer(Box::new(t.1))
    })(input)
}

fn parse_range<'a>(input: ParseInput<'a>) -> ParseResult<'a, WebType<'a>> {
    map(
        tuple((
            parse_range_bound,
            pascal_token(PascalToken::DoubleDot),
            parse_range_bound,
        )),
        |t| WebType::Range(t.0, t.2),
    )(input)
}

fn parse_range_bound<'a>(input: ParseInput<'a>) -> ParseResult<'a, RangeBound<'a>> {
    alt((
        map(int_literal, |t| RangeBound::Literal(t)),
        map(merged_string_literals, |t| RangeBound::Literal(t)),
        parse_binary_range_bound,
        map(identifier, |i| RangeBound::Symbolic1(i)),
        parse_unary_range_bound,
    ))(input)
}

fn parse_unary_range_bound<'a>(input: ParseInput<'a>) -> ParseResult<'a, RangeBound<'a>> {
    map(
        tuple((
            alt((
                pascal_token(PascalToken::Plus),
                pascal_token(PascalToken::Minus),
            )),
            identifier,
        )),
        |t| RangeBound::UnarySymbolic(t.0, t.1),
    )(input)
}

/// This is for WEB range bounds that rely on math performed on @define
/// constants by the WEB preprocessor.
fn parse_binary_range_bound<'a>(input: ParseInput<'a>) -> ParseResult<'a, RangeBound<'a>> {
    map(
        tuple((
            identifier,
            alt((
                pascal_token(PascalToken::Plus),
                pascal_token(PascalToken::Minus),
            )),
            int_literal,
        )),
        |t| RangeBound::Symbolic2(t.0, t.1, t.2),
    )(input)
}

fn parse_packed_file_of<'a>(input: ParseInput<'a>) -> ParseResult<'a, WebType<'a>> {
    map(
        tuple((
            reserved_word(PascalReservedWord::Packed),
            reserved_word(PascalReservedWord::File),
            reserved_word(PascalReservedWord::Of),
            identifier,
        )),
        |t| WebType::PackedFileOf(t.3),
    )(input)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebArrayType<'a> {
    is_packed: bool,
    axes: Vec<Box<WebType<'a>>>,
    element: Box<WebType<'a>>,
}

fn parse_array<'a>(input: ParseInput<'a>) -> ParseResult<'a, WebType<'a>> {
    map(
        tuple((
            opt(reserved_word(PascalReservedWord::Packed)),
            reserved_word(PascalReservedWord::Array),
            pascal_token(PascalToken::OpenDelimiter(DelimiterKind::SquareBracket)),
            separated_list0(
                pascal_token(PascalToken::Comma),
                map(parse_type, |e| Box::new(e)),
            ),
            pascal_token(PascalToken::CloseDelimiter(DelimiterKind::SquareBracket)),
            reserved_word(PascalReservedWord::Of),
            map(parse_type, |e| Box::new(e)),
        )),
        |t| {
            WebType::Array(WebArrayType {
                is_packed: t.0.is_some(),
                axes: t.3,
                element: t.6,
            })
        },
    )(input)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebRecordType<'a> {
    is_packed: bool,
    fields: Vec<WebRecordField<'a>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebRecordField<'a> {
    names: Vec<PascalToken<'a>>,
    ty: Box<WebType<'a>>,
    comment: Option<WebComment<'a>>,
}

fn parse_record<'a>(input: ParseInput<'a>) -> ParseResult<'a, WebType<'a>> {
    map(
        tuple((
            opt(reserved_word(PascalReservedWord::Packed)),
            reserved_word(PascalReservedWord::Record),
            many1(parse_record_field),
            reserved_word(PascalReservedWord::End),
        )),
        |t| {
            WebType::Record(WebRecordType {
                is_packed: t.0.is_some(),
                fields: t.2,
            })
        },
    )(input)
}

fn parse_record_field<'a>(input: ParseInput<'a>) -> ParseResult<'a, WebRecordField<'a>> {
    map(
        tuple((
            separated_list1(pascal_token(PascalToken::Comma), identifier_as_token),
            pascal_token(PascalToken::Colon),
            parse_type,
            pascal_token(PascalToken::Semicolon),
            opt(comment),
        )),
        |t| WebRecordField {
            names: t.0,
            ty: Box::new(t.2),
            comment: t.4,
        },
    )(input)
}

// Prettifying

impl<'a> RenderInline for WebType<'a> {
    fn measure_inline(&self) -> usize {
        match self {
            WebType::Integer => 7,
            WebType::Real => 4,
            WebType::Boolean => 7,
            WebType::Range(blo, bhi) => blo.measure_inline() + bhi.measure_inline() + 4,
            WebType::PackedFileOf(t) => 15 + t.value.as_ref().len(),
            WebType::Array(arr) => arr.measure_inline(),
            WebType::Record(_rec) => prettify::NOT_INLINE,
            WebType::Pointer(ty) => 1 + ty.measure_inline(),
            WebType::UserDefined(s) => s.value.as_ref().len(),
        }
    }

    fn render_inline(&self, dest: &mut Prettifier) {
        match self {
            WebType::Integer => dest.noscope_push("integer"),
            WebType::Real => dest.noscope_push("real"),
            WebType::Boolean => dest.noscope_push("boolean"),

            WebType::Range(blo, bhi) => {
                blo.render_inline(dest);
                dest.noscope_push(" .. ");
                bhi.render_inline(dest);
            }

            WebType::PackedFileOf(t) => {
                dest.noscope_push("packed file of ");
                dest.noscope_push(t.value.as_ref());
            }

            WebType::Array(arr) => arr.render_inline(dest),
            WebType::Record(_rec) => dest.noscope_push("XXXrecordXXX"),

            WebType::Pointer(ty) => {
                dest.noscope_push("^");
                ty.render_inline(dest);
            }

            WebType::UserDefined(s) => dest.noscope_push(s.value.as_ref()),
        }
    }
}

impl<'a> WebType<'a> {
    pub fn render_flex(&self, dest: &mut Prettifier) {
        match self {
            WebType::Integer
            | WebType::Real
            | WebType::Boolean
            | WebType::UserDefined(_)
            | WebType::PackedFileOf(_)
            | WebType::Pointer(_)
            | WebType::Range(..) => self.render_inline(dest),

            WebType::Array(arr) => arr.render_flex(dest),
            WebType::Record(rec) => rec.render_flex(dest),
        }
    }
}

impl<'a> RenderInline for RangeBound<'a> {
    fn measure_inline(&self) -> usize {
        match self {
            RangeBound::Literal(t) => t.measure_inline(),
            RangeBound::Symbolic1(s) => s.value.as_ref().len(),
            RangeBound::Symbolic2(s1, op, s2) => {
                s1.value.as_ref().len() + op.measure_inline() + s2.measure_inline() + 4
            }
            RangeBound::UnarySymbolic(op, s) => op.measure_inline() + s.len(),
        }
    }

    fn render_inline(&self, dest: &mut Prettifier) {
        match self {
            RangeBound::Literal(t) => t.render_inline(dest),
            RangeBound::Symbolic1(s) => dest.noscope_push(s.value.as_ref()),
            RangeBound::Symbolic2(s1, op, s2) => {
                dest.noscope_push('(');
                dest.noscope_push(s1.value.as_ref());
                dest.space();
                op.render_inline(dest);
                dest.space();
                s2.render_inline(dest);
                dest.noscope_push(')');
            }
            RangeBound::UnarySymbolic(op, s) => {
                op.render_inline(dest);
                dest.noscope_push(s.value.as_ref());
            }
        }
    }
}

impl<'a> RenderInline for WebArrayType<'a> {
    fn measure_inline(&self) -> usize {
        let mut w = 0;

        if self.is_packed {
            w += 7;
        }

        w += 7; // "array ["
        w += prettify::measure_inline_seq(&self.axes, 2);
        w += 5; // "] of "
        w += self.element.measure_inline();
        w
    }

    fn render_inline(&self, dest: &mut Prettifier) {
        if self.is_packed {
            dest.noscope_push("packed ");
        }

        dest.noscope_push("array [");
        prettify::render_inline_seq(&self.axes, ", ", dest);
        dest.noscope_push("] of ");
        self.element.render_inline(dest);
    }
}

impl<'a> WebArrayType<'a> {
    pub fn render_flex(&self, dest: &mut Prettifier) {
        let wx = prettify::measure_inline_seq(&self.axes, 2);
        let we = self.element.measure_inline();

        if self.is_packed {
            dest.noscope_push("packed ");
        }

        dest.noscope_push("array [");

        if dest.fits(wx + we + 5) {
            prettify::render_inline_seq(&self.axes, ", ", dest);
            dest.noscope_push("] of ");
            self.element.render_inline(dest);
        } else if dest.fits(wx + 4) {
            prettify::render_inline_seq(&self.axes, ", ", dest);
            dest.noscope_push("] of");
            dest.indent_small();
            dest.newline_needed();
            self.element.render_flex(dest);
            dest.dedent_small();
        } else {
            dest.indent_small();

            for ax in &self.axes {
                dest.newline_indent();
                ax.render_flex(dest);
                dest.noscope_push(',');
            }

            dest.dedent_small();
            dest.newline_indent();
            dest.noscope_push("] of");
            dest.noscope_push(' ');

            if dest.fits(we) {
                self.element.render_inline(dest);
            } else {
                self.element.render_flex(dest);
            }
        }
    }
}

impl<'a> WebRecordType<'a> {
    pub fn render_flex(&self, dest: &mut Prettifier) {
        if self.is_packed {
            dest.noscope_push("packed ");
        }

        dest.noscope_push("record {");
        dest.indent_block();

        for f in &self.fields {
            dest.newline_needed();

            let wc = f
                .comment
                .as_ref()
                .map(|c| c.measure_inline() + 1)
                .unwrap_or(0);

            let wn = prettify::measure_inline_seq(&f.names, 2);

            if dest.fits(wn + f.ty.measure_inline() + wc + 3) {
                prettify::render_inline_seq(&f.names, ", ", dest);
                dest.noscope_push(": ");
                f.ty.render_inline(dest);
                dest.noscope_push(',');

                if let Some(c) = f.comment.as_ref() {
                    dest.space();
                    c.render_inline(dest);
                }
            } else if dest.fits(wn + f.ty.measure_inline() + 3) {
                if let Some(c) = f.comment.as_ref() {
                    c.render_inline(dest);
                    dest.newline_needed();
                }

                prettify::render_inline_seq(&f.names, ", ", dest);
                dest.noscope_push(": ");
                f.ty.render_inline(dest);
                dest.noscope_push(',');
            } else {
                if let Some(c) = f.comment.as_ref() {
                    c.render_inline(dest);
                    dest.newline_needed();
                }

                prettify::render_inline_seq(&f.names, ", ", dest);
                dest.noscope_push(": ");
                f.ty.render_flex(dest);
                dest.noscope_push(',');
            }
        }

        dest.dedent_block();
        dest.newline_indent();
        dest.noscope_push('}');
    }
}
