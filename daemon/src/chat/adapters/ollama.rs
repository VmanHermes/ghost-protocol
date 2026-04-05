use super::{ChatAdapter, ParsedMessage};

pub struct OllamaAdapter {
    buffer: String,
}

impl OllamaAdapter {
    pub fn new() -> Self { Self { buffer: String::new() } }
}

impl ChatAdapter for OllamaAdapter {
    fn feed(&mut self, text: &str) -> Vec<ParsedMessage> {
        self.buffer.push_str(text);
        let mut messages = vec![];
        while let Some(idx) = self.buffer.find(">>> ") {
            let content = self.buffer[..idx].trim().to_string();
            self.buffer = self.buffer[idx + 4..].to_string();
            if !content.is_empty() {
                messages.push(ParsedMessage { role: "assistant".into(), content });
            }
        }
        messages
    }
    fn flush(&mut self) -> Vec<ParsedMessage> {
        if self.buffer.trim().is_empty() {
            self.buffer.clear();
            return vec![];
        }
        let content = std::mem::take(&mut self.buffer).trim().to_string();
        vec![ParsedMessage { role: "assistant".into(), content }]
    }
}
