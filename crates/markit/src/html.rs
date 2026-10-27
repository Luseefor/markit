use pulldown_cmark::{html, CowStr, Event, Tag};

/// Reuse the caller's allocation on repeated renders. Existing contents are cleared.
pub fn render_html_into(source: &str, output: &mut String) {
    output.clear();
    output.reserve(source.len());
    let events = super::parser(source).map(|event| match event {
        // Raw HTML is displayed literally, never executed, including HTML blocks.
        Event::Html(text) | Event::InlineHtml(text) => Event::Text(text),
        Event::Start(Tag::Link {
            link_type,
            dest_url,
            title,
            id,
        }) => {
            let dest_url = safe_destination(dest_url);
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                title,
                id,
            })
        }
        Event::Start(Tag::Image {
            link_type,
            dest_url,
            title,
            id,
        }) => {
            let dest_url = safe_destination(dest_url);
            Event::Start(Tag::Image {
                link_type,
                dest_url,
                title,
                id,
            })
        }
        other => other,
    });
    html::push_html(output, events);
}

fn safe_destination(url: CowStr<'_>) -> CowStr<'_> {
    if super::is_safe_url(&url) {
        url
    } else {
        CowStr::Borrowed("")
    }
}

pub fn render_html(source: &str) -> String {
    let mut output = String::with_capacity(source.len());
    render_html_into(source, &mut output);
    output
}
