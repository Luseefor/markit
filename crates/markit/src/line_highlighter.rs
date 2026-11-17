//! Incremental grammar highlighting for editors. Lines are parsed only when
//! their tokens are asked for, and an edit drops the cached parse from the
//! edited line on instead of re-reading the file.

use std::rc::Rc;

use syntect::parsing::SyntaxReference;

use crate::{
    grammar::{
        append_line_tokens, highlighters, initial, parse_line_at, plain_token, syntax_for,
        syntaxes, Boundary, ParsedLine,
    },
    highlight::{language_alias, CodeToken},
};

/// A whole file split into lines, with a parse checkpoint after each one.
pub struct LineHighlighter {
    grammar: Option<&'static SyntaxReference>,
    lines: Vec<String>,
    parsed: Vec<Option<Rc<ParsedLine>>>,
    initial: Option<Rc<Boundary>>,
    newline: &'static str,
    trailing_newline: bool,
    valid: usize,
}

impl LineHighlighter {
    /// Creates a highlighter for `language`. Unknown names, like `""`, keep
    /// every line as plain text.
    pub fn new(language: &str) -> LineHighlighter {
        let language = language_alias(language);
        let grammar = if language.is_empty() {
            None
        } else {
            syntax_for(&language)
        };
        let initial = grammar.map(initial);
        LineHighlighter {
            grammar,
            lines: Vec::new(),
            parsed: Vec::new(),
            initial,
            newline: "\n",
            trailing_newline: false,
            valid: 0,
        }
    }

    /// Replaces the text. `\n` and `\r\n` both separate lines; a final line
    /// ending is remembered so the last line parses like it does in a file.
    pub fn set_text(&mut self, text: &str) {
        self.newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
        self.trailing_newline = text.ends_with('\n');
        self.lines = if text.is_empty() {
            Vec::new()
        } else {
            let body = text.strip_suffix('\n').unwrap_or(text);
            body.split('\n')
                .map(|line| line.strip_suffix('\r').unwrap_or(line).to_owned())
                .collect()
        };
        self.parsed = vec![None; self.lines.len()];
        self.valid = 0;
    }

    /// Replaces `removed_lines` lines at `start_line` with `inserted`, none of
    /// which carry a line ending. Cached parses from `start_line` on are no
    /// longer trusted, though an unchanged suffix may be reused once the new
    /// parse reaches a boundary equal to its old one.
    pub fn edit(&mut self, start_line: usize, removed_lines: usize, inserted: &[&str]) {
        let old_len = self.lines.len();
        let start = start_line.min(old_len);
        let removed = removed_lines.min(old_len - start);
        if removed == 0 && inserted.is_empty() {
            return;
        }
        if removed == 0 && start == old_len {
            self.trailing_newline = false;
        }
        let replacement: Vec<String> = inserted
            .iter()
            .map(|line| line.strip_suffix('\r').unwrap_or(line).to_owned())
            .collect();
        self.parsed
            .splice(start..start + removed, replacement.iter().map(|_| None));
        self.lines.splice(start..start + removed, replacement);
        self.valid = self.valid.min(start);
    }

    /// Tokens for the lines in `start_line..end_line`, one list per line.
    /// Tokens cover a line's text without its line ending. Lines are parsed
    /// lazily, starting from the last line whose boundary is known.
    pub fn tokens(&mut self, start_line: usize, end_line: usize) -> Vec<Vec<CodeToken>> {
        let start = start_line.min(self.lines.len());
        let end = end_line.min(self.lines.len());
        if end <= start {
            return Vec::new();
        }
        if self.parse_to(end).is_none() {
            return (start..end).map(|index| self.plain_line(index)).collect();
        }
        (start..end)
            .map(|index| {
                let mut tokens = Vec::new();
                if let Some(line) = self.parsed[index].as_ref() {
                    append_line_tokens(&mut tokens, line, self.lines[index].len());
                }
                tokens
            })
            .collect()
    }

    /// Parses lines from `valid` up to `end`, reusing a cached line whenever
    /// its text and before-boundary still match.
    fn parse_to(&mut self, end: usize) -> Option<()> {
        self.grammar?;
        let syntaxes = syntaxes();
        let (light, dark) = highlighters();
        let mut state = if self.valid > 0 {
            self.parsed.get(self.valid - 1)?.as_ref()?.after.clone()
        } else {
            self.initial.as_ref()?.clone()
        };
        let mut index = self.valid;
        while index < end {
            let cached = self.parsed.get(index).and_then(Clone::clone);
            if let Some(cached) = cached {
                if cached.before == state && cached.text == self.line_text(index) {
                    state = cached.after.clone();
                    self.valid = index + 1;
                    index += 1;
                    continue;
                }
            }
            #[cfg(test)]
            PARSED_LINES.with(|lines| lines.set(lines.get() + 1));
            let parsed =
                parse_line_at(index, &self.line_text(index), &state, syntaxes, light, dark)?;
            state = parsed.after.clone();
            self.parsed[index] = Some(parsed);
            self.valid = index + 1;
            index += 1;
        }
        Some(())
    }

    fn line_text(&self, index: usize) -> String {
        let mut text = String::with_capacity(self.lines[index].len() + self.newline.len());
        text.push_str(&self.lines[index]);
        if index + 1 < self.lines.len() || self.trailing_newline {
            text.push_str(self.newline);
        }
        text
    }

    fn plain_line(&self, index: usize) -> Vec<CodeToken> {
        let line = self.lines[index].as_str();
        if line.is_empty() {
            Vec::new()
        } else {
            vec![plain_token(line)]
        }
    }
}

#[cfg(test)]
std::thread_local! {
    static PARSED_LINES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn parsed_line_count() -> usize {
    PARSED_LINES.with(std::cell::Cell::get)
}

#[cfg(test)]
mod tests;
