pub mod generic;
pub mod claude;
pub mod ollama;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ParsedMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
pub enum AdapterEvent {
    Delta(String),
    Message(ParsedMessage),
    Status(String),
    Meta { tokens: Option<u64>, context_pct: Option<f64> },
}

pub trait ChatAdapter: Send + Sync {
    fn feed(&mut self, text: &str) -> Vec<AdapterEvent>;
    fn flush(&mut self) -> Vec<AdapterEvent>;
}

pub fn adapter_for_agent(agent_id: &str) -> Box<dyn ChatAdapter> {
    if agent_id == "claude-code" || agent_id.starts_with("claude") {
        Box::new(claude::ClaudeAdapter::new())
    } else if agent_id.starts_with("ollama:") {
        Box::new(ollama::OllamaAdapter::new())
    } else {
        Box::new(generic::GenericAdapter::new())
    }
}
