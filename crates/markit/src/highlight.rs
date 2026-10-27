use serde::{Deserialize, Serialize};
use std::{cell::RefCell, sync::Arc};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CodeToken {
    pub text: String,
    pub light: String,
    pub dark: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HighlightedCode {
    pub language: String,
    pub plain: bool,
    pub tokens: Vec<CodeToken>,
}
thread_local! {
    static CACHE: RefCell<crate::cache::Cache<(String, String), Arc<HighlightedCode>>> = RefCell::new(crate::cache::Cache::new(4 * 1024 * 1024));
}
pub fn language_alias(language: &str) -> String {
    match language.trim().to_ascii_lowercase().as_str() {
        "ts" => "typescript",
        "js" | "mjs" | "cjs" => "javascript",
        "py" => "python",
        "rs" => "rust",
        "sh" | "zsh" | "shell" | "shellscript" | "console" => "bash",
        "c++" => "cpp",
        "cs" => "csharp",
        "yml" => "yaml",
        "md" => "markdown",
        "xml" | "svg" | "markup" => "html",
        "jsonc" => "json",
        "patch" => "diff",
        other => return other.to_owned(),
    }
    .into()
}
#[derive(Clone, Copy, PartialEq)]
enum Tone {
    Plain,
    Comment,
    String,
    Number,
    Keyword,
    Function,
    Type,
    Property,
    Tag,
    Punctuation,
}
impl Tone {
    fn colors(self) -> (&'static str, &'static str) {
        match self {
            Self::Plain | Self::Punctuation => ("#24292f", "#c9d1d9"),
            Self::Comment => ("#6e7781", "#8b949e"),
            Self::String => ("#0a3069", "#a5d6ff"),
            Self::Number | Self::Property => ("#0550ae", "#79c0ff"),
            Self::Keyword => ("#cf222e", "#ff7b72"),
            Self::Function => ("#8250df", "#d2a8ff"),
            Self::Type => ("#953800", "#ffa657"),
            Self::Tag => ("#116329", "#7ee787"),
        }
    }
}
fn word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$')
}
fn keyword(word: &str, language: &str) -> bool {
    let words = match language {
        "python" => "and as assert async await break class continue def del elif else except False finally for from global if import in is lambda None nonlocal not or pass raise return True try while with yield self",
        "bash" => "if then else elif fi for in do done case esac while until function export local return echo cd source readonly unset declare select break continue",
        "sql" => "select from where join left right inner outer on as insert into update delete create table index drop alter values set null not and or order by group having limit offset union all distinct case when then else end primary key references default true false",
        "json" | "json5" => "true false null Infinity NaN",
        "yaml" => "true false null yes no on off",
        "rust" => "as async await break const continue crate dyn else enum extern false fn for if impl in let loop match mod move mut pub ref return self Self static struct super trait true type unsafe use where while abstract become box do final macro override priv typeof unsized virtual yield try union",
        "swift" => "actor any as associatedtype async await borrowing break case catch class consuming continue convenience default defer deinit didSet do dynamic else enum extension fallthrough false fileprivate final for func get guard if import in indirect init inout internal is isolated lazy let mutating nil nonisolated nonmutating open operator optional override package private protocol public repeat required rethrows return self Self set some static struct subscript super switch throw throws true try typealias unowned var weak where while willSet",
        _ => "abstract as async await boolean break case catch char class const continue debugger default defer delete do double else enum export extends false final finally float for from func function go if implements import in instanceof int interface internal is let long map match namespace native new null object of out override package private protected public readonly record ref return satisfies sealed self short static string struct super switch synchronized this throw throws trait true try type typeof undefined unsigned use using val var virtual void volatile when where while with yield",
    };
    words.split_ascii_whitespace().any(|candidate| {
        if language == "sql" {
            candidate.eq_ignore_ascii_case(word)
        } else {
            candidate == word
        }
    })
}
fn scan(code: &str, language: &str) -> HighlightedCode {
    let supported = [
        "typescript",
        "tsx",
        "javascript",
        "jsx",
        "python",
        "bash",
        "rust",
        "go",
        "java",
        "kotlin",
        "swift",
        "c",
        "cpp",
        "csharp",
        "json",
        "json5",
        "yaml",
        "markdown",
        "sql",
        "css",
        "html",
        "diff",
        "clike",
    ]
    .contains(&language);
    let plain = || HighlightedCode {
        language: language.into(),
        plain: true,
        tokens: vec![CodeToken {
            text: code.into(),
            light: "#24292f".into(),
            dark: "#c9d1d9".into(),
        }],
    };
    if !supported || code.len() > 1024 * 1024 {
        return plain();
    }
    let bytes = code.as_bytes();
    let mut at = 0;
    let mut tokens: Vec<CodeToken> = Vec::new();
    let mut in_tag = false;
    while at < bytes.len() {
        let start = at;
        let byte = bytes[at];
        let mut tone = Tone::Plain;
        let hash_comment = ["python", "bash", "yaml"].contains(&language);
        let slash_comment = !["python", "bash", "yaml", "html", "markdown"].contains(&language);
        if (hash_comment && byte == b'#')
            || (slash_comment && code[at..].starts_with("//"))
            || (language == "sql" && code[at..].starts_with("--"))
        {
            at += code[at..].find('\n').unwrap_or(bytes.len() - at);
            tone = Tone::Comment;
        } else if slash_comment && code[at..].starts_with("/*") {
            at += 2;
            let mut depth = 1;
            while at < bytes.len() && depth > 0 {
                if language == "rust" && code[at..].starts_with("/*") {
                    depth += 1;
                    at += 2;
                } else if code[at..].starts_with("*/") {
                    depth -= 1;
                    at += 2;
                } else {
                    at += code[at..].chars().next().unwrap().len_utf8();
                }
            }
            tone = Tone::Comment;
        } else if ["html", "jsx", "tsx"].contains(&language) && code[at..].starts_with("<!--") {
            at = code[at + 4..]
                .find("-->")
                .map_or(bytes.len(), |end| at + 4 + end + 3);
            tone = Tone::Comment;
        } else if language == "rust" && byte == b'r' && code[at + 1..].starts_with(['#', '"']) {
            let hashes = bytes[at + 1..].iter().take_while(|&&b| b == b'#').count();
            let quote = at + 1 + hashes;
            if bytes.get(quote) == Some(&b'"') {
                let end_marker = format!("\"{}", "#".repeat(hashes));
                at = code[quote + 1..]
                    .find(&end_marker)
                    .map_or(bytes.len(), |end| quote + 1 + end + end_marker.len());
                tone = Tone::String;
            } else {
                at += 1;
            }
        } else if matches!(byte, b'"' | b'\'' | b'`') {
            // Rust lifetimes are identifiers rather than unterminated strings.
            let lifetime = language == "rust"
                && byte == b'\''
                && bytes.get(at + 1).is_some_and(|b| b.is_ascii_alphabetic())
                && bytes.get(at + 2) != Some(&b'\'');
            if lifetime {
                at += 1;
                while at < bytes.len() && word_byte(bytes[at]) {
                    at += 1;
                }
            } else {
                let triple = ["python", "kotlin", "swift"].contains(&language)
                    && bytes
                        .get(at..at + 3)
                        .is_some_and(|part| part == [byte, byte, byte]);
                let marker = if triple { 3 } else { 1 };
                at += marker;
                while at < bytes.len() {
                    if bytes[at] == b'\\' {
                        at += 1;
                        if at < bytes.len() {
                            at += code[at..].chars().next().unwrap().len_utf8();
                        }
                    } else if bytes[at] == byte
                        && (!triple
                            || bytes
                                .get(at..at + 3)
                                .is_some_and(|part| part == [byte, byte, byte]))
                    {
                        at += marker;
                        break;
                    } else {
                        at += code[at..].chars().next().unwrap().len_utf8();
                    }
                }
                tone = Tone::String;
            }
        } else if byte.is_ascii_digit() && (at == 0 || !word_byte(bytes[at - 1])) {
            at += 1;
            while at < bytes.len()
                && (bytes[at].is_ascii_alphanumeric() || matches!(bytes[at], b'.' | b'_'))
            {
                at += 1;
            }
            tone = Tone::Number;
        } else if byte.is_ascii_alphabetic() || matches!(byte, b'_' | b'$') {
            at += 1;
            while at < bytes.len() && word_byte(bytes[at]) {
                at += 1;
            }
            let word = &code[start..at];
            let after = code[at..].trim_start();
            tone = if keyword(word, language) {
                Tone::Keyword
            } else if in_tag && ["html", "jsx", "tsx"].contains(&language) {
                if after.starts_with('=') {
                    Tone::Property
                } else {
                    Tone::Tag
                }
            } else if after.starts_with('(') {
                Tone::Function
            } else if after.starts_with(':') {
                Tone::Property
            } else if word.as_bytes()[0].is_ascii_uppercase() {
                Tone::Type
            } else {
                Tone::Plain
            };
        } else {
            if ["html", "jsx", "tsx"].contains(&language) {
                if byte == b'<' {
                    in_tag = true;
                } else if byte == b'>' {
                    in_tag = false;
                }
            }
            at += code[at..].chars().next().unwrap().len_utf8();
            if byte.is_ascii_punctuation() {
                tone = Tone::Punctuation;
            }
        }
        let (light, dark) = tone.colors();
        if let Some(last) = tokens
            .last_mut()
            .filter(|last| last.light == light && last.dark == dark)
        {
            last.text.push_str(&code[start..at]);
        } else {
            tokens.push(CodeToken {
                text: code[start..at].into(),
                light: light.into(),
                dark: dark.into(),
            });
        }
        if tokens.len() > 8192 {
            return plain();
        }
    }
    HighlightedCode {
        language: language.into(),
        plain: false,
        tokens,
    }
}
pub fn highlight(code: &str, language: &str) -> Arc<HighlightedCode> {
    let key = (language_alias(language), code.to_owned());
    if let Some(cached) = CACHE.with(|cache| cache.borrow_mut().get(&key)) {
        return cached;
    }
    let result = Arc::new(scan(code, &key.0));
    let weight = key.0.len()
        + key.1.len()
        + result
            .tokens
            .iter()
            .map(|t| t.text.len() + 96)
            .sum::<usize>();
    CACHE.with(|cache| cache.borrow_mut().insert(key, result.clone(), weight));
    result
}
