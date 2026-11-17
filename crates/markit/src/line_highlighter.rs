//! Incremental grammar highlighting for editors. Lines are parsed only when
//! their tokens are asked for, and an edit drops the cached parse from the
//! edited line on instead of re-reading the file.

use std::sync::Arc;

use syntect::parsing::SyntaxReference;

use crate::{
    grammar::{
        append_line_tokens, hash_text, highlighters, initial, parse_line_at, plain_line,
        plain_token, syntax_for, syntaxes, Boundary, ParsedLine,
    },
    highlight::{language_alias, CodeToken},
};

/// Lines longer than this are kept as one plain run, as `highlight` does.
const MAX_LINE: usize = 2048;
const DEFAULT_BUDGET: usize = 2000;

struct Line {
    text: String,
    parsed: Option<Arc<ParsedLine>>,
}

/// A whole file split into lines, with a parse checkpoint after each one.
/// It can be moved between threads, but only one thread may call it at a time.
pub struct LineHighlighter {
    grammar: Option<&'static SyntaxReference>,
    lines: Vec<Line>,
    initial: Option<Arc<Boundary>>,
    valid: usize,
    budget: usize,
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
            initial,
            valid: 0,
            budget: DEFAULT_BUDGET,
        }
    }

    /// Replaces the text. Lines split on `\n` only; a `\r` before a `\n` is
    /// part of that line's ending, and a lone `\r` stays in the line. A
    /// trailing `\n` therefore adds an empty last line.
    pub fn set_text(&mut self, text: &str) {
        let count = text.bytes().filter(|byte| *byte == b'\n').count() + 1;
        let mut lines = Vec::with_capacity(count);
        let mut parts = text.split('\n').peekable();
        while let Some(part) = parts.next() {
            let mut line = String::with_capacity(part.len() + usize::from(parts.peek().is_some()));
            line.push_str(part);
            if parts.peek().is_some() {
                line.push('\n');
            }
            lines.push(Line {
                text: line,
                parsed: None,
            });
        }
        self.lines = lines;
        self.valid = 0;
    }

    /// Replaces `removed_lines` lines at `start_line` with `inserted`, none of
    /// which carry a line ending. Entries containing `\n` become several
    /// lines. Cached parses from the edit on are no longer trusted, though an
    /// unchanged suffix is reused once the new parse reaches a matching
    /// before-boundary.
    pub fn edit(&mut self, start_line: usize, removed_lines: usize, inserted: &[&str]) {
        let old_len = self.lines.len();
        let start = start_line.min(old_len);
        let removed = removed_lines.min(old_len - start);
        if removed == 0 && inserted.is_empty() {
            return;
        }
        let at_end = start + removed == old_len;
        let replacement = inserted_lines(inserted, at_end);
        let mut invalid = start;
        if at_end && start > 0 {
            let previous = &mut self.lines[start - 1];
            let content = visible_len(&previous.text);
            if replacement.is_empty() {
                previous.text.truncate(content);
            } else if removed == 0 {
                previous.text.push('\n');
            }
            previous.parsed = None;
            invalid = start - 1;
        }
        self.lines.splice(start..start + removed, replacement);
        if self.lines.is_empty() {
            self.lines.push(Line {
                text: String::new(),
                parsed: None,
            });
        }
        self.valid = self.valid.min(invalid);
    }

    /// Tokens for the lines in `start_line..end_line`, one list per line.
    /// Tokens cover a line's text without its line ending. Lines are parsed
    /// lazily, starting from the last line whose boundary is known.
    ///
    /// At most [`budget`](Self::set_budget) lines are parsed per call. Lines
    /// past that budget come back as plain tokens; call
    /// [`advance`](Self::advance) to catch up.
    pub fn tokens(&mut self, start_line: usize, end_line: usize) -> Vec<Vec<CodeToken>> {
        let start = start_line.min(self.lines.len());
        let end = end_line.min(self.lines.len());
        if end <= start {
            return Vec::new();
        }
        let budget = self.budget;
        if self.parse_to(end, budget).is_none() {
            return (start..end).map(|index| self.plain_line(index)).collect();
        }
        (start..end)
            .map(|index| {
                if index < self.valid {
                    self.token_line(index)
                } else {
                    self.plain_line(index)
                }
            })
            .collect()
    }

    /// Sets how many lines one [`tokens`](Self::tokens) call may parse.
    pub fn set_budget(&mut self, budget: usize) {
        self.budget = budget;
    }

    /// Parses up to `max_lines` more lines without asking for tokens. Returns
    /// true while more lines still need parsing.
    pub fn advance(&mut self, max_lines: usize) -> bool {
        if self.grammar.is_none() {
            return false;
        }
        let _ = self.parse_to(self.lines.len(), max_lines);
        self.valid < self.lines.len()
    }

    /// Approximate heap bytes held for line text and parse checkpoints.
    pub fn memory_bytes(&self) -> usize {
        let mut bytes = self.lines.capacity() * std::mem::size_of::<Line>();
        for line in &self.lines {
            bytes += line.text.capacity();
            if let Some(parsed) = &line.parsed {
                bytes += std::mem::size_of::<Arc<ParsedLine>>() + parsed.memory_bytes();
            }
        }
        bytes
    }

    /// Parses lines from `valid` up to `end`, reusing a cached line whenever
    /// its text and before-boundary still match. `max_parses` bounds the lines
    /// that need real work; cache hits do not consume it.
    fn parse_to(&mut self, end: usize, max_parses: usize) -> Option<()> {
        self.grammar?;
        let syntaxes = syntaxes();
        let (light, dark) = highlighters();
        let mut state = if self.valid > 0 {
            self.lines
                .get(self.valid - 1)?
                .parsed
                .as_ref()?
                .after
                .clone()
        } else {
            self.initial.as_ref()?.clone()
        };
        let mut index = self.valid;
        let mut parsed_count = 0;
        while index < end {
            let text = self.lines[index].text.as_str();
            let cached = self.lines[index].parsed.clone();
            if let Some(cached) = cached {
                if cached.before == state
                    && cached.len == text.len()
                    && cached.hash == hash_text(text)
                {
                    state = cached.after.clone();
                    self.valid = index + 1;
                    index += 1;
                    continue;
                }
            }
            if parsed_count >= max_parses {
                break;
            }
            #[cfg(test)]
            PARSED_LINES.with(|lines| lines.set(lines.get() + 1));
            let parsed = if visible_len(text) > MAX_LINE {
                plain_line(text, &state)
            } else {
                parse_line_at(index, text, &state, syntaxes, light, dark)?
            };
            parsed_count += 1;
            state = parsed.after.clone();
            self.lines[index].parsed = Some(parsed);
            self.valid = index + 1;
            index += 1;
        }
        Some(())
    }

    fn token_line(&self, index: usize) -> Vec<CodeToken> {
        let Some(line) = self.lines[index].parsed.as_ref() else {
            return self.plain_line(index);
        };
        let text = self.lines[index].text.as_str();
        let mut tokens = Vec::new();
        append_line_tokens(&mut tokens, text, visible_len(text), line);
        tokens
    }

    fn plain_line(&self, index: usize) -> Vec<CodeToken> {
        let text = self.lines[index].text.as_str();
        let content = &text[..visible_len(text)];
        if content.is_empty() {
            Vec::new()
        } else {
            vec![plain_token(content)]
        }
    }
}

fn visible_len(text: &str) -> usize {
    if text.ends_with("\r\n") {
        text.len() - 2
    } else if text.ends_with('\n') {
        text.len() - 1
    } else {
        text.len()
    }
}

fn inserted_lines(inserted: &[&str], at_end: bool) -> Vec<Line> {
    if inserted.is_empty() {
        return Vec::new();
    }
    let joined = inserted.join("\n");
    let parts: Vec<&str> = joined.split('\n').collect();
    let count = parts.len();
    parts
        .into_iter()
        .enumerate()
        .map(|(index, part)| {
            let mut text = String::with_capacity(part.len() + 1);
            text.push_str(part);
            if index + 1 < count || !at_end {
                text.push('\n');
            }
            Line { text, parsed: None }
        })
        .collect()
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
