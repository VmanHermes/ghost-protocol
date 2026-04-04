use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use serde::Serialize;

const DEFAULT_CAPACITY: usize = 1000;

#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    pub level: String,
    pub message: String,
    pub timestamp: String,
    pub source: String,
}

#[derive(Clone)]
pub struct LogBuffer {
    entries: Arc<Mutex<VecDeque<LogEntry>>>,
    capacity: usize,
}

impl LogBuffer {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(Mutex::new(VecDeque::with_capacity(DEFAULT_CAPACITY))),
            capacity: DEFAULT_CAPACITY,
        }
    }

    #[cfg(test)]
    fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: Arc::new(Mutex::new(VecDeque::with_capacity(capacity))),
            capacity,
        }
    }

    pub fn push(&self, entry: LogEntry) {
        let mut entries = self.entries.lock().unwrap();
        if entries.len() >= self.capacity {
            entries.pop_front();
        }
        entries.push_back(entry);
    }

    pub fn entries(&self, limit: usize, level: Option<&str>) -> Vec<LogEntry> {
        let entries = self.entries.lock().unwrap();
        let iter = entries.iter().filter(|e| match level {
            Some(lvl) => e.level.eq_ignore_ascii_case(lvl),
            None => true,
        });
        // Take the *last* `limit` matching entries to return most recent, but in
        // chronological (oldest-first) order.
        let matching: Vec<LogEntry> = iter.cloned().collect();
        let start = matching.len().saturating_sub(limit);
        matching[start..].to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(level: &str, message: &str) -> LogEntry {
        LogEntry {
            level: level.to_string(),
            message: message.to_string(),
            timestamp: "2026-01-01T00:00:00Z".to_string(),
            source: "test".to_string(),
        }
    }

    #[test]
    fn test_push_and_retrieve() {
        let buf = LogBuffer::new();
        buf.push(entry("info", "hello"));
        buf.push(entry("error", "boom"));

        let all = buf.entries(10, None);
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].message, "hello");
        assert_eq!(all[1].message, "boom");

        let errors = buf.entries(10, Some("ERROR"));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].message, "boom");
    }

    #[test]
    fn test_capacity_eviction() {
        let buf = LogBuffer::with_capacity(3);
        for i in 0..5 {
            buf.push(entry("info", &format!("msg{}", i)));
        }

        let all = buf.entries(10, None);
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].message, "msg2");
        assert_eq!(all[1].message, "msg3");
        assert_eq!(all[2].message, "msg4");
    }
}
