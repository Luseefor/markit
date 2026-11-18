//! Line checkpoints and style runs shared by whole-file and incremental
//! highlighting.
//!
//! `ParseState` is not `Sync` only because onig match regions hold raw
//! pointers. The incremental handle is used by one thread at a time and is
//! marked `Send` at the C boundary, so atomic checkpoints remain the right
//! sharing type.
#![allow(clippy::arc_with_non_send_sync)]

use std::{
    cell::RefCell,
    collections::HashMap,
    sync::{Arc, OnceLock},
};

use syntect::{
    highlighting::{Color, Highlighter},
    parsing::{ParseState, Scope, ScopeStack, SyntaxReference, SyntaxSet},
};

use crate::highlight::CodeToken;

thread_local! {
    /// Light and dark colors for a scope stack. A theme's style is a pure
    /// function of the stack, so this is shared by every document and line.
    static STYLES: RefCell<HashMap<Vec<Scope>, Colors>> = RefCell::new(HashMap::new());
}

/// Parser state and scope stack between lines. Theme state is derived from
/// the scope stack, so it is not stored.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Boundary {
    pub(crate) parser: ParseState,
    pub(crate) path: ScopeStack,
}

/// One parsed line. The live text stays with the caller, so a document does
/// not store every line twice. `len` and `hash` identify the text that was
/// parsed.
pub(crate) struct ParsedLine {
    pub(crate) before: Arc<Boundary>,
    pub(crate) after: Arc<Boundary>,
    pub(crate) len: usize,
    pub(crate) hash: u64,
    runs: Vec<Run>,
}
type Colors = Arc<(String, String)>;
#[derive(Clone)]
struct Run {
    len: usize,
    colors: Colors,
}

/// Style runs for one line, without its parser boundaries. Editors keep a
/// bounded cache of these for recently requested lines.
pub(crate) struct CachedRuns {
    runs: Vec<Run>,
}

impl CachedRuns {
    pub(crate) fn plain(len: usize) -> CachedRuns {
        CachedRuns {
            runs: vec![Run {
                len,
                colors: plain_colors(),
            }],
        }
    }

    pub(crate) fn append_tokens(&self, tokens: &mut Vec<CodeToken>, text: &str, visible: usize) {
        append_runs(tokens, text, visible, &self.runs);
    }

    pub(crate) fn memory_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.runs.capacity() * std::mem::size_of::<Run>()
    }
}

impl Boundary {
    /// Approximate heap bytes, including an allowance for the parser stack
    /// syntect keeps behind `ParseState`.
    pub(crate) fn memory_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.path.len() * std::mem::size_of::<Scope>() + 128
    }
}

pub(crate) fn initial(syntax: &SyntaxReference) -> Arc<Boundary> {
    Arc::new(Boundary {
        parser: ParseState::new(syntax),
        path: ScopeStack::new(),
    })
}

/// A stable, cheap comparison value for a line. Equality of the length and
/// this hash is used instead of keeping a second copy of the line text.
pub(crate) fn hash_text(text: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in text.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn color(color: Color) -> String {
    format!("#{:02x}{:02x}{:02x}", color.r, color.g, color.b)
}

/// Same colors as syntect's `HighlightIterator` for the stack: it applies
/// the same single- and multi-selector scoring for every push, so the style
/// is a function of the stack. An empty stack uses the theme default, as the
/// iterator's initial state does.
fn colors(path: &[Scope], light: &Highlighter<'_>, dark: &Highlighter<'_>) -> Colors {
    STYLES.with(|styles| {
        let mut styles = styles.borrow_mut();
        if let Some(colors) = styles.get(path) {
            return colors.clone();
        }
        let (l, d) = if path.is_empty() {
            (
                light.get_default().foreground,
                dark.get_default().foreground,
            )
        } else {
            (
                light.style_for_stack(path).foreground,
                dark.style_for_stack(path).foreground,
            )
        };
        let colors = Arc::new((color(l), color(d)));
        if styles.len() >= 16 * 1024 {
            styles.clear();
        }
        styles.insert(path.to_vec(), colors.clone());
        colors
    })
}

fn plain_colors() -> Colors {
    static PLAIN: OnceLock<Colors> = OnceLock::new();
    PLAIN
        .get_or_init(|| Arc::new(("#24292f".into(), "#c9d1d9".into())))
        .clone()
}

fn after(before: &Arc<Boundary>, state: Boundary) -> Arc<Boundary> {
    if state == **before {
        before.clone()
    } else {
        Arc::new(state)
    }
}

fn parse_line(
    line: &str,
    before: &Arc<Boundary>,
    syntaxes: &SyntaxSet,
    light: &Highlighter<'_>,
    dark: &Highlighter<'_>,
) -> Option<Arc<ParsedLine>> {
    let mut state = Boundary::clone(before);
    let runs = parse_into_parts(&mut state, line, syntaxes, Some((light, dark)))?;
    Some(Arc::new(ParsedLine {
        before: before.clone(),
        after: after(before, state),
        len: line.len(),
        hash: hash_text(line),
        runs,
    }))
}

/// One line at its place in the file. A bare newline past line 0 carries no
/// matchable text (trailing-space rules need spaces or tabs), so it parses to
/// empty scope ops and replays the line in the current style with the boundary
/// unchanged. Line 0 always parses: `ParseState::first_line` emits the syntax
/// meta scope exactly once.
pub(crate) fn parse_line_at(
    index: usize,
    line: &str,
    before: &Arc<Boundary>,
    syntaxes: &SyntaxSet,
    light: &Highlighter<'_>,
    dark: &Highlighter<'_>,
) -> Option<Arc<ParsedLine>> {
    if index > 0 && (line == "\n" || line == "\r\n") {
        let run = Run {
            len: line.len(),
            colors: colors(before.path.as_slice(), light, dark),
        };
        return Some(Arc::new(ParsedLine {
            before: before.clone(),
            after: before.clone(),
            len: line.len(),
            hash: hash_text(line),
            runs: vec![run],
        }));
    }
    parse_line(line, before, syntaxes, light, dark)
}

/// Parses one line into `state` and returns its style runs.
pub(crate) fn parse_into_runs(
    state: &mut Boundary,
    index: usize,
    line: &str,
    syntaxes: &SyntaxSet,
    light: &Highlighter<'_>,
    dark: &Highlighter<'_>,
) -> Option<CachedRuns> {
    if index > 0 && (line == "\n" || line == "\r\n") {
        return Some(CachedRuns {
            runs: vec![Run {
                len: line.len(),
                colors: colors(state.path.as_slice(), light, dark),
            }],
        });
    }
    parse_into_parts(state, line, syntaxes, Some((light, dark))).map(|runs| CachedRuns { runs })
}

/// Parses one line into `state` without building style runs.
pub(crate) fn parse_into_state(
    state: &mut Boundary,
    index: usize,
    line: &str,
    syntaxes: &SyntaxSet,
) -> Option<()> {
    if index > 0 && (line == "\n" || line == "\r\n") {
        return Some(());
    }
    parse_into_parts(state, line, syntaxes, None).map(|_| ())
}

/// Parses a line and applies its scope operations. Style runs are built only
/// when `theme` is present; state-only lines skip color work and allocation.
fn parse_into_parts(
    state: &mut Boundary,
    line: &str,
    syntaxes: &SyntaxSet,
    theme: Option<(&Highlighter<'_>, &Highlighter<'_>)>,
) -> Option<Vec<Run>> {
    let operations = state.parser.parse_line(line, syntaxes).ok()?;
    let mut runs: Vec<Run> = Vec::new();
    if let Some((light, dark)) = theme {
        let mut push = |len: usize, colors: Colors| {
            if let Some(last) = runs.last_mut().filter(|last| last.colors == colors) {
                last.len += len;
            } else {
                runs.push(Run { len, colors });
            }
        };
        let mut pos = 0;
        for (at, operation) in &operations {
            let at = (*at).min(line.len());
            if at > pos {
                push(at - pos, colors(state.path.as_slice(), light, dark));
                pos = at;
            }
            state.path.apply(operation).ok()?;
        }
        if line.len() > pos {
            push(line.len() - pos, colors(state.path.as_slice(), light, dark));
        }
    } else {
        for (_, operation) in &operations {
            state.path.apply(operation).ok()?;
        }
    }
    Some(runs)
}

/// Appends a line's runs as tokens, merging with the last token when the
/// colours match. Only the first `visible` bytes are emitted.
pub(crate) fn append_line_tokens(
    tokens: &mut Vec<CodeToken>,
    text: &str,
    visible: usize,
    line: &ParsedLine,
) {
    append_runs(tokens, text, visible, &line.runs);
}

fn append_runs(tokens: &mut Vec<CodeToken>, text: &str, visible: usize, runs: &[Run]) {
    let mut offset = 0;
    for run in runs {
        if offset >= visible {
            break;
        }
        let len = run.len.min(visible - offset);
        let part = &text[offset..offset + len];
        if let Some(last) = tokens
            .last_mut()
            .filter(|last| last.light == run.colors.0 && last.dark == run.colors.1)
        {
            last.text.push_str(part);
        } else {
            tokens.push(CodeToken {
                text: part.into(),
                light: run.colors.0.clone(),
                dark: run.colors.1.clone(),
            });
        }
        offset += run.len;
    }
}
