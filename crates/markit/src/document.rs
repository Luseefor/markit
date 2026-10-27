use pulldown_cmark::{CodeBlockKind, Event, Tag, TagEnd};
use serde::{Deserialize, Serialize};

/// All positions are UTF-16 code units, matching NSRange and Android spans.
/// Ranges are half-open and index the rendered `text`, not the Markdown source.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Document {
    pub version: u32,
    pub text: String,
    pub spans: Vec<Span>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    #[serde(flatten)]
    pub style: Style,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Style {
    Heading { level: u8 },
    Paragraph,
    Strong,
    Emphasis,
    Strikethrough,
    Code,
    CodeBlock { language: String },
    Link { url: String },
    Image { url: String },
    Quote { depth: usize },
    ListItem { depth: usize },
    Table,
    TableHeader,
    Rule,
}

struct Frame {
    end: TagEnd,
    start: usize,
    style: Option<Style>,
}

struct Builder {
    document: Document,
    position: usize,
    frames: Vec<Frame>,
    lists: Vec<Option<u64>>,
    quote_depth: usize,
    cell: usize,
}

impl Builder {
    fn text(&mut self, text: &str) {
        self.document.text.push_str(text);
        self.position += text.encode_utf16().count();
    }

    fn newline(&mut self) {
        if !self.document.text.is_empty() && !self.document.text.ends_with('\n') {
            self.text("\n");
        }
    }

    fn span(&mut self, start: usize, style: Style) {
        if start < self.position {
            self.document.spans.push(Span {
                start,
                end: self.position,
                style,
            });
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        let end = tag.to_end();
        let mut start = self.position;
        let style = match tag {
            Tag::Heading { level, .. } => Some(Style::Heading { level: level as u8 }),
            Tag::Paragraph => Some(Style::Paragraph),
            Tag::Strong => Some(Style::Strong),
            Tag::Emphasis => Some(Style::Emphasis),
            Tag::Strikethrough => Some(Style::Strikethrough),
            Tag::CodeBlock(kind) => Some(Style::CodeBlock {
                language: match kind {
                    CodeBlockKind::Fenced(info) => {
                        info.split_whitespace().next().unwrap_or("").to_owned()
                    }
                    CodeBlockKind::Indented => String::new(),
                },
            }),
            Tag::Link { dest_url, .. } if super::is_safe_url(&dest_url) => Some(Style::Link {
                url: dest_url.into_string(),
            }),
            Tag::Image { dest_url, .. } if super::is_safe_url(&dest_url) => Some(Style::Image {
                url: dest_url.into_string(),
            }),
            Tag::BlockQuote(_) => {
                self.quote_depth += 1;
                Some(Style::Quote {
                    depth: self.quote_depth,
                })
            }
            Tag::List(first) => {
                self.newline();
                self.lists.push(first);
                None
            }
            Tag::Item => {
                self.newline();
                start = self.position;
                for _ in 1..self.lists.len() {
                    self.text("  ");
                }
                match self.lists.last_mut() {
                    Some(Some(number)) => {
                        let prefix = format!("{}. ", *number);
                        *number = number.saturating_add(1);
                        self.text(&prefix);
                    }
                    _ => self.text("• "),
                }
                Some(Style::ListItem {
                    depth: self.lists.len(),
                })
            }
            Tag::Table(_) => Some(Style::Table),
            Tag::TableHead => {
                self.cell = 0;
                Some(Style::TableHeader)
            }
            Tag::TableRow => {
                self.cell = 0;
                None
            }
            Tag::TableCell => {
                if self.cell > 0 {
                    self.text("\t");
                }
                self.cell += 1;
                None
            }
            _ => None,
        };
        self.frames.push(Frame { end, start, style });
    }

    fn end(&mut self, end: TagEnd) {
        let frame = self.frames.pop().expect("parser emits balanced events");
        debug_assert_eq!(frame.end, end);
        // Include the paragraph terminator so native paragraph spans never spill
        // across two adjacent blocks. All storage and positions stay UTF-16.
        match end {
            TagEnd::Paragraph
            | TagEnd::Heading(_)
            | TagEnd::CodeBlock
            | TagEnd::Item
            | TagEnd::BlockQuote(_)
            | TagEnd::TableHead
            | TagEnd::TableRow
            | TagEnd::Table
            | TagEnd::HtmlBlock => self.newline(),
            _ => {}
        }
        if let Some(style) = frame.style {
            self.span(frame.start, style);
        }
        match end {
            TagEnd::List(_) => {
                self.lists.pop();
            }
            TagEnd::BlockQuote(_) => self.quote_depth -= 1,
            _ => {}
        }
    }
}

/// Build the native render plan in a single pass without a recursive AST.
pub fn parse(source: &str) -> Document {
    let mut builder = Builder {
        document: Document {
            version: 1,
            text: String::with_capacity(source.len()),
            spans: Vec::new(),
        },
        position: 0,
        frames: Vec::new(),
        lists: Vec::new(),
        quote_depth: 0,
        cell: 0,
    };
    for event in super::parser(source) {
        match event {
            Event::Start(tag) => builder.start(tag),
            Event::End(end) => builder.end(end),
            Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => builder.text(&text),
            Event::Code(text) => {
                let start = builder.position;
                builder.text(&text);
                builder.span(start, Style::Code);
            }
            Event::SoftBreak => builder.text(" "),
            Event::HardBreak => builder.text("\n"),
            Event::Rule => {
                builder.newline();
                let start = builder.position;
                builder.text("────────\n");
                builder.span(start, Style::Rule);
            }
            Event::TaskListMarker(checked) => builder.text(if checked { "☑ " } else { "☐ " }),
            // These extensions are deliberately disabled in parser().
            Event::FootnoteReference(text) | Event::InlineMath(text) | Event::DisplayMath(text) => {
                builder.text(&text)
            }
        }
    }
    // Outer spans first: native font/style overrides can then compose inner marks.
    // Stable ordering preserves parser order for ranges with equal boundaries.
    builder
        .document
        .spans
        .sort_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)));
    builder.document
}
