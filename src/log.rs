use parking_lot::RwLock;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};

const MAX_ENTRIES: usize = 200;

#[derive(Clone, serde::Serialize)]
pub struct LogEntry {
    pub id: u64,
    pub time: String,
    pub command: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub duration_ms: u64,
}

pub struct LogStore {
    entries: RwLock<VecDeque<LogEntry>>,
    next_id: AtomicU64,
}

impl LogStore {
    pub const fn new() -> Self {
        Self {
            entries: RwLock::new(VecDeque::new()),
            next_id: AtomicU64::new(1),
        }
    }

    pub fn push(&self, mut entry: LogEntry) {
        entry.id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let mut entries = self.entries.write();
        entries.push_back(entry);
        while entries.len() > MAX_ENTRIES {
            entries.pop_front();
        }
    }

    pub fn since(&self, id: u64) -> Vec<LogEntry> {
        self.entries.read().iter().filter(|e| e.id > id).cloned().collect()
    }

    pub fn all(&self) -> Vec<LogEntry> {
        self.entries.read().iter().cloned().collect()
    }
}
