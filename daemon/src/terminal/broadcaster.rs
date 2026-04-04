use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::broadcast;

use crate::store::chunks::TerminalChunkRecord;

const BROADCAST_CAPACITY: usize = 256;

pub struct SessionBroadcaster {
    sender: broadcast::Sender<TerminalChunkRecord>,
    subscriber_count: AtomicUsize,
}

impl SessionBroadcaster {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(BROADCAST_CAPACITY);
        Self {
            sender,
            subscriber_count: AtomicUsize::new(0),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<TerminalChunkRecord> {
        self.subscriber_count.fetch_add(1, Ordering::SeqCst);
        self.sender.subscribe()
    }

    pub fn unsubscribe(&self) {
        self.subscriber_count.fetch_sub(1, Ordering::SeqCst);
    }

    pub fn send(&self, chunk: TerminalChunkRecord) {
        let _ = self.sender.send(chunk);
    }

    pub fn subscriber_count(&self) -> usize {
        self.subscriber_count.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_chunk(data: &str) -> TerminalChunkRecord {
        TerminalChunkRecord {
            id: 1,
            session_id: "s1".to_string(),
            stream: "stdout".to_string(),
            chunk: data.to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    #[tokio::test]
    async fn test_broadcast_to_multiple_subscribers() {
        let broadcaster = SessionBroadcaster::new();
        let mut rx1 = broadcaster.subscribe();
        let mut rx2 = broadcaster.subscribe();

        broadcaster.send(test_chunk("hello"));

        let c1 = rx1.recv().await.unwrap();
        let c2 = rx2.recv().await.unwrap();
        assert_eq!(c1.chunk, "hello");
        assert_eq!(c2.chunk, "hello");
    }

    #[tokio::test]
    async fn test_send_with_no_subscribers() {
        let broadcaster = SessionBroadcaster::new();
        // Should not panic even with no receivers
        broadcaster.send(test_chunk("nobody listening"));
    }

    #[test]
    fn test_subscriber_count_tracking() {
        let broadcaster = SessionBroadcaster::new();
        assert_eq!(broadcaster.subscriber_count(), 0);

        let _rx1 = broadcaster.subscribe();
        assert_eq!(broadcaster.subscriber_count(), 1);

        let _rx2 = broadcaster.subscribe();
        assert_eq!(broadcaster.subscriber_count(), 2);

        broadcaster.unsubscribe();
        assert_eq!(broadcaster.subscriber_count(), 1);

        broadcaster.unsubscribe();
        assert_eq!(broadcaster.subscriber_count(), 0);
    }
}
