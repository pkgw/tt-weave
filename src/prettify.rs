//! Prettify the Pascal source.

use lazy_static::lazy_static;
use std::{
    fmt::{self, Write},
    ops::Deref,
    str::FromStr,
};
use syntect::{
    highlighting::{Color, FontStyle, HighlightIterator, HighlightState, Highlighter, Theme},
    parsing::{Scope, ScopeStack, ScopeStackOp},
};

use crate::weblang::base::{ModuleId, SpanValue};

// See https://www.sublimetext.com/docs/scope_naming.html for some scope hints.

const INITIAL_SCOPES: &str = "source.c";

lazy_static! {
    pub static ref KEYWORD_SCOPE: Scope = Scope::new("keyword.control.c").unwrap();
    pub static ref COMMENT_SCOPE: Scope = Scope::new("comment.line.c").unwrap();
    pub static ref STRING_LITERAL_SCOPE: Scope = Scope::new("string.quoted.double").unwrap();
    pub static ref HEX_LITERAL_SCOPE: Scope =
        Scope::new("constant.numeric.integer.hexadecimal").unwrap();
    pub static ref DECIMAL_LITERAL_SCOPE: Scope =
        Scope::new("constant.numeric.integer.decimal").unwrap();
    pub static ref FLOAT_LITERAL_SCOPE: Scope = Scope::new("constant.numeric.float").unwrap();
    pub static ref LABEL_NAME_SCOPE: Scope = Scope::new("entity.name.label").unwrap();
}

const WIDTH: usize = 60;

#[derive(Clone, Debug)]
pub struct Prettifier {
    full_width: usize,
    indent: usize,
    remaining_width: usize,
    newline_needed: bool,
    text: String,
    ops: Vec<(usize, ScopeStackOp)>,

    /// Along with the "scope ops" used by syntect to prettify, we maintain a
    /// list of "inserts" of TeX text used to mark up the code with features
    /// that go beyond colorizing. We handle inserts this way so that the
    /// prettifier can look at the length of `text` to properly understand the
    /// alignment of the underlying code.
    ///
    /// The offset here is measured in bytes so that we can determine the right
    /// offset during prettification by looking at `text.len()`, which is
    /// measured in bytes.
    inserts: Vec<(usize, TexInsert)>,
}

impl Prettifier {
    pub fn new() -> Self {
        Prettifier {
            full_width: WIDTH,
            indent: 0,
            remaining_width: WIDTH,
            newline_needed: false,
            text: String::default(),
            ops: Vec::default(),
            inserts: Vec::default(),
        }
    }

    #[inline(always)]
    pub fn fits(&self, width: usize) -> bool {
        let eff_width = if self.newline_needed {
            self.full_width - self.indent
        } else {
            self.remaining_width
        };

        width <= eff_width
    }

    pub fn would_fit_on_new_line(&self, width: usize) -> bool {
        width <= self.full_width - self.indent
    }

    pub fn indent_block(&mut self) -> bool {
        if self.full_width - self.indent > 4 {
            self.indent += 4;
            true
        } else {
            false
        }
    }

    pub fn dedent_block(&mut self) -> bool {
        if self.indent > 3 {
            self.indent -= 4;
            true
        } else {
            false
        }
    }

    pub fn indent_small(&mut self) -> bool {
        if self.full_width - self.indent > 2 {
            self.indent += 2;
            true
        } else {
            false
        }
    }

    pub fn dedent_small(&mut self) -> bool {
        if self.indent > 1 {
            self.indent -= 2;
            true
        } else {
            false
        }
    }

    pub fn newline_indent(&mut self) {
        self.text.push('\n');

        for _ in 0..self.indent {
            self.text.push(' ');
        }

        self.newline_needed = false;
        self.remaining_width = self.full_width - self.indent;
    }

    pub fn newline_needed(&mut self) {
        self.newline_needed = true;
    }

    #[inline(always)]
    fn maybe_newline(&mut self) {
        if self.newline_needed {
            self.newline_indent();
        }
    }

    pub fn scope_push<S: fmt::Display>(&mut self, scope: Scope, text: S) {
        self.maybe_newline();

        let n0 = self.text.len();
        self.ops.push((n0, ScopeStackOp::Push(scope)));
        write!(self.text, "{}", text).unwrap();
        let n1 = self.text.len();
        self.ops.push((n1, ScopeStackOp::Pop(1)));
        self.remaining_width = self.remaining_width.saturating_sub(n1 - n0);
    }

    pub fn with_scope<F: FnOnce(&mut Self)>(&mut self, scope: Scope, func: F) {
        let n0 = self.text.len();
        self.ops.push((n0, ScopeStackOp::Push(scope)));
        func(self);
        let n1 = self.text.len();
        self.ops.push((n1, ScopeStackOp::Pop(1)));
    }

    pub fn keyword<S: fmt::Display>(&mut self, text: S) {
        self.scope_push(*KEYWORD_SCOPE, text)
    }

    pub fn noscope_push<S: fmt::Display>(&mut self, text: S) {
        // TODO: never use this? Should always have some kine of scope?
        self.maybe_newline();
        let n0 = self.text.len();
        write!(self.text, "{}", text).unwrap();
        let n1 = self.text.len();
        self.remaining_width = self.remaining_width.saturating_sub(n1 - n0);
    }

    pub fn space(&mut self) {
        self.text.push(' ');
        self.remaining_width = self.remaining_width.saturating_sub(1);
    }

    pub fn toplevel_separator(&mut self) {
        self.text.push('\n');
        self.newline_indent();
    }

    /// Add a "TeX insert".
    ///
    /// Set `text_next` to true if text will be insert immediately after this
    /// insert. This makes sure to apply a newline and indent if needed, so that
    /// there is no space between the insert and the following text.
    pub fn insert(&mut self, ins: TexInsert, text_next: bool) {
        if text_next {
            self.maybe_newline();
        }

        self.inserts.push((self.text.len(), ins));
    }

    /// Handle inserts outside of the colorized styling.
    ///
    /// This is needed to deal with the XeTeX array macro hack. And maybe other
    /// things in the future?
    fn handle_outer_inserts(
        &self,
        i_text: usize,
        mut insert_idx: usize,
        mut i_next_insert: usize,
    ) -> (usize, usize) {
        while i_text == i_next_insert {
            match self.inserts[insert_idx].1 {
                // Macro hack marker specially handled at top of emit()
                TexInsert::XetexArrayMacroHackMarker => {}

                TexInsert::XetexArrayMacroHackBracket => {
                    print!("]");
                }

                _ => break,
            }

            // Prep for the next insert.
            insert_idx += 1;
            i_next_insert = self
                .inserts
                .get(insert_idx)
                .map(|t| t.0)
                .unwrap_or(usize::MAX);
        }

        (insert_idx, i_next_insert)
    }

    /// Handle the inserts at the given text position.
    ///
    /// There may be 0, 1, or many to handle.
    fn handle_inserts(
        &self,
        i_text: usize,
        mut insert_idx: usize,
        mut i_next_insert: usize,
    ) -> (usize, usize) {
        while i_text == i_next_insert {
            match self.inserts[insert_idx].1 {
                TexInsert::StartModuleReference(id) => {
                    print!("\\WebModuleReference{{{}}}{{", id);
                }

                TexInsert::EndMacro => {
                    print!("}}");
                }

                // Break on "outer" inserts so as not to eat them.
                TexInsert::XetexArrayMacroHackMarker | TexInsert::XetexArrayMacroHackBracket => {
                    break
                }
            }

            // Prep for the next insert.
            insert_idx += 1;
            i_next_insert = self
                .inserts
                .get(insert_idx)
                .map(|t| t.0)
                .unwrap_or(usize::MAX);
        }

        (insert_idx, i_next_insert)
    }

    pub fn emit(self, theme: &Theme, inline: bool) {
        let highlighter = Highlighter::new(theme);
        let initial_stack = ScopeStack::from_str(INITIAL_SCOPES).unwrap();
        let mut hs = HighlightState::new(&highlighter, initial_stack);
        let hi = HighlightIterator::new(&mut hs, &self.ops[..], &self.text[..], &highlighter);
        let mut insert_idx = 0;
        let mut i_text = 0;

        let xetex_array_macro_hack =
            !self.inserts.is_empty() && self.inserts[0].1.is_xetex_array_macro_hack_marker();

        let (env, terminator) = if inline {
            ("WebPrettifiedInline", "")
        } else {
            ("WebPrettifiedDisplay", "%\n")
        };

        if xetex_array_macro_hack {
            insert_idx += 1;
            println!("$[\\WebBeginXetexArrayMacro{{}}%");
        } else {
            println!("\\begin{{{}}}%", env);
        }

        let mut i_next_insert = self
            .inserts
            .get(insert_idx)
            .map(|t| t.0)
            .unwrap_or(usize::MAX);

        for (style, span) in hi {
            (insert_idx, i_next_insert) =
                self.handle_outer_inserts(i_text, insert_idx, i_next_insert);

            print!(
                "\\S{{{}}}{{{}}}{{",
                ColorHexConvert(style.foreground),
                ColorHexConvert(style.background)
            );

            if style.font_style.intersects(FontStyle::BOLD) {
                print!("\\bf");
            }

            if style.font_style.intersects(FontStyle::ITALIC) {
                print!("\\it");
            }

            if style.font_style.intersects(FontStyle::UNDERLINE) {
                print!("\\ul");
            }

            print!("}}{{");

            for c in span.chars() {
                (insert_idx, i_next_insert) =
                    self.handle_inserts(i_text, insert_idx, i_next_insert);

                match c {
                    '$' => print!("\\$"),
                    '%' => print!("\\%"),
                    '^' => print!("\\^"),
                    '_' => print!("\\_"),
                    '{' => print!("\\{{"),
                    '}' => print!("\\}}"),
                    '#' => print!("\\#"),
                    '\\' => print!("{{\\textbackslash}}"),
                    '&' => print!("\\&"),
                    '~' => print!("{{\\textasciitilde}}"),
                    ' ' => print!("\\ "),
                    '\n' => print!("\\WebNL\n"),
                    other => print!("{}", other),
                }

                i_text += c.len_utf8();
            }

            (insert_idx, i_next_insert) = self.handle_inserts(i_text, insert_idx, i_next_insert);
            print!("}}");
        }

        self.handle_outer_inserts(i_text, insert_idx, i_next_insert);
        println!("%");

        if xetex_array_macro_hack {
            println!("\\WebEndXetexArrayMacro$%");
        } else {
            print!("\\end{{{}}}{}", env, terminator);
        }
    }
}

/// A trait for measuring how wide some WEB language items that can be rendered
/// in a fully "inline" format.
pub trait RenderInline {
    /// Get how many characters wide the item would be if render all on one
    /// line. Return `NOT_INLINE` if the item should not be rendered in an
    /// inline mode.
    fn measure_inline(&self) -> usize;

    /// Render the item in its inline format.
    fn render_inline(&self, dest: &mut Prettifier);
}

pub const NOT_INLINE: usize = 9999;

impl<T: RenderInline> RenderInline for Box<T> {
    fn measure_inline(&self) -> usize {
        self.deref().measure_inline()
    }

    fn render_inline(&self, dest: &mut Prettifier) {
        self.deref().render_inline(dest)
    }
}

impl<T: RenderInline> RenderInline for &T {
    fn measure_inline(&self) -> usize {
        self.deref().measure_inline()
    }

    fn render_inline(&self, dest: &mut Prettifier) {
        self.deref().render_inline(dest)
    }
}

impl<'a, T: RenderInline> RenderInline for SpanValue<'a, T> {
    fn measure_inline(&self) -> usize {
        self.value.measure_inline()
    }

    fn render_inline(&self, dest: &mut Prettifier) {
        self.value.render_inline(dest)
    }
}

/// Measure how wide a sequence of items will be if rendered inline.
///
/// The items are assumed to be rendered with a separator of width `sep_width`.
pub fn measure_inline_seq<I: IntoIterator<Item = T>, T: RenderInline>(
    seq: I,
    sep_width: usize,
) -> usize {
    let mut n = 0;

    for item in seq.into_iter() {
        if n != 0 {
            n += sep_width;
        }

        n += item.measure_inline();
    }

    n
}

/// Render a sequence of items inline.
pub fn render_inline_seq<I: IntoIterator<Item = T>, T: RenderInline>(
    seq: I,
    sep: &str,
    dest: &mut Prettifier,
) {
    let mut first = true;

    for item in seq.into_iter() {
        if first {
            first = false;
        } else {
            dest.noscope_push(sep);
        }

        item.render_inline(dest);
    }
}

#[derive(Clone, Debug)]
pub enum TexInsert {
    /// Insert the beginning of a macro that wraps a reference to a WEB module.
    /// This should be followed by an EndMacro.
    StartModuleReference(ModuleId),

    /// Insert the ending of a macro -- i.e., a closing brace.
    EndMacro,

    /// Should be inserted at offset zero. Indicates that the hack for
    /// XeTeX(2022.0):576 is active, and we need to emit special delimiters to
    /// make the output compatible with the \arr macro used in the \halign
    /// there.
    XetexArrayMacroHackMarker,

    /// The other component of the XeTeX array macro hack: inserts an unescaped `]`
    /// at the specified position.
    XetexArrayMacroHackBracket,
}

impl TexInsert {
    pub fn is_xetex_array_macro_hack_marker(&self) -> bool {
        if let TexInsert::XetexArrayMacroHackMarker = self {
            true
        } else {
            false
        }
    }
}

struct ColorHexConvert(Color);

impl fmt::Display for ColorHexConvert {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "rgba({},{},{},{:.2})",
            self.0.r,
            self.0.g,
            self.0.b,
            self.0.a as f32 / 255.0
        )
    }
}
