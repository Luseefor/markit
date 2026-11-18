//! Incremental grammar highlighting for editors. Parser state is kept at
//! sparse checkpoints and style runs only for a bounded window of recent
//! lines, so a large file costs bytes per line rather than a full parse tree.
//!
//! `Boundary` contains onig match regions, whose raw pointers make it neither
//! `Send` nor `Sync`; the handle is marked `Send` at the C boundary and used by
//! one thread at a time, so atomic checkpoints are still the right sharing type.
#![allow(clippy::arc_with_non_send_sync)]

use std::{collections::BTreeMap, mem, sync::Arc};

use syntect::parsing::SyntaxReference;

use crate::{
    grammar::{hash_text, initial, syntax_for, Boundary},
    highlight::{language_alias, CodeToken},
};

mod cache;
mod parse;

use cache::{
    checkpoint_bytes, inserted_lines, line_ranges, plain_text, visible_len, Checkpoint, Line,
    RunCache,
};

/// Lines longer than this are kept as one plain run, as `highlight` does.
const MAX_LINE: usize = 2048;
const DEFAULT_BUDGET: usize = 2000;
const CHECKPOINT_LINES: usize = 32;

struct ParsePlan {
    line: usize,
    offset: usize,
    end: usize,
    max_parses: usize,
    cache_from: usize,
    cache_to: usize,
}

/// A whole file split into lines, with sparse parser checkpoints and a
/// bounded run cache. It can be moved between threads, but only one thread
/// may call it at a time.
pub struct LineHighlighter {
    grammar: Option<&'static SyntaxReference>,
    text: String,
    lines: Vec<Line>,
    initial: Option<Arc<Boundary>>,
    checkpoints: BTreeMap<usize, Checkpoint>,
    stale: BTreeMap<usize, Checkpoint>,
    frontier: Option<(usize, usize, Arc<Boundary>)>,
    parsed_through: usize,
    anchor: Option<usize>,
    runs: RunCache,
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
            text: String::new(),
            lines: Vec::new(),
            initial,
            checkpoints: BTreeMap::new(),
            stale: BTreeMap::new(),
            frontier: None,
            parsed_through: 0,
            anchor: None,
            runs: RunCache::new(),
            budget: DEFAULT_BUDGET,
        }
    }

    /// Replaces the text. Lines split on `\n` only; a `\r` before a `\n` is
    /// part of that line's ending, and a lone `\r` stays in the line. A
    /// trailing `\n` therefore adds an empty last line.
    pub fn set_text(&mut self, text: &str) {
        self.text.clear();
        self.text.push_str(text);
        self.lines = line_ranges(text);
        self.checkpoints.clear();
        self.stale.clear();
        self.frontier = None;
        self.parsed_through = 0;
        self.anchor = None;
        self.runs.clear();
    }

    /// Replaces `removed_lines` lines at `start_line` with `inserted`, none of
    /// which carry a line ending. Entries containing `\n` become several
    /// lines. Cached parses from the edit on are no longer trusted, though an
    /// unchanged suffix is reused once a checkpoint boundary matches.
    pub fn edit(&mut self, start_line: usize, removed_lines: usize, inserted: &[&str]) {
        let old_len = self.lines.len();
        let start = start_line.min(old_len);
        let removed = removed_lines.min(old_len - start);
        if removed == 0 && inserted.is_empty() {
            return;
        }
        let at_end = start + removed == old_len;
        let (inserted_text, lengths) = inserted_lines(inserted, at_end);
        let (splice_start, splice_end, prefix, previous_changed) =
            self.splice_for(start, removed, at_end, &lengths);
        let removed_bytes = splice_end - splice_start;
        let mut replacement = String::with_capacity(prefix + inserted_text.len());
        if prefix > 0 {
            replacement.push('\n');
        }
        replacement.push_str(&inserted_text);
        let delta_text = replacement.len() as isize - removed_bytes as isize;
        let delta_lines = lengths.len() as isize - removed as isize;
        self.text
            .replace_range(splice_start..splice_end, &replacement);
        if previous_changed && start > 0 {
            let offset = self.line_offset(start - 1);
            let previous = &mut self.lines[start - 1];
            previous.len = splice_start + prefix - offset;
            previous.hash = hash_text(&self.text[offset..offset + previous.len]);
        }
        let mut descriptors = Vec::with_capacity(lengths.len());
        let mut at = splice_start + prefix;
        for &len in &lengths {
            descriptors.push(Line {
                len,
                hash: hash_text(&self.text[at..at + len]),
            });
            at += len;
        }
        self.lines.drain(start..start + removed);
        self.lines.splice(start..start, descriptors);
        let after = start + lengths.len();
        self.anchor = (after < self.lines.len()).then_some(after);
        if self.lines.is_empty() {
            self.lines.push(Line {
                len: 0,
                hash: hash_text(""),
            });
        }
        self.invalidate(start, removed, delta_lines, delta_text, previous_changed);
    }

    /// Tokens for the lines in `start_line..end_line`, one list per line.
    /// Tokens cover a line's text without its line ending. At most the
    /// configured budget of lines is parsed per call; lines it did not reach
    /// come back as plain tokens, and [`advance`](Self::advance) reports
    /// whether work remains.
    pub fn tokens(&mut self, start_line: usize, end_line: usize) -> Vec<Vec<CodeToken>> {
        let start = start_line.min(self.lines.len());
        let end = end_line.min(self.lines.len());
        if end <= start {
            return Vec::new();
        }
        if self.grammar.is_none() {
            return (start..end).map(|line| self.plain_line(line)).collect();
        }
        let cached = (start..end).all(|line| self.runs.contains(line));
        if !cached {
            let Some((from, offset, state)) = self.known_state(start) else {
                return (start..end).map(|line| self.plain_line(line)).collect();
            };
            let budget = self.budget;
            let _ = self.parse_forward(
                state,
                ParsePlan {
                    line: from,
                    offset,
                    end,
                    max_parses: budget,
                    cache_from: start,
                    cache_to: end,
                },
            );
        }
        let mut offset = self.line_offset(start);
        (start..end)
            .map(|line| {
                let len = self.lines[line].len;
                let text = &self.text[offset..offset + len];
                offset += len;
                let hash = self.lines[line].hash;
                let Some(runs) = self.runs.get(line, hash) else {
                    return plain_text(text);
                };
                let mut tokens = Vec::new();
                runs.append_tokens(&mut tokens, text, visible_len(text));
                tokens
            })
            .collect()
    }

    /// Sets how many lines one [`tokens`](Self::tokens) call may parse.
    pub fn set_budget(&mut self, budget: usize) {
        self.budget = budget;
    }

    /// Parses up to `max_lines` more lines without asking for tokens. Returns
    /// true while lines still await a checkpoint.
    pub fn advance(&mut self, max_lines: usize) -> bool {
        if self.grammar.is_none() || self.parsed_through >= self.lines.len() {
            return false;
        }
        let at = self.parsed_through;
        if let Some((from, offset, state)) = self.known_state(at) {
            let _ = self.parse_forward(
                state,
                ParsePlan {
                    line: from,
                    offset,
                    end: self.lines.len(),
                    max_parses: max_lines,
                    cache_from: 0,
                    cache_to: 0,
                },
            );
        }
        self.parsed_through < self.lines.len()
    }

    /// Approximate heap bytes held for parse checkpoints and cached token
    /// runs. The document text itself is one shared buffer and is not
    /// included.
    pub fn memory_bytes(&self) -> usize {
        let mut bytes = self.lines.capacity() * mem::size_of::<Line>()
            + checkpoint_bytes(&self.checkpoints)
            + checkpoint_bytes(&self.stale)
            + self.runs.memory_bytes();
        if let Some((_, _, boundary)) = &self.frontier {
            bytes += mem::size_of::<Arc<Boundary>>() + boundary.memory_bytes();
        }
        bytes
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
