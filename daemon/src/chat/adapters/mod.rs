pub mod generic;
pub mod claude;
pub mod ollama;

pub struct ParsedMessage {
    pub role: String,
    pub content: String,
}

pub trait ChatAdapter: Send + Sync {
    fn feed(&mut self, text: &str) -> Vec<ParsedMessage>;
    fn flush(&mut self) -> Vec<ParsedMessage>;
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
