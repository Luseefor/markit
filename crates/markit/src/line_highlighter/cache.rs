//! Compact line metadata, checkpoints, and the bounded run cache used by the
//! incremental highlighter.

use std::{
    collections::{BTreeMap, HashMap},
    mem,
    sync::Arc,
};

use crate::{
    grammar::{hash_text, plain_token, Boundary, CachedRuns},
    highlight::CodeToken,
};

const CACHED_LINES: usize = 2000;

pub(super) struct Line {
    pub(super) len: usize,
    pub(super) hash: u64,
}

#[derive(Clone)]
pub(super) struct Checkpoint {
    pub(super) offset: usize,
    pub(super) boundary: Arc<Boundary>,
}

struct CachedLine {
    runs: Arc<CachedRuns>,
    hash: u64,
    used: u64,
}

pub(super) struct RunCache {
    runs: HashMap<usize, CachedLine>,
    tick: u64,
}

impl RunCache {
    pub(super) fn new() -> RunCache {
        RunCache {
            runs: HashMap::new(),
            tick: 0,
        }
    }

    pub(super) fn get(&mut self, line: usize, hash: u64) -> Option<Arc<CachedRuns>> {
        self.tick += 1;
        let entry = self.runs.get_mut(&line)?;
        if entry.hash != hash {
            return None;
        }
        entry.used = self.tick;
        Some(entry.runs.clone())
    }

    pub(super) fn contains(&self, line: usize) -> bool {
        self.runs.contains_key(&line)
    }

    pub(super) fn insert(&mut self, line: usize, hash: u64, runs: CachedRuns) {
        if self.runs.contains_key(&line) {
            return;
        }
        if self.runs.len() >= CACHED_LINES {
            if let Some(old) = self
                .runs
                .iter()
                .min_by_key(|(_, entry)| entry.used)
                .map(|(line, _)| *line)
            {
                self.runs.remove(&old);
            }
        }
        self.tick += 1;
        self.runs.insert(
            line,
            CachedLine {
                runs: Arc::new(runs),
                hash,
                used: self.tick,
            },
        );
    }

    pub(super) fn clear(&mut self) {
        self.runs.clear();
        self.tick = 0;
    }

    pub(super) fn memory_bytes(&self) -> usize {
        let mut bytes =
            self.runs.capacity() * (mem::size_of::<usize>() + mem::size_of::<CachedLine>());
        for entry in self.runs.values() {
            bytes += mem::size_of::<Arc<CachedRuns>>() + entry.runs.memory_bytes();
        }
        bytes
    }
}

pub(super) fn checkpoint_bytes(checkpoints: &BTreeMap<usize, Checkpoint>) -> usize {
    checkpoints.len() * (mem::size_of::<(usize, Checkpoint)>() + mem::size_of::<Arc<Boundary>>())
        + checkpoints
            .values()
            .map(|checkpoint| checkpoint.boundary.memory_bytes())
            .sum::<usize>()
}

pub(super) fn visible_len(text: &str) -> usize {
    if text.ends_with("\r\n") {
        text.len() - 2
    } else if text.ends_with('\n') {
        text.len() - 1
    } else {
        text.len()
    }
}

pub(super) fn plain_text(text: &str) -> Vec<CodeToken> {
    let content = &text[..visible_len(text)];
    if content.is_empty() {
        Vec::new()
    } else {
        vec![plain_token(content)]
    }
}

pub(super) fn line_ranges(text: &str) -> Vec<Line> {
    let mut lines = Vec::with_capacity(text.bytes().filter(|byte| *byte == b'\n').count() + 1);
    let mut start = 0;
    for (at, byte) in text.bytes().enumerate() {
        if byte == b'\n' {
            lines.push(Line {
                len: at + 1 - start,
                hash: hash_text(&text[start..=at]),
            });
            start = at + 1;
        }
    }
    lines.push(Line {
        len: text.len() - start,
        hash: hash_text(&text[start..]),
    });
    lines
}

/// Inserted lines as one string and the length of each line in it.
pub(super) fn inserted_lines(inserted: &[&str], at_end: bool) -> (String, Vec<usize>) {
    if inserted.is_empty() {
        return (String::new(), Vec::new());
    }
    let joined = inserted.join("\n");
    let parts: Vec<&str> = joined.split('\n').collect();
    let count = parts.len();
    let mut text = String::with_capacity(joined.len() + count);
    let mut lengths = Vec::with_capacity(count);
    for (index, part) in parts.iter().enumerate() {
        let ending = index + 1 < count || !at_end;
        text.push_str(part);
        if ending {
            text.push('\n');
        }
        lengths.push(part.len() + usize::from(ending));
    }
    (text, lengths)
}
