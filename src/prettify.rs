//! Prettify the Pascal source.

use lazy_static::lazy_static;
use std::{
    fmt::{self, Write},
    str::FromStr,
};
use syntect::{
    highlighting::{Color, FontStyle, HighlightIterator, HighlightState, Highlighter, Theme},
    parsing::{Scope, ScopeStack, ScopeStackOp},
};

use crate::weblang::base::StringSpan;

const INITIAL_SCOPES: &str = "source.c";

lazy_static! {
    pub static ref KEYWORD_SCOPE: Scope = Scope::new("keyword.control.c").unwrap();
}

#[derive(Clone, Debug)]
pub struct Prettifier {
    indent: usize,
    remaining_width: usize,
    is_inline: bool,
    newline_needed: bool,
    text: String,
    ops: Vec<(usize, ScopeStackOp)>,
}

impl Prettifier {
    pub fn new_inline(is_inline: bool) -> Self {
        Prettifier {
            indent: 0,
            remaining_width: 60,
            is_inline,
            newline_needed: false,
            text: String::default(),
            ops: Vec::default(),
        }
    }

    #[inline(always)]
    pub fn fits(&self, width: usize) -> bool {
        width <= self.remaining_width
    }

    pub fn indent_block(&mut self) {
        if self.remaining_width > 4 {
            self.indent += 4;
            self.remaining_width -= 4;
        }
    }

    pub fn dedent_block(&mut self) {
        if self.indent > 3 {
            self.indent -= 4;
            self.remaining_width += 4;
        }
    }

    pub fn indent_small(&mut self) {
        if self.remaining_width > 2 {
            self.indent += 2;
            self.remaining_width -= 2;
        }
    }

    pub fn dedent_small(&mut self) {
        if self.indent > 1 {
            self.indent -= 2;
            self.remaining_width += 2;
        }
    }

    pub fn newline_indent(&mut self) {
        self.text.push('\n');

        for _ in 0..self.indent {
            self.text.push(' ');
        }

        self.newline_needed = false;
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

    pub fn scope_push<S: fmt::Display>(&mut self, scope: Scope, text: S) -> usize {
        self.maybe_newline();

        let n0 = self.text.len();
        self.ops.push((n0, ScopeStackOp::Push(scope)));
        write!(self.text, "{}", text).unwrap();
        let n1 = self.text.len();
        self.ops.push((n1, ScopeStackOp::Pop(1)));
        n1 - n0
    }

    pub fn noscope_push<S: fmt::Display>(&mut self, text: S) {
        // TODO: never use this? Should always have some kine of scope?
        self.maybe_newline();
        write!(self.text, "{}", text).unwrap();
    }

    pub fn space(&mut self) {
        self.text.push(' ');
    }

    pub fn toplevel_separator(&mut self) {
        self.text.push('\n');
        self.newline_indent();
    }

    pub fn emit(self, theme: &Theme, inline: bool) {
        let highlighter = Highlighter::new(theme);
        let initial_stack = ScopeStack::from_str(INITIAL_SCOPES).unwrap();
        let mut hs = HighlightState::new(&highlighter, initial_stack);
        let hi = HighlightIterator::new(&mut hs, &self.ops[..], &self.text[..], &highlighter);

        let (env, terminator) = if inline {
            ("WebPrettifiedInline", "")
        } else {
            ("WebPrettifiedDisplay", "%\n")
        };

        println!("\\begin{{{}}}%", env);

        for (style, span) in hi {
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
                    '\n' => print!("\\WebNL\n"), // XXXXXXXXXXXXx
                    other => print!("{}", other),
                }
            }

            print!("}}");
        }

        println!("%");
        print!("\\end{{{}}}{}", env, terminator);
    }
}

pub fn module_reference_measure_inline<'a>(mr: &StringSpan<'a>) -> usize {
    mr.value.as_ref().len() + 4
}

pub fn module_reference_render<'a>(mr: &StringSpan<'a>, dest: &mut Prettifier) {
    dest.noscope_push("< ");
    dest.noscope_push(mr.value.as_ref());
    dest.noscope_push(" >");
}

#[derive(Debug, Default)]
pub struct PrettifiedCode {}

impl PrettifiedCode {}

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
