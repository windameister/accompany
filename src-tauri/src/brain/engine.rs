use base64::Engine;
use tauri::{AppHandle, Emitter};

use super::queue::{BrainEvent, EventQueue, EventSource, Priority};
use crate::agent::tts::TtsClient;

const ALERT_VOICE: &str = "Chinese (Mandarin)_Cute_Spirit";

/// The brain engine: consumes events, decides what/when/how to tell the user.
pub async fn run(app: AppHandle, queue: EventQueue, tts: TtsClient) {
    tracing::info!("Brain engine started");

    loop {
        // Wait for and collect a batch of events
        let events = queue.drain_batch().await;
        if events.is_empty() {
            continue;
        }

        // Don't output anything during onboarding — queue events silently
        if !crate::soul::is_onboarded() {
            tracing::debug!("Brain: {} events queued during onboarding, holding", events.len());
            // Re-queue them for after onboarding
            for e in events {
                queue.push(e).await;
            }
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            continue;
        }

        tracing::info!("Brain processing {} events", events.len());

        // Compose a unified message from the batch
        let (message, mood, should_speak) = compose_message(&events);

        // Update character mood
        let _ = app.emit("character-mood", mood);

        // Show speech bubble
        let _ = app.emit("brain-message", serde_json::json!({
            "message": message,
            "event_count": events.len(),
            "has_urgent": events.iter().any(|e| e.priority == Priority::Urgent),
        }));

        // TTS if should speak
        if should_speak {
            match tts.synthesize(&message, ALERT_VOICE).await {
                Ok(bytes) => {
                    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                    let _ = app.emit("tts-audio", serde_json::json!({
                        "seq": 0,
                        "audio": b64,
                        "source": "brain",
                    }));
                }
                Err(e) => tracing::warn!("Brain TTS failed: {}", e),
            }
        }

        // Settle after output — don't spam
        let cooldown = if events.iter().any(|e| e.priority == Priority::Urgent) {
            2000 // Short cooldown for urgent
        } else {
            5000 // Normal cooldown
        };
        tokio::time::sleep(tokio::time::Duration::from_millis(cooldown)).await;
    }
}

/// Compose a natural message from a batch of events.
fn compose_message(events: &[BrainEvent]) -> (String, &'static str, bool) {
    if events.is_empty() {
        return (String::new(), "idle", false);
    }

    // Single event — just use its summary
    if events.len() == 1 {
        let e = &events[0];
        let mood = match e.priority {
            Priority::Urgent => "alert",
            Priority::High => "alert",
            Priority::Normal => "happy",
            Priority::Low => "idle",
        };
        let speak = e.priority >= Priority::Normal;
        return (e.summary.clone(), mood, speak);
    }

    // Multiple events — compose a summary
    let urgent: Vec<&BrainEvent> = events.iter().filter(|e| e.priority >= Priority::High).collect();
    let normal: Vec<&BrainEvent> = events.iter().filter(|e| e.priority == Priority::Normal).collect();
    let low: Vec<&BrainEvent> = events.iter().filter(|e| e.priority == Priority::Low).collect();

    let mut parts = Vec::new();

    if !urgent.is_empty() {
        if urgent.len() == 1 {
            parts.push(urgent[0].summary.clone());
        } else {
            // Group by category
            let approval_count = urgent.iter().filter(|e| e.category == "approval").count();
            let failure_count = urgent.iter().filter(|e| e.category == "deploy_failure").count();

            if approval_count > 0 {
                parts.push(format!("有{}个 Claude session 在等你批准喵", approval_count));
            }
            if failure_count > 0 {
                parts.push(format!("{}个部署失败了", failure_count));
            }
            // Other urgent items
            for e in urgent.iter().filter(|e| e.category != "approval" && e.category != "deploy_failure") {
                parts.push(e.summary.clone());
            }
        }
    }

    if !normal.is_empty() {
        let deploy_ok = normal.iter().filter(|e| e.category == "deploy_success").count();
        if deploy_ok > 0 {
            parts.push(format!("{}个部署成功了", deploy_ok));
        }
        for e in normal.iter().filter(|e| e.category != "deploy_success") {
            parts.push(e.summary.clone());
        }
    }

    // Low priority — only mention if nothing else
    if parts.is_empty() && !low.is_empty() {
        parts.push(low[0].summary.clone());
    }

    let message = if parts.len() == 1 {
        parts[0].clone()
    } else {
        format!("主人，有几件事跟你说喵~ {}", parts.join("。"))
    };

    let mood = if !urgent.is_empty() { "alert" } else { "happy" };
    let speak = !urgent.is_empty() || !normal.is_empty();

    (message, mood, speak)
}
