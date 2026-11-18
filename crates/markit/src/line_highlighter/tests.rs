use super::*;
use crate::highlight_grammar;
use serde::Deserialize;
const PLAIN_LIGHT: &str = "#24292f";
const PLAIN_DARK: &str = "#c9d1d9";

#[derive(Deserialize)]
struct Fixture {
    language: String,
    source: String,
}

fn fixtures() -> Vec<Fixture> {
    serde_json::from_str(include_str!("../../tests/fixtures/grammar.json")).unwrap()
}

fn lines_of(text: &str) -> Vec<String> {
    let parts: Vec<&str> = text.split('\n').collect();
    parts
        .iter()
        .enumerate()
        .map(|(index, part)| {
            let part = if index + 1 < parts.len() {
                part.strip_suffix('\r').unwrap_or(part)
            } else {
                part
            };
            part.to_owned()
        })
        .collect()
}

fn content(tokens: &[CodeToken]) -> String {
    tokens.iter().map(|token| token.text.as_str()).collect()
}

fn contents(lines: &[Vec<CodeToken>]) -> Vec<String> {
    lines.iter().map(|line| content(line)).collect()
}

fn push_piece(tokens: &mut Vec<CodeToken>, text: &str, token: &CodeToken) {
    if text.is_empty() {
        return;
    }
    if let Some(last) = tokens
        .last_mut()
        .filter(|last| last.light == token.light && last.dark == token.dark)
    {
        last.text.push_str(text);
    } else {
        tokens.push(CodeToken {
            text: text.into(),
            light: token.light.clone(),
            dark: token.dark.clone(),
        });
    }
}

/// What `highlight` returns, cut into one token list per source line.
fn highlight_lines(source: &str, language: &str) -> Vec<Vec<CodeToken>> {
    let code = highlight_grammar(source, language).unwrap();
    let mut lines: Vec<Vec<CodeToken>> = vec![Vec::new()];
    for token in &code.tokens {
        let mut rest = token.text.as_str();
        while let Some(at) = rest.find('\n') {
            let piece = &rest[..at];
            push_piece(
                lines.last_mut().unwrap(),
                piece.strip_suffix('\r').unwrap_or(piece),
                token,
            );
            lines.push(Vec::new());
            rest = &rest[at + 1..];
        }
        push_piece(lines.last_mut().unwrap(), rest, token);
    }
    lines
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 as usize
    }
}

fn colour_of(tokens: &[Vec<CodeToken>], line: usize, needle: &str) -> String {
    let text = content(&tokens[line]);
    let at = text.find(needle).unwrap();
    let mut start = 0;
    for token in &tokens[line] {
        let end = start + token.text.len();
        if start <= at && at + needle.len() <= end {
            return token.light.clone();
        }
        start = end;
    }
    panic!("{needle:?} spans several tokens");
}

#[test]
fn line_tokens_match_highlight_on_every_fixture() {
    for fixture in fixtures() {
        let mut highlighter = LineHighlighter::new(&fixture.language);
        highlighter.set_text(&fixture.source);
        let expected = highlight_lines(&fixture.source, &fixture.language);
        assert_eq!(
            highlighter.tokens(0, expected.len()),
            expected,
            "{}",
            fixture.language
        );
    }
}

#[test]
fn editing_one_line_matches_full_highlight() {
    for (position, fixture) in fixtures().iter().enumerate() {
        let mut lines = lines_of(&fixture.source);
        let mut highlighter = LineHighlighter::new(&fixture.language);
        highlighter.set_text(&fixture.source);
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15 ^ position as u64);
        for edit in 0..12 {
            let index = rng.next() % lines.len();
            let replacement = match edit % 6 {
                0 => format!("    // edited {edit}"),
                1 => format!("let value_{edit} = \"text\";"),
                2 => format!("/* open {edit}"),
                3 => String::new(),
                4 => format!("}} else {{ // {edit}"),
                _ => format!("value = {edit}"),
            };
            lines[index] = replacement.clone();
            highlighter.edit(index, 1, &[replacement.as_str()]);
            let text = lines.join("\n");
            assert_eq!(
                highlighter.tokens(0, lines.len()),
                highlight_lines(&text, &fixture.language),
                "{} edit {edit}",
                fixture.language
            );
        }
    }
}

#[test]
fn inserting_and_removing_lines_matches_full_highlight() {
    for (position, fixture) in fixtures().iter().enumerate() {
        let mut lines = lines_of(&fixture.source);
        let mut highlighter = LineHighlighter::new(&fixture.language);
        highlighter.set_text(&fixture.source);
        let mut rng = Rng(0x2545_f491_4f6c_dd1d ^ position as u64);
        for step in 0..10 {
            let start = rng.next() % (lines.len() + 1);
            let removed = (rng.next() % 3).min(lines.len() - start);
            let inserted: Vec<String> = (0..rng.next() % 4)
                .map(|index| format!("inserted {step} {index}"))
                .collect();
            lines.splice(start..start + removed, inserted.iter().cloned());
            let inserted: Vec<&str> = inserted.iter().map(String::as_str).collect();
            highlighter.edit(start, removed, &inserted);
            let text = lines.join("\n");
            assert_eq!(
                highlighter.tokens(0, lines.len()),
                highlight_lines(&text, &fixture.language),
                "{} step {step}",
                fixture.language
            );
        }
    }
}

#[test]
fn opening_a_block_comment_rehighlights_following_lines() {
    let source = "fn main() {\n    let first = 1;\n    let second = 2;\n    let third = 3;\n}\n";
    let mut highlighter = LineHighlighter::new("rust");
    highlighter.set_text(source);
    let before = highlighter.tokens(0, 6);
    let keyword = colour_of(&before, 2, "let");
    highlighter.edit(1, 1, &["    /* opened"]);
    let opened = highlighter.tokens(0, 6);
    let opened_text = "fn main() {\n    /* opened\n    let second = 2;\n    let third = 3;\n}\n";
    assert_eq!(opened, highlight_lines(opened_text, "rust"));
    let comment = colour_of(&opened, 1, "opened");
    assert_eq!(colour_of(&opened, 2, "let"), comment);
    assert_eq!(colour_of(&opened, 3, "let"), comment);
    highlighter.edit(2, 1, &["    */"]);
    let closed = highlighter.tokens(0, 6);
    let closed_text = "fn main() {\n    /* opened\n    */\n    let third = 3;\n}\n";
    assert_eq!(closed, highlight_lines(closed_text, "rust"));
    assert_eq!(colour_of(&closed, 3, "let"), keyword);
}

#[test]
fn appending_after_a_line_comment_reparses_the_last_line() {
    let mut highlighter = LineHighlighter::new("rust");
    highlighter.set_text("// note");
    assert_eq!(contents(&highlighter.tokens(0, 1)), ["// note"]);
    highlighter.edit(1, 0, &["let x = 1;"]);
    let tokens = highlighter.tokens(0, 2);
    assert_eq!(contents(&tokens), ["// note", "let x = 1;"]);
    assert_eq!(tokens, highlight_lines("// note\nlet x = 1;", "rust"));
}

#[test]
fn removing_the_last_lines_reparses_the_line_comment() {
    let mut highlighter = LineHighlighter::new("rust");
    highlighter.set_text("// note\nlet x = 1;\n");
    highlighter.tokens(0, 3);
    highlighter.edit(1, 2, &[]);
    assert_eq!(contents(&highlighter.tokens(0, 1)), ["// note"]);
    assert_eq!(highlighter.tokens(0, 1), highlight_lines("// note", "rust"));
}

#[test]
fn lines_split_on_newline_only() {
    let mut highlighter = LineHighlighter::new("");
    highlighter.set_text("a\r\nb\rc");
    assert_eq!(contents(&highlighter.tokens(0, 5)), ["a", "b\rc"]);
    highlighter.set_text("a\n");
    assert_eq!(contents(&highlighter.tokens(0, 5)), ["a", ""]);
    highlighter.set_text("");
    assert_eq!(contents(&highlighter.tokens(0, 5)), [""]);
}

#[test]
fn inserted_newlines_split_into_lines() {
    let mut highlighter = LineHighlighter::new("");
    highlighter.set_text("a");
    highlighter.edit(1, 0, &["b\nc"]);
    assert_eq!(contents(&highlighter.tokens(0, 5)), ["a", "b", "c"]);
    highlighter.edit(3, 0, &["d\ne\n"]);
    assert_eq!(
        contents(&highlighter.tokens(0, 8)),
        ["a", "b", "c", "d", "e", ""]
    );
}

#[test]
fn a_400kb_line_stays_plain_without_grammar_parsing() {
    let long = "x".repeat(400 * 1024);
    let source = format!("fn main() {{\n    let value = \"{long}\";\n    let after = 1;\n}}\n");
    let mut highlighter = LineHighlighter::new("rust");
    highlighter.set_text(&source);
    let before = parsed_line_count();
    let tokens = highlighter.tokens(0, 5);
    assert_eq!(parsed_line_count() - before, 4);
    assert_eq!(tokens[1].len(), 1);
    assert_eq!(tokens[1][0].light, PLAIN_LIGHT);
    assert_eq!(tokens[1][0].dark, PLAIN_DARK);
    assert_eq!(contents(&tokens)[2], "    let after = 1;");
    assert_ne!(colour_of(&tokens, 2, "let"), PLAIN_LIGHT);

    let replacement = format!("    let changed = \"{long} y\";");
    highlighter.edit(1, 1, &[replacement.as_str()]);
    let before = parsed_line_count();
    let tokens = highlighter.tokens(0, 5);
    assert_eq!(parsed_line_count() - before, 4);
    assert_eq!(tokens[1].len(), 1);
    assert_eq!(tokens[1][0].light, PLAIN_LIGHT);
    assert_eq!(tokens[1][0].dark, PLAIN_DARK);
    assert_eq!(contents(&tokens)[2], "    let after = 1;");
    assert_ne!(colour_of(&tokens, 2, "let"), PLAIN_LIGHT);
}

#[test]
fn set_budget_limits_each_tokens_call() {
    let source: String = (0..20)
        .map(|index| format!("let value_{index} = {index};\n"))
        .collect();
    let mut highlighter = LineHighlighter::new("rust");
    highlighter.set_text(&source);
    highlighter.set_budget(5);
    let before = parsed_line_count();
    let tokens = highlighter.tokens(0, 20);
    assert_eq!(parsed_line_count() - before, 5);
    assert!(tokens[10..]
        .iter()
        .all(|line| line.iter().all(|token| token.light == PLAIN_LIGHT)));
    highlighter.set_budget(100);
    assert_eq!(highlighter.tokens(0, 21), highlight_lines(&source, "rust"));
}

#[test]
fn budget_opens_far_lines_as_plain_then_advance_catches_up() {
    let mut source = "\n".repeat(150_000);
    for index in 0..60 {
        source.push_str(&format!("let value_{index} = {index};\n"));
    }
    let mut highlighter = LineHighlighter::new("rust");
    highlighter.set_text(&source);
    let before = parsed_line_count();
    let tokens = highlighter.tokens(150_000, 150_060);
    assert!(parsed_line_count() - before <= DEFAULT_BUDGET);
    assert!(tokens
        .iter()
        .all(|line| line.iter().all(|token| token.light == PLAIN_LIGHT)));
    assert!(highlighter.advance(0));
    while highlighter.advance(2000) {}
    let mut oracle = LineHighlighter::new("rust");
    oracle.set_text(&source);
    oracle.set_budget(usize::MAX);
    assert_eq!(
        highlighter.tokens(150_000, 150_060),
        oracle.tokens(150_000, 150_060)
    );
    assert!(!highlighter.advance(0));
}

#[test]
fn tokens_parse_only_up_to_the_requested_line() {
    let source: String = (0..80)
        .map(|index| format!("let value_{index} = {index};\n"))
        .collect();
    let mut highlighter = LineHighlighter::new("rust");
    highlighter.set_text(&source);
    let before = parsed_line_count();
    highlighter.tokens(0, 10);
    assert_eq!(parsed_line_count() - before, 10);
    let before = parsed_line_count();
    highlighter.tokens(0, 10);
    assert_eq!(parsed_line_count(), before);
    let before = parsed_line_count();
    highlighter.tokens(20, 30);
    assert_eq!(parsed_line_count() - before, 20);
    highlighter.edit(5, 1, &["/* changed"]);
    let before = parsed_line_count();
    highlighter.tokens(0, 10);
    assert_eq!(parsed_line_count() - before, 10);
}

#[test]
fn unknown_language_is_plain_text() {
    let source = "# title\n\nnot a comment \"or a string\" 42\nlast\n";
    let lines = lines_of(source);
    for language in ["not-a-language", ""] {
        let mut highlighter = LineHighlighter::new(language);
        highlighter.set_text(source);
        let tokens = highlighter.tokens(0, lines.len());
        assert_eq!(tokens.len(), lines.len());
        for (index, line) in lines.iter().enumerate() {
            if line.is_empty() {
                assert!(tokens[index].is_empty());
                continue;
            }
            assert_eq!(tokens[index].len(), 1);
            assert_eq!(tokens[index][0].text, *line);
            assert_eq!(tokens[index][0].light, PLAIN_LIGHT);
            assert_eq!(tokens[index][0].dark, PLAIN_DARK);
        }
    }
}

#[test]
fn memory_stays_under_64_bytes_per_line_at_200k_lines() {
    let source = "\n".repeat(200_000);
    let mut highlighter = LineHighlighter::new("rust");
    highlighter.set_text(&source);
    highlighter.tokens(0, 60);
    highlighter.advance(usize::MAX);
    let bytes = highlighter.memory_bytes();
    assert!(bytes / 200_000 <= 64, "{bytes} bytes for 200,000 lines");
}
