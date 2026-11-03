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

/// Content the canvas can render.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactKind {
    Html,
    Svg,
    Mermaid,
    React,
}

impl ArtifactKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ArtifactKind::Html => "html",
            ArtifactKind::Svg => "svg",
            ArtifactKind::Mermaid => "mermaid",
            ArtifactKind::React => "react",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ArtifactKind::Html => "HTML",
            ArtifactKind::Svg => "SVG",
            ArtifactKind::Mermaid => "Diagram",
            ArtifactKind::React => "React",
        }
    }

    pub fn file_ext(self) -> &'static str {
        match self {
            ArtifactKind::Html => "html",
            ArtifactKind::Svg => "svg",
            ArtifactKind::Mermaid => "mmd",
            ArtifactKind::React => "jsx",
        }
    }

    /// highlight.js language for the code view.
    pub fn code_lang(self) -> &'static str {
        match self {
            ArtifactKind::Html | ArtifactKind::Svg => "xml",
            ArtifactKind::Mermaid => "plaintext",
            ArtifactKind::React => "javascript",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Artifact {
    pub kind: ArtifactKind,
    pub code: String,
}

fn clean_lang(lang: &str) -> String {
    lang.split(|c: char| c.is_whitespace() || c == ',' || c == '{')
        .next()
        .unwrap_or("")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '#' | '-' | '_'))
        .take(20)
        .collect::<String>()
        .to_ascii_lowercase()
}

pub fn artifact_kind(lang: &str, code: &str) -> Option<ArtifactKind> {
    let head = code.trim_start();
    let starts_svg = head.starts_with("<svg") || (head.starts_with("<?xml") && head.contains("<svg"));
    match clean_lang(lang).as_str() {
        "html" | "htm" | "xhtml" => Some(ArtifactKind::Html),
        "svg" => Some(ArtifactKind::Svg),
        "xml" if starts_svg => Some(ArtifactKind::Svg),
        "mermaid" | "mmd" => Some(ArtifactKind::Mermaid),
        "jsx" | "tsx" | "react" => Some(ArtifactKind::React),
        _ => None,
    }
}

fn code_block(lang: &str, code: &str, preview_index: Option<usize>) -> String {
    let lang = clean_lang(lang);
    let class = if lang.is_empty() { String::new() } else { format!(" class=\"language-{lang}\"") };
    let label = if lang.is_empty() { "code".to_string() } else { lang };
    let preview = preview_index
        .map(|i| {
            format!("<button type=\"button\" class=\"copy-btn preview-btn\" data-preview=\"{i}\">▶ Preview</button>")
        })
        .unwrap_or_default();
    format!(
        "<div class=\"code-block\"><div class=\"code-head\"><span>{label}</span><span class=\"code-actions\">{preview}\
         <button type=\"button\" class=\"copy-btn\" data-copy-code>Copy</button></span></div>\
         <pre><code{class}>{}</code></pre></div>",
        escape(code)
    )
}

/// pulldown-cmark closes an unterminated fence at the end of input; a block still being
/// streamed must not count as finished.
fn fence_closed(md: &str, range: std::ops::Range<usize>) -> bool {
    let src = md.get(range).unwrap_or("").trim_end();
    let first = src.lines().next().unwrap_or("").trim_start();
    let fence: String = first.chars().take_while(|c| *c == '`' || *c == '~').collect();
    !fence.is_empty() && src.lines().count() > 1 && src.lines().last().is_some_and(|l| l.trim().starts_with(&fence))
}

/// Every previewable code block in a message, in order (matches the `data-preview` indexes).
pub fn artifacts(md: &str) -> Vec<Artifact> {
    let mut out = Vec::new();
    let mut code: Option<(String, String)> = None;
    for (ev, range) in Parser::new_ext(md, Options::empty()).into_offset_iter() {
        match (&mut code, ev) {
            (None, Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(l)))) if fence_closed(md, range) => {
                code = Some((l.to_string(), String::new()))
            }
            (Some((_, text)), Event::Text(t)) => text.push_str(&t),
            (Some(_), Event::End(TagEnd::CodeBlock)) => {
                let (lang, text) = code.take().unwrap_or_default();
                if let Some(kind) = artifact_kind(&lang, &text) {
                    out.push(Artifact { kind, code: text });
                }
            }
            _ => {}
        }
    }
    out
}

pub fn render(md: &str) -> String {
    let opts = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut events: Vec<Event> = Vec::new();
    let mut code: Option<(String, String, bool)> = None; // (lang, text, fence closed)
    let mut previews = 0usize;
    let mut block = |lang: &str, text: &str, closed: bool| {
        let idx = artifact_kind(lang, text).filter(|_| closed).map(|_| {
            previews += 1;
            previews - 1
        });
        code_block(lang, text, idx)
    };

    for (ev, range) in Parser::new_ext(md, opts).into_offset_iter() {
        if let Some((_, text, _)) = &mut code {
            match ev {
                Event::Text(t) => text.push_str(&t),
                Event::End(TagEnd::CodeBlock) => {
                    let (lang, text, closed) = code.take().unwrap();
                    events.push(Event::Html(CowStr::from(block(&lang, &text, closed))));
                }
                _ => {}
            }
            continue;
        }
        match ev {
            Event::Start(Tag::CodeBlock(kind)) => {
                let (lang, closed) = match kind {
                    CodeBlockKind::Fenced(l) => (l.to_string(), fence_closed(md, range)),
                    CodeBlockKind::Indented => (String::new(), false),
                };
                code = Some((lang, String::new(), closed));
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
    if let Some((lang, text, _)) = code {
        events.push(Event::Html(CowStr::from(code_block(&lang, &text, None))));
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
    fn finds_artifacts() {
        let md = "Here:\n```html\n<h1>Hi</h1>\n```\n```rust\nfn x(){}\n```\n```xml\n<svg></svg>\n```\n```mermaid\ngraph TD; A-->B\n```\n```tsx\nexport default function App(){return <b/>}\n```";
        let a = artifacts(md);
        let kinds: Vec<_> = a.iter().map(|x| x.kind).collect();
        assert_eq!(kinds, vec![ArtifactKind::Html, ArtifactKind::Svg, ArtifactKind::Mermaid, ArtifactKind::React]);
        assert_eq!(a[0].code, "<h1>Hi</h1>\n");
        let html = render(md);
        assert!(html.contains("data-preview=\"0\"") && html.contains("data-preview=\"3\""));
        assert_eq!(html.matches("data-preview=").count(), 4, "rust block has no preview button");
        // A block still streaming (no closing fence) gets no preview button yet.
        assert!(!render("```html\n<p>").contains("data-preview"));
    }

    #[test]
    fn snippet_marks() {
        assert_eq!(snippet_html("<b>[[cat]]</b>"), "&lt;b&gt;<mark>cat</mark>&lt;/b&gt;");
    }
}
