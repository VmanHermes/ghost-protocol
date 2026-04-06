use std::sync::atomic::{AtomicUsize, Ordering};
use serde::Serialize;
use tokio::sync::broadcast;

use crate::store::chat::ChatMessage;

const BROADCAST_CAPACITY: usize = 256;

#[derive(Debug, Clone, Serialize)]
pub enum ChatEvent {
    Delta { session_id: String, message_id: String, delta: String },
    Message { message: ChatMessage },
    Status { session_id: String, status: String },
    Meta { session_id: String, tokens: Option<u64>, context_pct: Option<f64> },
    SessionRenamed { session_id: String, name: String },
}

pub struct ChatBroadcaster {
    sender: broadcast::Sender<ChatEvent>,
    subscriber_count: AtomicUsize,
}

impl ChatBroadcaster {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(BROADCAST_CAPACITY);
        Self {
            sender,
            subscriber_count: AtomicUsize::new(0),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ChatEvent> {
        self.subscriber_count.fetch_add(1, Ordering::SeqCst);
        self.sender.subscribe()
    }

    pub fn unsubscribe(&self) {
        self.subscriber_count.fetch_sub(1, Ordering::SeqCst);
    }

    pub fn send(&self, event: ChatEvent) {
        let _ = self.sender.send(event);
    }

    pub fn subscriber_count(&self) -> usize {
        self.subscriber_count.load(Ordering::SeqCst)
    }
}
