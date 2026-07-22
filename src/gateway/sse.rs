//! Minimal, resumable SSE parser (text/event-stream).
//!
//! Feed arbitrary byte chunks; complete events come out. Handles multi-line
//! `data:` accumulation, `event:` names, `id:` fields, comment lines, and
//! CRLF/CR/LF line endings. Never panics on any input.

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SseEvent {
    pub event: String,
    pub data: String,
    pub id: String,
}

#[derive(Default)]
pub struct SseParser {
    buf: Vec<u8>,
    cur_event: String,
    cur_data: Vec<String>,
    cur_id: String,
}

impl SseParser {
    pub fn new() -> SseParser {
        SseParser::default()
    }

    /// Push bytes; return every event completed by this chunk.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        self.buf.extend_from_slice(chunk);
        let mut out = Vec::new();
        while let Some(nl) = self.buf.iter().position(|b| *b == b'\n' || *b == b'\r') {
            let line: Vec<u8> = self.buf.drain(0..nl).collect();
            // Swallow the terminator (and the LF of a CRLF pair).
            let first = self.buf.remove(0);
            if first == b'\r' && self.buf.first() == Some(&b'\n') {
                self.buf.remove(0);
            }
            let line = String::from_utf8_lossy(&line).to_string();
            if line.is_empty() {
                if !self.cur_data.is_empty() || !self.cur_event.is_empty() {
                    out.push(SseEvent {
                        event: if self.cur_event.is_empty() {
                            "message".into()
                        } else {
                            std::mem::take(&mut self.cur_event)
                        },
                        data: self.cur_data.join("\n"),
                        id: std::mem::take(&mut self.cur_id),
                    });
                    self.cur_event.clear();
                    self.cur_data.clear();
                } else {
                    self.cur_id.clear();
                }
                continue;
            }
            if line.starts_with(':') {
                continue; // comment / keep-alive
            }
            let (field, value) = match line.split_once(':') {
                Some((f, v)) => (f, v.strip_prefix(' ').unwrap_or(v)),
                None => (line.as_str(), ""),
            };
            match field {
                "event" => self.cur_event = value.to_string(),
                "data" => self.cur_data.push(value.to_string()),
                "id" => self.cur_id = value.to_string(),
                _ => {}
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_step_events_across_chunk_splits() {
        let mut p = SseParser::new();
        let payload =
            b"id: 1\nevent: step\ndata: {\"cursor\": 1}\n\nid: 2\nevent: step\ndata: {\"cur";
        let mut events = p.push(payload);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event, "step");
        assert_eq!(events[0].data, "{\"cursor\": 1}");
        assert_eq!(events[0].id, "1");
        events = p.push(b"sor\": 2}\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "{\"cursor\": 2}");
    }

    #[test]
    fn keep_alive_comments_are_silent() {
        let mut p = SseParser::new();
        assert!(p.push(b": keep-alive\n\n").is_empty());
    }

    #[test]
    fn multi_line_data_joins_with_newline() {
        let mut p = SseParser::new();
        let events = p.push(b"data: a\ndata: b\n\n");
        assert_eq!(events[0].data, "a\nb");
        assert_eq!(events[0].event, "message");
    }

    #[test]
    fn crlf_line_endings() {
        let mut p = SseParser::new();
        let events = p.push(b"event: done\r\ndata: x\r\n\r\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event, "done");
        assert_eq!(events[0].data, "x");
    }

    #[test]
    fn batched_events_in_one_chunk() {
        let mut p = SseParser::new();
        let events =
            p.push(b"event: step\ndata: 1\n\nevent: step\ndata: 2\n\nevent: done\ndata: {}\n\n");
        assert_eq!(events.len(), 3);
        assert_eq!(events[2].event, "done");
    }
}
