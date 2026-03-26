use serde::Serialize;
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::{Mutex, Notify};

/// Priority levels for incoming events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum Priority {
    /// Background info — only mention when idle and nothing else pending
    Low = 0,
    /// Normal updates — GitHub success, session events
    Normal = 1,
    /// Important — needs attention soon (PR review, deploy failure)
    High = 2,
    /// Urgent — needs immediate attention (Claude approval waiting)
    Urgent = 3,
}

/// Source of the event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum EventSource {
    Claude,
    GitHub,
    User,
    System,
}

/// A single event in the brain's queue.
#[derive(Debug, Clone, Serialize)]
pub struct BrainEvent {
    pub id: String,
    pub source: EventSource,
    pub priority: Priority,
    pub category: String,       // e.g. "approval", "deploy_success", "deploy_failure"
    pub summary: String,        // Human-readable summary for TTS
    pub details: serde_json::Value, // Raw event data
    #[serde(skip)]
    pub created_at: std::time::Instant,
    pub dedup_key: Option<String>, // For merging similar events
}

impl BrainEvent {
    pub fn new(
        source: EventSource,
        priority: Priority,
        category: &str,
        summary: &str,
    ) -> Self {
        Self {
            id: ulid::Ulid::new().to_string(),
            source,
            priority,
            category: category.to_string(),
            summary: summary.to_string(),
            details: serde_json::Value::Null,
            created_at: std::time::Instant::now(),
            dedup_key: None,
        }
    }

    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = details;
        self
    }

    pub fn with_dedup(mut self, key: &str) -> Self {
        self.dedup_key = Some(key.to_string());
        self
    }
}

/// Thread-safe event queue with notification.
#[derive(Clone)]
pub struct EventQueue {
    queue: Arc<Mutex<VecDeque<BrainEvent>>>,
    notify: Arc<Notify>,
}

impl EventQueue {
    pub fn new() -> Self {
        Self {
            queue: Arc::new(Mutex::new(VecDeque::new())),
            notify: Arc::new(Notify::new()),
        }
    }

    /// Push an event and wake the brain engine.
    pub async fn push(&self, event: BrainEvent) {
        let mut q = self.queue.lock().await;

        // Dedup: if same dedup_key exists, merge (replace with higher priority)
        if let Some(key) = &event.dedup_key {
            if let Some(existing) = q.iter_mut().find(|e| e.dedup_key.as_deref() == Some(key)) {
                // Update summary with count
                let count = existing.details.get("count")
                    .and_then(|c| c.as_u64())
                    .unwrap_or(1) + 1;
                existing.summary = format!("{} (x{})", event.summary, count);
                existing.details = serde_json::json!({"count": count, "latest": event.details});
                if event.priority > existing.priority {
                    existing.priority = event.priority;
                }
                drop(q);
                self.notify.notify_one();
                return;
            }
        }

        q.push_back(event);
        drop(q);
        self.notify.notify_one();
    }

    /// Wait for events, then drain all pending events sorted by priority.
    pub async fn drain_batch(&self) -> Vec<BrainEvent> {
        // Wait until there's at least one event
        loop {
            {
                let q = self.queue.lock().await;
                if !q.is_empty() {
                    break;
                }
            }
            self.notify.notified().await;
        }

        // Small delay to batch nearby events together
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        let mut q = self.queue.lock().await;
        let mut events: Vec<BrainEvent> = q.drain(..).collect();

        // Sort by priority (urgent first)
        events.sort_by(|a, b| b.priority.cmp(&a.priority));

        events
    }

    /// Check how many events are pending.
    pub async fn len(&self) -> usize {
        self.queue.lock().await.len()
    }
}
