use super::{AdapterEvent, ChatAdapter, ParsedMessage};

pub struct OllamaAdapter {
    buffer: String,
    in_response: bool,
}

impl OllamaAdapter {
    pub fn new() -> Self { Self { buffer: String::new(), in_response: false } }
}

impl ChatAdapter for OllamaAdapter {
    fn feed(&mut self, text: &str) -> Vec<AdapterEvent> {
        let mut events = Vec::new();
        for ch in text.chars() {
            self.buffer.push(ch);
            if self.buffer.ends_with(">>> ") {
                let response = self.buffer[..self.buffer.len() - 4].to_string();
                if !response.is_empty() && self.in_response {
                    events.push(AdapterEvent::Message(ParsedMessage {
                        role: "assistant".into(), content: response.trim().to_string(),
                    }));
                    events.push(AdapterEvent::Status("idle".into()));
                }
                self.buffer.clear();
                self.in_response = true;
                continue;
            }
            if self.in_response && !self.buffer.ends_with(">>>") && !self.buffer.ends_with(">> ") {
                events.push(AdapterEvent::Delta(ch.to_string()));
            }
        }
        events
    }

    fn flush(&mut self) -> Vec<AdapterEvent> {
        let mut events = Vec::new();
        let remaining = std::mem::take(&mut self.buffer).trim().to_string();
        if !remaining.is_empty() {
            events.push(AdapterEvent::Message(ParsedMessage {
                role: "assistant".into(), content: remaining,
            }));
        }
        events
    }
}
