//! Parsing and invalidation internals for [`LineHighlighter`](super::LineHighlighter).

use std::{collections::BTreeMap, sync::Arc};

use crate::{
    grammar::{highlighters, parse_into_runs, parse_into_state, syntaxes, Boundary, CachedRuns},
    highlight::CodeToken,
};

use super::{
    cache::{plain_text, visible_len, Checkpoint},
    LineHighlighter, ParsePlan, CHECKPOINT_LINES, MAX_LINE,
};

impl LineHighlighter {
    /// Byte start of a line, summed from the nearest checkpoint.
    pub(super) fn line_offset(&self, line: usize) -> usize {
        let (from, mut offset) = self
            .checkpoints
            .range(..=line)
            .next_back()
            .map(|(at, checkpoint)| (*at, checkpoint.offset))
            .unwrap_or((0, 0));
        offset += self.lines[from..line]
            .iter()
            .map(|line| line.len)
            .sum::<usize>();
        offset
    }

    pub(super) fn line_text(&self, line: usize) -> &str {
        let start = self.line_offset(line);
        &self.text[start..start + self.lines[line].len]
    }

    pub(super) fn plain_line(&self, line: usize) -> Vec<CodeToken> {
        plain_text(self.line_text(line))
    }

    /// The closest known boundary at or before `line`.
    pub(super) fn known_state(&self, line: usize) -> Option<(usize, usize, Arc<Boundary>)> {
        let checkpoint = self
            .checkpoints
            .range(..=line)
            .next_back()
            .map(|(at, checkpoint)| (*at, checkpoint.offset, checkpoint.boundary.clone()));
        if let Some((frontier, offset, boundary)) = &self.frontier {
            if *frontier <= line {
                if let Some((at, _, _)) = &checkpoint {
                    if *at >= *frontier {
                        return checkpoint;
                    }
                }
                return Some((*frontier, *offset, boundary.clone()));
            }
        }
        if checkpoint.is_some() {
            return checkpoint;
        }
        self.initial.clone().map(|initial| (0, 0, initial))
    }

    /// Parses from `line` toward `end`, caching runs for `cache_from..cache_to`
    /// and adding checkpoints as it goes. Returns the first line not parsed.
    pub(super) fn parse_forward(&mut self, state: Arc<Boundary>, plan: ParsePlan) -> usize {
        let ParsePlan {
            mut line,
            mut offset,
            end,
            max_parses,
            cache_from,
            cache_to,
        } = plan;
        let syntaxes = syntaxes();
        let (light, dark) = highlighters();
        let mut boundary = Boundary::clone(&state);
        let mut parsed = 0;
        let mut promoted: Option<(usize, usize, Arc<Boundary>)> = None;
        while line < end {
            if let Some(old) = self.stale.get(&line).cloned() {
                if old.boundary.as_ref() == &boundary {
                    let remaining = self.stale.split_off(&line);
                    self.stale.clear();
                    for (at, checkpoint) in remaining {
                        if promoted.as_ref().is_none_or(|(best, _, _)| at > *best) {
                            promoted = Some((at, checkpoint.offset, checkpoint.boundary.clone()));
                        }
                        self.checkpoints.insert(at, checkpoint);
                    }
                }
            }
            if parsed >= max_parses {
                break;
            }
            let len = self.lines[line].len;
            let text = &self.text[offset..offset + len];
            let cached = line >= cache_from && line < cache_to;
            if visible_len(text) > MAX_LINE {
                if cached && !self.runs.contains(line) {
                    let hash = self.lines[line].hash;
                    self.runs.insert(line, hash, CachedRuns::plain(len));
                }
            } else if cached && !self.runs.contains(line) {
                #[cfg(test)]
                super::PARSED_LINES.with(|lines| lines.set(lines.get() + 1));
                let Some(runs) = parse_into_runs(&mut boundary, line, text, syntaxes, light, dark)
                else {
                    break;
                };
                let hash = self.lines[line].hash;
                self.runs.insert(line, hash, runs);
            } else {
                #[cfg(test)]
                super::PARSED_LINES.with(|lines| lines.set(lines.get() + 1));
                let Some(()) = parse_into_state(&mut boundary, line, text, syntaxes) else {
                    break;
                };
            }
            offset += len;
            let next = line + 1;
            if next < self.lines.len()
                && (next.is_multiple_of(CHECKPOINT_LINES) || self.anchor == Some(next))
            {
                self.checkpoints.insert(
                    next,
                    Checkpoint {
                        offset,
                        boundary: Arc::new(boundary.clone()),
                    },
                );
                if self.anchor == Some(next) {
                    self.anchor = None;
                }
            }
            line = next;
            parsed += 1;
        }
        if let Some((at, at_offset, boundary)) = promoted {
            if at > self.parsed_through {
                self.parsed_through = at;
                self.frontier = Some((at, at_offset, boundary));
            }
        }
        if line > self.parsed_through {
            self.parsed_through = line;
            self.frontier = Some((line, offset, Arc::new(boundary)));
        } else if self.frontier.is_none() && line == self.parsed_through {
            self.frontier = Some((line, offset, Arc::new(boundary)));
        }
        line
    }

    /// Text range to replace, whether a leading newline belongs to the line
    /// before the edit, and whether that line's ending changed.
    pub(super) fn splice_for(
        &self,
        start: usize,
        removed: usize,
        at_end: bool,
        lengths: &[usize],
    ) -> (usize, usize, usize, bool) {
        if !at_end {
            return (
                self.line_offset(start),
                self.line_offset(start + removed),
                0,
                false,
            );
        }
        if start == 0 {
            return (0, self.text.len(), 0, false);
        }
        if lengths.is_empty() {
            let offset = self.line_offset(start - 1);
            let text = &self.text[offset..offset + self.lines[start - 1].len];
            return (offset + visible_len(text), self.text.len(), 0, true);
        }
        if removed == 0 {
            return (self.text.len(), self.text.len(), 1, true);
        }
        (self.line_offset(start), self.text.len(), 0, false)
    }

    pub(super) fn invalidate(
        &mut self,
        start: usize,
        removed: usize,
        delta_lines: isize,
        delta_text: isize,
        previous_changed: bool,
    ) {
        let invalid = if previous_changed {
            start.saturating_sub(1)
        } else {
            start
        };
        self.parsed_through = self.parsed_through.min(invalid);
        self.frontier = None;
        self.runs.clear();
        let keep = invalid;
        let mut stale = BTreeMap::new();
        for (at, checkpoint) in self.checkpoints.split_off(&(keep + 1)) {
            if at >= start + removed {
                let shifted = at.saturating_add_signed(delta_lines);
                if shifted < self.lines.len() {
                    stale.insert(
                        shifted,
                        Checkpoint {
                            offset: checkpoint.offset.saturating_add_signed(delta_text),
                            boundary: checkpoint.boundary,
                        },
                    );
                }
            }
        }
        self.stale = stale;
    }
}
