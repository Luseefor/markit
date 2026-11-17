use std::{
    cell::RefCell,
    sync::{Arc, OnceLock},
};
use syntect::{
    highlighting::{Highlighter, ThemeSet},
    parsing::{SyntaxReference, SyntaxSet},
    util::LinesWithEndings,
};

use crate::highlight::{language_alias, CodeToken, HighlightedCode};

mod line;
pub(crate) use line::{
    append_line_tokens, hash_text, initial, parse_line_at, plain_line, Boundary, ParsedLine,
};

static SYNTAXES: OnceLock<SyntaxSet> = OnceLock::new();
static THEMES: OnceLock<ThemeSet> = OnceLock::new();
thread_local! {
    static PREVIOUS: RefCell<Option<ParsedCode>> = const { RefCell::new(None) };
    /// The last result. A streamed fence that only grows is answered here
    /// instead of the byte-budgeted cache, so its tokens can be reused in
    /// place once the caller has dropped them.
    static LAST: RefCell<Option<(String, Arc<HighlightedCode>)>> = const { RefCell::new(None) };
    static CACHE: RefCell<crate::cache::Cache<(String, String), Arc<HighlightedCode>>> =
        RefCell::new(crate::cache::Cache::new(4 * 1024 * 1024));
}

/// The two themes every syntax color comes from, built once per process.
fn themes() -> &'static ThemeSet {
    THEMES.get_or_init(|| {
        let extras = two_face::theme::extra();
        let mut themes = ThemeSet::new();
        themes.themes.insert(
            "light".into(),
            extras
                .get(two_face::theme::EmbeddedThemeName::InspiredGithub)
                .clone(),
        );
        themes.themes.insert(
            "dark".into(),
            extras
                .get(two_face::theme::EmbeddedThemeName::Dracula)
                .clone(),
        );
        themes
    })
}

/// `Highlighter::new` splits every theme rule into selector lists; build the
/// two highlighters once per process instead of once per call.
pub(crate) fn highlighters() -> &'static (Highlighter<'static>, Highlighter<'static>) {
    let themes = themes();
    static HIGHLIGHTERS: OnceLock<(Highlighter<'static>, Highlighter<'static>)> = OnceLock::new();
    HIGHLIGHTERS.get_or_init(|| {
        (
            Highlighter::new(&themes.themes["light"]),
            Highlighter::new(&themes.themes["dark"]),
        )
    })
}

/// The grammars shared by every language. Built once, on first use.
pub(crate) fn syntaxes() -> &'static SyntaxSet {
    SYNTAXES.get_or_init(two_face::syntax::extra_newlines)
}

/// The grammar for a name, token or extension. `None` means plain text.
pub(crate) fn syntax_for(language: &str) -> Option<&'static SyntaxReference> {
    let syntaxes = syntaxes();
    syntaxes
        .find_syntax_by_token(language)
        .or_else(|| syntaxes.find_syntax_by_extension(language))
        .or_else(|| {
            syntaxes.find_syntax_by_name(match language {
                "csharp" => "C#",
                "jsx" | "json5" => "JavaScript",
                "tsx" => "TypeScriptReact",
                "bash" => "Bourne Again Shell (bash)",
                _ => language,
            })
        })
}

pub(crate) fn plain_token(text: &str) -> CodeToken {
    CodeToken {
        text: text.into(),
        light: "#24292f".into(),
        dark: "#c9d1d9".into(),
    }
}

pub fn highlight(code: &str, language: &str) -> Arc<HighlightedCode> {
    let language = language_alias(language);
    let last = LAST.with(|last| last.borrow_mut().take());
    let last = match last {
        Some((source, result)) if result.language == language && source == code => {
            LAST.with(|slot| *slot.borrow_mut() = Some((source, result.clone())));
            return result;
        }
        Some((source, result)) if result.language == language && code.starts_with(&source) => {
            Some((source, result))
        }
        _ => None,
    };
    let extension = last.is_some();
    let key = (language.clone(), code.to_owned());
    if let Some(cached) = CACHE.with(|cache| {
        cache
            .borrow_mut()
            .get_by(&key, |candidate| candidate == &key)
    }) {
        return cached;
    }
    let plain = || HighlightedCode {
        language: language.clone(),
        plain: true,
        tokens: vec![plain_token(code)],
    };
    let result = if code.len() > 128 * 1024
        || code.lines().any(|line| line.len() > 2048)
        || language.is_empty()
    {
        plain()
    } else if let Some(syntax) = syntax_for(&language) {
        let themes = highlighters();
        let previous = PREVIOUS.with(|cache| cache.borrow_mut().take());
        let reuse = last
            .and_then(|(source, result)| Some((source.len(), Arc::try_unwrap(result).ok()?)))
            .filter(|(_, result)| !result.plain)
            .map(|(len, result)| (len, result.tokens));
        match parse_code(code, &language, syntax, syntaxes(), themes, previous, reuse) {
            Some((tokens, parsed)) => {
                PREVIOUS.with(|cache| *cache.borrow_mut() = parsed);
                HighlightedCode {
                    language: language.clone(),
                    plain: false,
                    tokens,
                }
            }
            None => plain(),
        }
    } else {
        plain()
    };
    let weight = key.0.len()
        + key.1.len()
        + result
            .tokens
            .iter()
            .map(|token| token.text.len() + 96)
            .sum::<usize>();
    let result = Arc::new(result);
    let (language, code) = key;
    if !extension {
        CACHE.with(|cache| {
            cache
                .borrow_mut()
                .insert((language, code.clone()), result.clone(), weight)
        });
    }
    LAST.with(|last| *last.borrow_mut() = Some((code, result.clone())));
    result
}

fn cut_tokens(mut tokens: Vec<CodeToken>, at: usize) -> Vec<CodeToken> {
    let mut end = 0;
    for (index, token) in tokens.iter_mut().enumerate() {
        if end + token.text.len() >= at {
            token.text.truncate(at - end);
            let keep = index + usize::from(at > end);
            tokens.truncate(keep);
            return tokens;
        }
        end += token.text.len();
    }
    tokens.clear();
    tokens
}

// One recent document per thread, with at most 4096 line checkpoints. Exact-result
// caching remains independently byte-budgeted above. Lines share their snapshots
// across edits, so reusing a suffix does not clone parser or scope stacks.
struct ParsedCode {
    language: String,
    initial: Arc<Boundary>,
    lines: Vec<Arc<ParsedLine>>,
}
fn parse_code(
    code: &str,
    language: &str,
    syntax: &syntect::parsing::SyntaxReference,
    syntaxes: &SyntaxSet,
    (light, dark): &(Highlighter<'_>, Highlighter<'_>),
    previous: Option<ParsedCode>,
    reuse: Option<(usize, Vec<CodeToken>)>,
) -> Option<(Vec<CodeToken>, Option<ParsedCode>)> {
    let lines: Vec<_> = LinesWithEndings::from(code).collect();
    let previous = previous.filter(|old| old.language == language);
    let old = previous
        .as_ref()
        .map(|old| old.lines.as_slice())
        .unwrap_or_default();
    let prefix = lines
        .iter()
        .zip(old)
        .take_while(|(line, old)| line.len() == old.len && hash_text(line) == old.hash)
        .count();
    let suffix = lines[prefix..]
        .iter()
        .rev()
        .zip(old[prefix..].iter().rev())
        .take_while(|(line, old)| line.len() == old.len && hash_text(line) == old.hash)
        .count();
    let initial = match previous.as_ref() {
        Some(old) => old.initial.clone(),
        None => initial(syntax),
    };
    let mut state = if prefix > 0 {
        old[prefix - 1].after.clone()
    } else {
        initial.clone()
    };
    let mut parsed = Vec::with_capacity(lines.len());
    parsed.extend_from_slice(&old[..prefix]);
    for index in prefix..lines.len() {
        if index >= lines.len() - suffix {
            let old_index = old.len() - (lines.len() - index);
            if *state == *old[old_index].before {
                parsed.extend_from_slice(&old[old_index..]);
                break;
            }
        }
        let line = parse_line_at(index, lines[index], &state, syntaxes, light, dark)?;
        state = line.after.clone();
        parsed.push(line);
    }
    // Tokens for the unchanged leading lines are the previous tokens cut at
    // the same byte: merging is a left-to-right fold over the same runs.
    let kept: usize = lines[..prefix].iter().map(|line| line.len()).sum();
    let mut tokens = match reuse {
        Some((len, tokens)) if kept <= len => cut_tokens(tokens, kept),
        _ => Vec::new(),
    };
    let first = if tokens.is_empty() { 0 } else { prefix };
    for (line, text) in parsed[first..].iter().zip(&lines[first..]) {
        append_line_tokens(&mut tokens, text, text.len(), line);
        if tokens.len() > 8192 {
            return None;
        }
    }
    let cache = (parsed.len() <= 4096).then(|| ParsedCode {
        language: language.into(),
        initial,
        lines: parsed,
    });
    Some((tokens, cache))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Fixture {
        language: String,
        source: String,
        json: String,
    }
    fn fixtures() -> Vec<Fixture> {
        serde_json::from_str(include_str!("../tests/fixtures/grammar.json")).unwrap()
    }
    fn clear() {
        CACHE.with(|cache| *cache.borrow_mut() = crate::cache::Cache::new(4 * 1024 * 1024));
        PREVIOUS.with(|cache| *cache.borrow_mut() = None);
        LAST.with(|last| *last.borrow_mut() = None);
    }
    fn json(source: &str, language: &str) -> String {
        serde_json::to_string(&*crate::highlight_grammar(source, language).unwrap()).unwrap()
    }
    #[test]
    fn grammar_json_matches_original_on_six_fixtures() {
        for fixture in fixtures() {
            clear();
            assert_eq!(
                json(&fixture.source, &fixture.language),
                fixture.json,
                "{}",
                fixture.language
            );
        }
    }
    #[test]
    fn streamed_growth_matches_full_grammar_json() {
        for fixture in fixtures() {
            let source = fixture.source;
            let mut cuts: Vec<usize> = (0..source.len())
                .step_by(5)
                .filter(|&at| source.is_char_boundary(at))
                .collect();
            cuts.extend(source.match_indices('\n').map(|(at, _)| at + 1));
            cuts.push(source.len());
            cuts.sort_unstable();
            cuts.dedup();
            clear();
            let streamed: Vec<String> = cuts
                .iter()
                .map(|&at| json(&source[..at], &fixture.language))
                .collect();
            for (&at, streamed) in cuts.iter().zip(streamed) {
                clear();
                assert_eq!(
                    streamed,
                    json(&source[..at], &fixture.language),
                    "{} at {at}",
                    fixture.language
                );
            }
        }
    }
    #[test]
    fn edits_and_line_insertions_match_full_grammar_json() {
        for fixture in fixtures() {
            clear();
            let mut source = fixture.source;
            json(&source, &fixture.language);
            // Include syntax-state changes that propagate beyond the edited line,
            // then close them again. Insert/delete lines and change the final newline.
            for insertion in [
                "inserted 🦀 line\n",
                "/*\n",
                "*/\n",
                "\"\"\"\n",
                "\"\"\"\n",
                "\r\n",
            ] {
                let mid = source
                    .match_indices('\n')
                    .nth(source.lines().count() / 2)
                    .map(|(i, _)| i + 1)
                    .unwrap_or(0);
                source.insert_str(mid, insertion);
                let incremental = json(&source, &fixture.language);
                clear();
                assert_eq!(
                    incremental,
                    json(&source, &fixture.language),
                    "{}: {insertion:?}",
                    fixture.language
                );
            }
            for _ in 0..3 {
                let end = source.find('\n').map(|i| i + 1).unwrap_or(source.len());
                source.drain(..end);
                let incremental = json(&source, &fixture.language);
                clear();
                assert_eq!(incremental, json(&source, &fixture.language));
            }
            source.push_str("\ntrailing 🦀");
            let incremental = json(&source, &fixture.language);
            clear();
            assert_eq!(incremental, json(&source, &fixture.language));
        }
    }
}
