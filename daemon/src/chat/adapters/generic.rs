use super::{ChatAdapter, ParsedMessage};

pub struct GenericAdapter {
    buffer: String,
}

impl GenericAdapter {
    pub fn new() -> Self { Self { buffer: String::new() } }
}

impl ChatAdapter for GenericAdapter {
    fn feed(&mut self, text: &str) -> Vec<ParsedMessage> {
        self.buffer.push_str(text);
        vec![]
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
