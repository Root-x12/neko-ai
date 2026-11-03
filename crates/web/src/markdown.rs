//! Markdown → safe HTML. Raw HTML in model output is shown as text, never executed.

use pulldown_cmark::{html, CodeBlockKind, CowStr, Event, LinkType, Options, Parser, Tag, TagEnd};

pub fn escape(s: &str) -> String {
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

fn safe_url(url: &str) -> bool {
    // Browsers ignore tabs/newlines inside URLs ("java\tscript:"), so drop them before checking.
    let lower: String =
        url.chars().filter(|c| !c.is_whitespace() && !c.is_control()).collect::<String>().to_ascii_lowercase();
    match lower.split_once(':') {
        // A colon before any '/', '?' or '#' means a scheme.
        Some((scheme, _)) if !scheme.contains(['/', '?', '#']) => matches!(scheme, "http" | "https" | "mailto"),
        _ => true,
    }
}

fn code_block(lang: &str, code: &str) -> String {
    let lang: String = lang
        .split(|c: char| c.is_whitespace() || c == ',' || c == '{')
        .next()
        .unwrap_or("")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '#' | '-' | '_'))
        .take(20)
        .collect::<String>()
        .to_ascii_lowercase();
    let class = if lang.is_empty() { String::new() } else { format!(" class=\"language-{lang}\"") };
    let label = if lang.is_empty() { "code".to_string() } else { lang };
    format!(
        "<div class=\"code-block\"><div class=\"code-head\"><span>{label}</span>\
         <button type=\"button\" class=\"copy-btn\" data-copy-code>Copy</button></div>\
         <pre><code{class}>{}</code></pre></div>",
        escape(code)
    )
}

pub fn render(md: &str) -> String {
    let opts = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut events: Vec<Event> = Vec::new();
    let mut code: Option<(String, String)> = None; // (lang, text)

    for ev in Parser::new_ext(md, opts) {
        if let Some((_, text)) = &mut code {
            match ev {
                Event::Text(t) => text.push_str(&t),
                Event::End(TagEnd::CodeBlock) => {
                    let (lang, text) = code.take().unwrap();
                    events.push(Event::Html(CowStr::from(code_block(&lang, &text))));
                }
                _ => {}
            }
            continue;
        }
        match ev {
            Event::Start(Tag::CodeBlock(kind)) => {
                let lang = match kind {
                    CodeBlockKind::Fenced(l) => l.to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                code = Some((lang, String::new()));
            }
            Event::Html(h) | Event::InlineHtml(h) => events.push(Event::Text(h)),
            Event::Start(Tag::Link { link_type, dest_url, title, id }) => {
                let dest_url = if safe_url(&dest_url) { dest_url } else { CowStr::from("#") };
                events.push(Event::Start(Tag::Link { link_type, dest_url, title, id }));
            }
            // Remote images would leak requests to third parties; show them as links instead.
            Event::Start(Tag::Image { dest_url, title, id, .. }) => {
                let dest_url = if safe_url(&dest_url) { dest_url } else { CowStr::from("#") };
                events.push(Event::Start(Tag::Link { link_type: LinkType::Inline, dest_url, title, id }));
                events.push(Event::Text(CowStr::from("🖼 ")));
            }
            Event::End(TagEnd::Image) => events.push(Event::End(TagEnd::Link)),
            other => events.push(other),
        }
    }
    // Unterminated fence while streaming.
    if let Some((lang, text)) = code {
        events.push(Event::Html(CowStr::from(code_block(&lang, &text))));
    }
    let mut out = String::with_capacity(md.len() * 3 / 2);
    html::push_html(&mut out, events.into_iter());
    out
}

/// Search snippets mark hits with `[[` `]]`; turn them into `<mark>` safely.
pub fn snippet_html(snippet: &str) -> String {
    escape(snippet).replace("[[", "<mark>").replace("]]", "</mark>")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_raw_html() {
        let out = render("hi <script>alert(1)</script> <img src=x onerror=alert(1)>");
        assert!(!out.contains("<script"));
        assert!(!out.contains("<img"));
        assert!(out.contains("&lt;script&gt;"));
    }

    #[test]
    fn blocks_js_links() {
        let out = render("[x](javascript:alert(1)) [y](https://ok.dev) [z](/rel) ![i](JaVaScRiPt:alert(1))");
        assert!(!out.to_lowercase().contains("javascript:"));
        assert!(out.contains("https://ok.dev"));
        assert!(out.contains("href=\"/rel\""));
    }

    #[test]
    fn renders_code_blocks() {
        let out = render("```rust\nfn main() { println!(\"<hi>\"); }\n```");
        assert!(out.contains("language-rust"));
        assert!(out.contains("&lt;hi&gt;"));
        assert!(out.contains("data-copy-code"));
        let open = render("```py\nprint(1)");
        assert!(open.contains("language-py"));
    }

    #[test]
    fn snippet_marks() {
        assert_eq!(snippet_html("<b>[[cat]]</b>"), "&lt;b&gt;<mark>cat</mark>&lt;/b&gt;");
    }
}
