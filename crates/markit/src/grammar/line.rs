//! Line checkpoints and style runs shared by whole-file and incremental
//! highlighting.

use std::{cell::RefCell, collections::HashMap, rc::Rc};

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
pub(crate) struct ParsedLine {
    pub(crate) text: String,
    pub(crate) before: Rc<Boundary>,
    pub(crate) after: Rc<Boundary>,
    runs: Vec<Run>,
}
type Colors = Rc<(String, String)>;
struct Run {
    len: usize,
    colors: Colors,
}

pub(crate) fn initial(syntax: &SyntaxReference) -> Rc<Boundary> {
    Rc::new(Boundary {
        parser: ParseState::new(syntax),
        path: ScopeStack::new(),
    })
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
        let colors = Rc::new((color(l), color(d)));
        if styles.len() >= 16 * 1024 {
            styles.clear();
        }
        styles.insert(path.to_vec(), colors.clone());
        colors
    })
}

fn parse_line(
    line: &str,
    before: &Rc<Boundary>,
    syntaxes: &SyntaxSet,
    light: &Highlighter<'_>,
    dark: &Highlighter<'_>,
) -> Option<Rc<ParsedLine>> {
    let mut state = Boundary::clone(before);
    let operations = state.parser.parse_line(line, syntaxes).ok()?;
    let mut runs: Vec<Run> = Vec::new();
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
    Some(Rc::new(ParsedLine {
        text: line.into(),
        before: before.clone(),
        after: Rc::new(state),
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
    before: &Rc<Boundary>,
    syntaxes: &SyntaxSet,
    light: &Highlighter<'_>,
    dark: &Highlighter<'_>,
) -> Option<Rc<ParsedLine>> {
    if index > 0 && (line == "\n" || line == "\r\n") {
        return Some(Rc::new(ParsedLine {
            text: line.into(),
            before: before.clone(),
            after: before.clone(),
            runs: vec![Run {
                len: line.len(),
                colors: colors(before.path.as_slice(), light, dark),
            }],
        }));
    }
    parse_line(line, before, syntaxes, light, dark)
}

/// Appends a line's runs as tokens, merging with the last token when the
/// colours match. Only the first `visible` bytes are emitted.
pub(crate) fn append_line_tokens(tokens: &mut Vec<CodeToken>, line: &ParsedLine, visible: usize) {
    let mut offset = 0;
    for run in &line.runs {
        if offset >= visible {
            break;
        }
        let len = run.len.min(visible - offset);
        let part = &line.text[offset..offset + len];
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
