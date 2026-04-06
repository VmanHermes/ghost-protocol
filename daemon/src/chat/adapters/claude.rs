use super::{AdapterEvent, ChatAdapter, ParsedMessage};

pub struct ClaudeAdapter {
    line_buffer: String,
    current_message: String,
}

impl ClaudeAdapter {
    pub fn new() -> Self {
        Self { line_buffer: String::new(), current_message: String::new() }
    }

    fn parse_line(&mut self, line: &str) -> Vec<AdapterEvent> {
        let mut events = Vec::new();
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { return events };

        // Unwrap stream_event wrapper (used with --include-partial-messages)
        let event = if v.get("type").and_then(|t| t.as_str()) == Some("stream_event") {
            v.get("event").cloned().unwrap_or(v)
        } else {
            v
        };

        match event.get("type").and_then(|t| t.as_str()) {
            Some("assistant") | Some("message_start") => {
                events.push(AdapterEvent::Status("thinking".into()));
            }
            Some("content_block_delta") => {
                if let Some(text) = event.pointer("/delta/text").and_then(|t| t.as_str()) {
                    self.current_message.push_str(text);
                    events.push(AdapterEvent::Delta(text.to_string()));
                }
            }
            Some("content_block_start") => {
                if let Some("tool_use") = event.pointer("/content_block/type").and_then(|t| t.as_str()) {
                    events.push(AdapterEvent::Status("tool_use".into()));
                }
            }
            Some("content_block_stop") | Some("message_stop") => {}
            Some("result") => {
                if !self.current_message.is_empty() {
                    events.push(AdapterEvent::Message(ParsedMessage {
                        role: "assistant".into(),
                        content: std::mem::take(&mut self.current_message),
                    }));
                }
                let tokens = event.pointer("/usage/output_tokens").and_then(|t| t.as_u64());
                let input_tokens = event.pointer("/usage/input_tokens").and_then(|t| t.as_u64());
                let total = match (tokens, input_tokens) {
                    (Some(o), Some(i)) => Some(o + i),
                    (Some(o), None) => Some(o),
                    _ => None,
                };
                if total.is_some() {
                    events.push(AdapterEvent::Meta { tokens: total, context_pct: None });
                }
                events.push(AdapterEvent::Status("idle".into()));
            }
            _ => {}
        }
        events
    }
}

impl ChatAdapter for ClaudeAdapter {
    fn feed(&mut self, text: &str) -> Vec<AdapterEvent> {
        self.line_buffer.push_str(text);
        let mut events = Vec::new();
        while let Some(pos) = self.line_buffer.find('\n') {
            let line: String = self.line_buffer.drain(..=pos).collect();
            let line = line.trim();
            if !line.is_empty() { events.extend(self.parse_line(line)); }
        }
        events
    }

    fn flush(&mut self) -> Vec<AdapterEvent> {
        let mut events = Vec::new();
        if !self.line_buffer.is_empty() {
            let remaining = std::mem::take(&mut self.line_buffer);
            let line = remaining.trim();
            if !line.is_empty() { events.extend(self.parse_line(line)); }
        }
        if !self.current_message.is_empty() {
            events.push(AdapterEvent::Message(ParsedMessage {
                role: "assistant".into(),
                content: std::mem::take(&mut self.current_message),
            }));
        }
        events
    }
}
