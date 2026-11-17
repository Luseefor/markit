use super::*;
use crate::highlight_grammar;
use serde::Deserialize;

#[derive(Deserialize)]
struct Fixture {
    language: String,
    source: String,
}

fn fixtures() -> Vec<Fixture> {
    serde_json::from_str(include_str!("../../tests/fixtures/grammar.json")).unwrap()
}

fn lines_of(text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    let body = text.strip_suffix('\n').unwrap_or(text);
    body.split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line).to_owned())
        .collect()
}

fn join(lines: &[String], trailing_newline: bool) -> String {
    let mut text = lines.join("\n");
    if trailing_newline && !lines.is_empty() {
        text.push('\n');
    }
    text
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
    if source.ends_with('\n') || source.is_empty() {
        lines.pop();
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
    let text: String = tokens[line]
        .iter()
        .map(|token| token.text.as_str())
        .collect();
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
        let trailing_newline = fixture.source.ends_with('\n');
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
            let text = join(&lines, trailing_newline);
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
        let mut trailing_newline = fixture.source.ends_with('\n');
        let mut highlighter = LineHighlighter::new(&fixture.language);
        highlighter.set_text(&fixture.source);
        let mut rng = Rng(0x2545_f491_4f6c_dd1d ^ position as u64);
        for step in 0..10 {
            let start = rng.next() % (lines.len() + 1);
            let removed = (rng.next() % 3).min(lines.len() - start);
            let inserted: Vec<String> = (0..rng.next() % 4)
                .map(|index| format!("inserted {step} {index}"))
                .collect();
            if removed == 0 && start == lines.len() && !inserted.is_empty() {
                trailing_newline = false;
            }
            lines.splice(start..start + removed, inserted.iter().cloned());
            let inserted: Vec<&str> = inserted.iter().map(String::as_str).collect();
            highlighter.edit(start, removed, &inserted);
            let text = join(&lines, trailing_newline);
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
    let before = highlighter.tokens(0, 5);
    let keyword = colour_of(&before, 2, "let");
    highlighter.edit(1, 1, &["    /* opened"]);
    let opened = highlighter.tokens(0, 5);
    let opened_text = "fn main() {\n    /* opened\n    let second = 2;\n    let third = 3;\n}\n";
    assert_eq!(opened, highlight_lines(opened_text, "rust"));
    let comment = colour_of(&opened, 1, "opened");
    assert_eq!(colour_of(&opened, 2, "let"), comment);
    assert_eq!(colour_of(&opened, 3, "let"), comment);
    highlighter.edit(2, 1, &["    */"]);
    let closed = highlighter.tokens(0, 5);
    let closed_text = "fn main() {\n    /* opened\n    */\n    let third = 3;\n}\n";
    assert_eq!(closed, highlight_lines(closed_text, "rust"));
    assert_eq!(colour_of(&closed, 3, "let"), keyword);
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
    assert_eq!(parsed_line_count() - before, 5);
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
            assert_eq!(tokens[index][0].light, "#24292f");
            assert_eq!(tokens[index][0].dark, "#c9d1d9");
        }
    }
}
