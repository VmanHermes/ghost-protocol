use super::{AdapterEvent, ChatAdapter, ParsedMessage};

pub struct GenericAdapter {
    line_buffer: String,
    message_buffer: String,
}

impl GenericAdapter {
    pub fn new() -> Self { Self { line_buffer: String::new(), message_buffer: String::new() } }
}

impl ChatAdapter for GenericAdapter {
    fn feed(&mut self, text: &str) -> Vec<AdapterEvent> {
        self.line_buffer.push_str(text);
        let mut events = Vec::new();
        while let Some(pos) = self.line_buffer.find('\n') {
            let line: String = self.line_buffer.drain(..=pos).collect();
            self.message_buffer.push_str(&line);
            events.push(AdapterEvent::Delta(line));
        }
        events
    }

    fn flush(&mut self) -> Vec<AdapterEvent> {
        let mut events = Vec::new();
        if !self.line_buffer.is_empty() {
            self.message_buffer.push_str(&self.line_buffer);
            events.push(AdapterEvent::Delta(std::mem::take(&mut self.line_buffer)));
        }
        if !self.message_buffer.is_empty() {
            events.push(AdapterEvent::Message(ParsedMessage {
                role: "assistant".into(),
                content: std::mem::take(&mut self.message_buffer).trim().to_string(),
            }));
        }
        events
    }
}
