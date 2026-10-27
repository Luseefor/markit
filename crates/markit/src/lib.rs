//! Markdown to safe HTML or native UTF-16 text ranges. No global state or UI dependency.
mod document;
mod html;

pub use document::{parse, Document, Span, Style};
pub use html::{render_html, render_html_into};

use pulldown_cmark::{Options, Parser};

fn parser(source: &str) -> Parser<'_> {
    Parser::new_ext(
        source,
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS,
    )
}

/// One serialization and one bridge call per document, never per Markdown token.
pub fn render_document_json(source: &str) -> String {
    serde_json::to_string(&parse(source)).expect("document serialization is infallible")
}

/// Allow explicit web/mail/phone schemes and relative URLs. Reject browser-normalized
/// control characters, backslashes and protocol-relative destinations.
pub fn is_safe_url(url: &str) -> bool {
    let url = url.trim();
    if url.chars().any(|c| c.is_control() || c == '\\') || url.starts_with("//") {
        return false;
    }
    let first = url.split(['/', '?', '#']).next().unwrap_or_default();
    match first.split_once(':') {
        Some((scheme, _)) => ["http", "https", "mailto", "tel"]
            .iter()
            .any(|allowed| scheme.eq_ignore_ascii_case(allowed)),
        None => true,
    }
}
