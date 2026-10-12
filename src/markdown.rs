use pulldown_cmark::{html, CowStr, Event, Parser, Tag};

/// Renders Markdown to HTML that is safe to embed in a page.
///
/// Raw HTML in the source is shown as text instead of being passed through, and
/// links or images that use a scheme other than http, https or mailto (for
/// example `javascript:`) are replaced with `#`.
pub fn render_markdown(markdown: &str) -> String {
    let parser = Parser::new(markdown).map(|event| match event {
        Event::Html(raw) | Event::InlineHtml(raw) => Event::Text(raw),
        Event::Start(Tag::Link {
            link_type,
            dest_url,
            title,
            id,
        }) => Event::Start(Tag::Link {
            link_type,
            dest_url: safe_url(dest_url),
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
            dest_url: safe_url(dest_url),
            title,
            id,
        }),
        other => other,
    });
    let mut output = String::new();
    html::push_html(&mut output, parser);
    output
}

fn safe_url(url: CowStr<'_>) -> CowStr<'_> {
    if is_safe_url(&url) {
        url
    } else {
        CowStr::Borrowed("#")
    }
}

/// Relative URLs and http, https and mailto URLs are allowed.
pub fn is_safe_url(url: &str) -> bool {
    // Browsers ignore whitespace and control characters inside a scheme
    // (`java\tscript:`), so they are removed before looking at it.
    let normalized: String = url
        .chars()
        .filter(|character| !character.is_whitespace() && !character.is_control())
        .collect::<String>()
        .to_ascii_lowercase();
    match normalized.find(':') {
        None => true,
        Some(index) => {
            let scheme = &normalized[..index];
            // A colon after `/`, `?` or `#` belongs to a relative URL's path, query or fragment.
            scheme.contains(['/', '?', '#']) || matches!(scheme, "http" | "https" | "mailto")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_is_rendered_to_html() {
        let output = render_markdown("# 제목\n\n**강조**");
        assert!(output.contains("<h1>제목</h1>"));
        assert!(output.contains("<strong>강조</strong>"));
    }

    #[test]
    fn raw_html_is_escaped() {
        let output =
            render_markdown("<script>alert(1)</script>\n\n문단 <img src=x onerror=alert(1)>");
        assert!(!output.contains("<script"));
        assert!(!output.contains("<img"));
        assert!(output.contains("&lt;script&gt;"));
    }

    #[test]
    fn dangerous_link_schemes_are_neutralized() {
        for source in [
            "[x](javascript:alert(1))",
            "[x](JaVaScRiPt:alert(1))",
            "[x](<java\tscript:alert(1)>)",
            "[x](data:text/html,hi)",
            "![x](vbscript:msgbox)",
        ] {
            let output = render_markdown(source);
            assert!(
                output.contains("href=\"#\"") || output.contains("src=\"#\""),
                "{source} → {output}"
            );
        }
    }

    #[test]
    fn ordinary_links_are_kept() {
        assert!(
            render_markdown("[a](https://example.com)").contains("href=\"https://example.com\"")
        );
        assert!(render_markdown("[a](mailto:me@example.com)").contains("mailto:me@example.com"));
        assert!(render_markdown("[a](/posts/x)").contains("href=\"/posts/x\""));
        assert!(render_markdown("[a](#section)").contains("href=\"#section\""));
        assert!(render_markdown("[a](./note?at=10:30)").contains("./note?at=10:30"));
    }
}
