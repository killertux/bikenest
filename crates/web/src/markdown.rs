//! Markdown → HTML for the versioned legal pages (§70/§71).
//!
//! The policy documents are authored as markdown (`policies/*.md`) and stored
//! verbatim in `policy_version`. This renderer is the *only* place that turns
//! them into markup, and it treats the source as untrusted (§103):
//!
//! - raw HTML in the source (`<script>`, `<img onerror=…>`, comments) is
//!   emitted as escaped **text**, never as markup;
//! - link/image destinations are restricted to `http(s):`, `mailto:`,
//!   site-relative (`/…`) and fragment (`#…`) URLs — anything else (e.g.
//!   `javascript:`) is neutralised to `#`.
//!
//! Templates mark the *output* of this function `|safe`; the stored text is
//! never marked safe itself.

use pulldown_cmark::{Event, Options, Parser, Tag, html};

/// Render policy markdown to HTML. Tables and strikethrough are enabled; raw
/// HTML is escaped; unsafe link schemes are neutralised.
pub fn render_policy_markdown(source: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);

    let events = Parser::new_ext(source, options).map(|event| match event {
        Event::Html(raw) | Event::InlineHtml(raw) => Event::Text(raw),
        Event::Start(Tag::Link {
            link_type,
            dest_url,
            title,
            id,
        }) => Event::Start(Tag::Link {
            link_type,
            dest_url: safe_destination(dest_url),
            title,
            id,
        }),
        Event::Start(Tag::Image {
            link_type,
            dest_url,
            title,
            id,
        }) => Event::Start(Tag::Image {
            link_type,
            dest_url: safe_destination(dest_url),
            title,
            id,
        }),
        other => other,
    });

    let mut out = String::with_capacity(source.len() + source.len() / 2);
    html::push_html(&mut out, events);
    out
}

fn safe_destination(dest: pulldown_cmark::CowStr<'_>) -> pulldown_cmark::CowStr<'_> {
    let d = dest.trim();
    let allowed = d.starts_with("https://")
        || d.starts_with("http://")
        || d.starts_with("mailto:")
        || (d.starts_with('/') && !d.starts_with("//"))
        || d.starts_with('#');
    if allowed { dest } else { "#".into() }
}

/// Minimal HTML text escaping for the few non-markdown strings that share the
/// `|safe` slot with rendered markdown (e.g. the "not available" notice).
pub fn escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_headings_lists_and_tables() {
        let html = render_policy_markdown(
            "## Quem somos\n\n- um\n- dois\n\n| Cookie | Prazo |\n|---|---|\n| `session_id` | 30 dias |\n",
        );
        assert!(html.contains("<h2>Quem somos</h2>"));
        assert!(html.contains("<li>um</li>"));
        assert!(html.contains("<table>"));
        assert!(html.contains("<code>session_id</code>"));
    }

    #[test]
    fn raw_html_is_shown_as_text_not_markup() {
        let html = render_policy_markdown(
            "Hello <script>alert(1)</script>\n\n<div onclick=\"x()\">block</div>\n",
        );
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
        assert!(!html.contains("<div"));
        assert!(html.contains("&lt;div onclick="));
    }

    #[test]
    fn unsafe_link_schemes_are_neutralised() {
        let html = render_policy_markdown(
            "[a](javascript:alert(1)) [b](https://example.com) [c](/privacy) [d](mailto:x@y.z) [e](data:text/html,x)",
        );
        assert!(!html.contains("javascript:"));
        assert!(!html.contains("data:"));
        assert!(html.contains("href=\"https://example.com\""));
        assert!(html.contains("href=\"/privacy\""));
        assert!(html.contains("href=\"mailto:x@y.z\""));
        assert!(html.contains("href=\"#\""));
    }

    #[test]
    fn protocol_relative_links_are_neutralised() {
        // `//evil.example` is not site-relative — a browser resolves it against
        // the current scheme, so it would silently leave the site.
        let html = render_policy_markdown("[a](//evil.example)");
        assert!(!html.contains("href=\"//evil.example\""));
        assert!(html.contains("href=\"#\""));
    }

    #[test]
    fn escape_text_escapes_markup_characters() {
        assert_eq!(
            escape_text("<b> & \"q\" 'a'"),
            "&lt;b&gt; &amp; &quot;q&quot; &#39;a&#39;"
        );
    }
}
