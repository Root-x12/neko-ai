//! Minimal incremental Server-Sent Events parser.
//!
//! Used by the server (reading provider streams) and the web client (reading our own stream).
//! Bytes can arrive split anywhere, including inside a UTF-8 character.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: String,
}

#[derive(Debug, Default)]
pub struct SseParser {
    buf: Vec<u8>,
}

impl SseParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds a chunk and returns every event it completed.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        self.buf.extend(chunk.iter().copied().filter(|&b| b != b'\r'));
        let mut out = Vec::new();
        while let Some(pos) = find(&self.buf, b"\n\n") {
            let block: Vec<u8> = self.buf.drain(..pos + 2).collect();
            if let Some(ev) = parse_block(&block[..pos]) {
                out.push(ev);
            }
        }
        out
    }

    /// Returns a trailing event that was not terminated by a blank line.
    pub fn finish(&mut self) -> Option<SseEvent> {
        let block = std::mem::take(&mut self.buf);
        parse_block(&block)
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn parse_block(block: &[u8]) -> Option<SseEvent> {
    let text = String::from_utf8_lossy(block);
    let mut event = None;
    let mut data: Option<String> = None;
    for line in text.split('\n') {
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        let (field, value) = match line.split_once(':') {
            Some((f, v)) => (f, v.strip_prefix(' ').unwrap_or(v)),
            None => (line, ""),
        };
        match field {
            "event" => event = Some(value.to_string()),
            "data" => match &mut data {
                Some(d) => {
                    d.push('\n');
                    d.push_str(value);
                }
                None => data = Some(value.to_string()),
            },
            _ => {}
        }
    }
    data.map(|data| SseEvent { event, data })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_split_chunks() {
        let mut p = SseParser::new();
        assert!(p.push(b"event: ping\nda").is_empty());
        let evs = p.push(b"ta: {\"a\":1}\n\ndata: two\n");
        assert_eq!(evs, vec![SseEvent { event: Some("ping".into()), data: "{\"a\":1}".into() }]);
        let evs = p.push(b"\n");
        assert_eq!(evs, vec![SseEvent { event: None, data: "two".into() }]);
    }

    #[test]
    fn handles_crlf_comments_and_multiline() {
        let mut p = SseParser::new();
        let evs = p.push(b": keep-alive\r\n\r\ndata: a\r\ndata: b\r\n\r\n");
        assert_eq!(evs, vec![SseEvent { event: None, data: "a\nb".into() }]);
    }

    #[test]
    fn keeps_utf8_split_across_chunks() {
        let mut p = SseParser::new();
        let bytes = "data: héllo 🐱\n\n".as_bytes();
        let (a, b) = bytes.split_at(8); // inside "é"
        assert!(p.push(a).is_empty());
        assert_eq!(p.push(b)[0].data, "héllo 🐱");
    }

    #[test]
    fn finish_returns_unterminated_event() {
        let mut p = SseParser::new();
        assert!(p.push(b"data: tail").is_empty());
        assert_eq!(p.finish().unwrap().data, "tail");
        assert!(p.finish().is_none());
    }
}
