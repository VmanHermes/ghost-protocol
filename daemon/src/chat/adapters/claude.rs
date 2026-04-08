use std::collections::HashMap;

use super::{AdapterEvent, ChatAdapter, ParsedMessage};

pub struct ClaudeAdapter {
    line_buffer: String,
    current_message: String,
    block_types: HashMap<u64, String>,
}

impl ClaudeAdapter {
    pub fn new() -> Self {
        Self {
            line_buffer: String::new(),
            current_message: String::new(),
            block_types: HashMap::new(),
        }
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

        let block_index = event.get("index").and_then(|index| index.as_u64());

        match event.get("type").and_then(|t| t.as_str()) {
            Some("assistant") | Some("message_start") => {
                events.push(AdapterEvent::Status("thinking".into()));
            }
            Some("content_block_start") => {
                if let Some(block_type) = event.pointer("/content_block/type").and_then(|t| t.as_str()) {
                    if let Some(index) = block_index {
                        self.block_types.insert(index, block_type.to_string());
                    }
                    if block_type == "tool_use" {
                        events.push(AdapterEvent::Status("tool_use".into()));
                    }
                }
            }
            Some("content_block_delta") => {
                let delta_type = event.pointer("/delta/type").and_then(|t| t.as_str());
                let block_type = block_index
                    .and_then(|index| self.block_types.get(&index))
                    .map(|value| value.as_str());
                let is_text_delta = matches!(block_type, Some("text")) || delta_type == Some("text_delta");

                if is_text_delta {
                    if let Some(text) = event.pointer("/delta/text").and_then(|t| t.as_str()) {
                        self.current_message.push_str(text);
                        events.push(AdapterEvent::Delta(text.to_string()));
                    }
                }
            }
            Some("content_block_stop") => {
                if let Some(index) = block_index {
                    self.block_types.remove(&index);
                }
            }
            Some("message_stop") => {}
            Some("result") => {
                self.block_types.clear();
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
        self.block_types.clear();
        if !self.current_message.is_empty() {
            events.push(AdapterEvent::Message(ParsedMessage {
                role: "assistant".into(),
                content: std::mem::take(&mut self.current_message),
            }));
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use super::ClaudeAdapter;
    use crate::chat::adapters::{AdapterEvent, ChatAdapter};

    #[test]
    fn streams_text_blocks_into_assistant_messages() {
        let mut adapter = ClaudeAdapter::new();

        let thinking = adapter.feed("{\"type\":\"message_start\"}\n");
        assert!(matches!(thinking.as_slice(), [AdapterEvent::Status(status)] if status == "thinking"));

        let block_start = adapter.feed("{\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\"}}\n");
        assert!(block_start.is_empty());

        let delta = adapter.feed("{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n");
        assert!(matches!(delta.as_slice(), [AdapterEvent::Delta(text)] if text == "Hello"));

        let result = adapter.feed("{\"type\":\"result\",\"usage\":{\"input_tokens\":12,\"output_tokens\":5}}\n");
        assert!(matches!(result.as_slice(),
            [AdapterEvent::Message(message), AdapterEvent::Meta { tokens: Some(17), context_pct: None }, AdapterEvent::Status(status)]
                if message.role == "assistant" && message.content == "Hello" && status == "idle"
        ));
    }

    #[test]
    fn ignores_non_text_content_blocks() {
        let mut adapter = ClaudeAdapter::new();

        adapter.feed("{\"type\":\"message_start\"}\n");
        let thinking_start = adapter.feed("{\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\"}}\n");
        assert!(thinking_start.is_empty());

        let thinking_delta = adapter.feed("{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"text\":\"hidden reasoning\"}}\n");
        assert!(thinking_delta.is_empty());

        let tool_start = adapter.feed("{\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\"}}\n");
        assert!(matches!(tool_start.as_slice(), [AdapterEvent::Status(status)] if status == "tool_use"));

        let result = adapter.feed("{\"type\":\"result\",\"usage\":{\"output_tokens\":3}}\n");
        assert!(matches!(result.as_slice(),
            [AdapterEvent::Meta { tokens: Some(3), context_pct: None }, AdapterEvent::Status(status)]
                if status == "idle"
        ));
    }
}
