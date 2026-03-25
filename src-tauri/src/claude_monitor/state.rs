use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Tracks all active Claude Code sessions.
#[derive(Debug, Clone)]
pub struct SessionTracker {
    sessions: Arc<RwLock<HashMap<String, SessionInfo>>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionInfo {
    pub session_id: String,
    pub cwd: String,
    pub project_name: String,
    pub status: SessionStatus,
    pub last_tool: Option<String>,
    pub started_at: String,
    pub last_activity: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Active,
    WaitingApproval,
    Idle,
    Stopped,
}

/// Payload from Claude Code hooks (common fields).
#[derive(Debug, Deserialize)]
pub struct HookPayload {
    pub session_id: Option<String>,
    pub cwd: Option<String>,
    pub hook_event_name: Option<String>,
    // SessionStart
    pub source: Option<String>,
    // PermissionRequest
    pub tool_name: Option<String>,
    pub tool_input: Option<serde_json::Value>,
    // Notification
    pub notification_type: Option<String>,
}

impl SessionTracker {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn on_session_start(&self, payload: &HookPayload) {
        let session_id = payload.session_id.clone().unwrap_or_default();
        let cwd = payload.cwd.clone().unwrap_or_default();
        let project_name = cwd
            .rsplit('/')
            .next()
            .unwrap_or("unknown")
            .to_string();

        let info = SessionInfo {
            session_id: session_id.clone(),
            cwd,
            project_name,
            status: SessionStatus::Active,
            last_tool: None,
            started_at: now(),
            last_activity: now(),
        };

        self.sessions.write().await.insert(session_id, info);
    }

    pub async fn on_permission_request(&self, payload: &HookPayload) {
        let session_id = payload.session_id.clone().unwrap_or_default();
        let mut sessions = self.sessions.write().await;

        if let Some(session) = sessions.get_mut(&session_id) {
            session.status = SessionStatus::WaitingApproval;
            session.last_tool = payload.tool_name.clone();
            session.last_activity = now();
        } else {
            // Session not tracked yet, create it
            let cwd = payload.cwd.clone().unwrap_or_default();
            let project_name = cwd.rsplit('/').next().unwrap_or("unknown").to_string();
            sessions.insert(
                session_id.clone(),
                SessionInfo {
                    session_id: session_id.clone(),
                    cwd,
                    project_name,
                    status: SessionStatus::WaitingApproval,
                    last_tool: payload.tool_name.clone(),
                    started_at: now(),
                    last_activity: now(),
                },
            );
        }
    }

    pub async fn on_stop(&self, payload: &HookPayload) {
        let session_id = payload.session_id.clone().unwrap_or_default();
        let mut sessions = self.sessions.write().await;

        if let Some(session) = sessions.get_mut(&session_id) {
            session.status = SessionStatus::Idle;
            session.last_activity = now();
        }
    }

    pub async fn on_notification(&self, payload: &HookPayload) {
        // permission_prompt notifications also indicate waiting
        if payload.notification_type.as_deref() == Some("permission_prompt") {
            self.on_permission_request(payload).await;
        }
    }

    /// Get all sessions currently waiting for approval.
    pub async fn waiting_sessions(&self) -> Vec<SessionInfo> {
        self.sessions
            .read()
            .await
            .values()
            .filter(|s| s.status == SessionStatus::WaitingApproval)
            .cloned()
            .collect()
    }

    /// Get all tracked sessions.
    pub async fn all_sessions(&self) -> Vec<SessionInfo> {
        self.sessions.read().await.values().cloned().collect()
    }
}

fn now() -> String {
    chrono::Local::now().format("%H:%M:%S").to_string()
}
