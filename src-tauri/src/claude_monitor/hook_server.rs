use axum::{extract::State, http::StatusCode, middleware, response::IntoResponse, routing::post, Json, Router};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};

use super::state::{HookPayload, SessionTracker};
use crate::brain::queue::{BrainEvent, EventQueue, EventSource, Priority};

/// Token file path for hook authentication.
fn token_path() -> std::path::PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join("accompany")
        .join("hook_token")
}

/// Get or create the hook auth token. Reuses existing token to avoid
/// invalidating already-installed hooks on restart.
pub fn get_or_create_token() -> String {
    let path = token_path();
    // Try to read existing token
    if let Ok(token) = std::fs::read_to_string(&path) {
        let token = token.trim().to_string();
        if !token.is_empty() {
            return token;
        }
    }
    // Generate new token only if none exists
    let token = ulid::Ulid::new().to_string();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, &token);
    token
}

/// Read the current hook token.
pub fn read_hook_token() -> Option<String> {
    std::fs::read_to_string(token_path()).ok().map(|s| s.trim().to_string())
}

#[derive(Clone)]
struct HookState {
    tracker: SessionTracker,
    app: AppHandle,
    brain: EventQueue,
    token: String,
}

pub async fn start_hook_server(app: AppHandle, tracker: SessionTracker, brain: EventQueue) {
    let token = get_or_create_token();
    tracing::info!("Hook auth token ready");

    let state = HookState {
        tracker,
        app,
        brain,
        token: token.clone(),
    };

    let token_for_check = token.clone();
    let auth_check = move |req: axum::http::Request<axum::body::Body>, next: middleware::Next| {
        let expected = token_for_check.clone();
        async move {
            let provided = req.headers()
                .get("X-Accompany-Token")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            // Accept if: token matches, OR no token provided (legacy hooks without auth)
            // This allows old hooks to keep working until they're reinstalled with token
            if !provided.is_empty() && provided != expected {
                tracing::warn!("Hook request rejected: invalid token");
                return Ok(StatusCode::UNAUTHORIZED.into_response());
            }
            Ok::<_, std::convert::Infallible>(next.run(req).await)
        }
    };

    let router = Router::new()
        .route("/hooks/session-start", post(handle_session_start))
        .route("/hooks/permission-request", post(handle_permission_request))
        .route("/hooks/notification", post(handle_notification))
        .route("/hooks/stop", post(handle_stop))
        .layer(middleware::from_fn(auth_check))
        .with_state(state);

    let addr = "127.0.0.1:17832";
    tracing::info!("Hook server listening on {}", addr);

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("Failed to bind hook server on {}: {}", addr, e);
            return;
        }
    };

    if let Err(e) = axum::serve(listener, router).await {
        tracing::error!("Hook server error: {}", e);
    }
}

/// Build a detailed, human-friendly alert message.
fn build_alert_message(project: &str, tool: Option<&str>, tool_input: Option<&serde_json::Value>) -> String {
    let tool_desc = match tool {
        Some("Bash") => {
            let cmd = tool_input
                .and_then(|v| v.get("command"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let short_cmd: String = cmd.chars().take(40).collect();
            format!("执行命令: {}", short_cmd)
        }
        Some("Edit") | Some("Write") => {
            let path = tool_input
                .and_then(|v| v.get("file_path"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let filename = path.rsplit('/').next().unwrap_or(path);
            format!("修改文件: {}", filename)
        }
        Some(t) => format!("使用 {}", t),
        None => "某个操作".to_string(),
    };

    format!("{}项目的 Claude 想要{}，需要你批准喵！", project, tool_desc)
}

/// Push an alert event into the brain queue (non-blocking, no direct TTS).
fn push_alert(state: &HookState, session_id: &str, project: &str, tool: &str, message: &str) {
    let event = BrainEvent::new(
        EventSource::Claude,
        Priority::Urgent,
        "approval",
        message,
    )
    .with_details(serde_json::json!({
        "session_id": session_id,
        "project": project,
        "tool": tool,
    }))
    .with_dedup(&format!("claude_approval_{}", session_id));

    let brain = state.brain.clone();
    tokio::spawn(async move {
        brain.push(event).await;
    });
}

async fn handle_session_start(
    State(state): State<HookState>,
    Json(payload): Json<HookPayload>,
) -> StatusCode {
    tracing::info!(
        "Session started: {} in {}",
        payload.session_id.as_deref().unwrap_or("?"),
        payload.cwd.as_deref().unwrap_or("?")
    );
    state.tracker.on_session_start(&payload).await;
    let _ = state.app.emit("claude-session-update", "session_start");
    StatusCode::OK
}

async fn handle_permission_request(
    State(state): State<HookState>,
    Json(payload): Json<HookPayload>,
) -> StatusCode {
    let session_id = payload.session_id.as_deref().unwrap_or("?");
    let tool = payload.tool_name.as_deref().unwrap_or("?");
    tracing::warn!("Approval needed: session={}, tool={}", session_id, tool);

    state.tracker.on_permission_request(&payload).await;

    // Build detailed alert
    let sessions = state.tracker.waiting_sessions().await;
    let session = sessions.iter().find(|s| s.session_id == session_id);
    let project = session.map(|s| s.project_name.as_str()).unwrap_or("unknown");

    let alert_msg = build_alert_message(
        project,
        payload.tool_name.as_deref(),
        payload.tool_input.as_ref(),
    );

    push_alert(&state, session_id, project, tool, &alert_msg);

    StatusCode::OK
}

async fn handle_notification(
    State(state): State<HookState>,
    Json(payload): Json<HookPayload>,
) -> StatusCode {
    let ntype = payload.notification_type.as_deref().unwrap_or("?");
    tracing::info!("Notification: type={}", ntype);

    state.tracker.on_notification(&payload).await;

    // PermissionRequest already handles the full alert with TTS.
    // Notification just updates state — no duplicate TTS.
    if ntype == "permission_prompt" {
        tracing::debug!("permission_prompt notification (TTS already sent via PermissionRequest)");
    }

    StatusCode::OK
}

async fn handle_stop(
    State(state): State<HookState>,
    Json(payload): Json<HookPayload>,
) -> StatusCode {
    tracing::info!(
        "Session stopped: {}",
        payload.session_id.as_deref().unwrap_or("?")
    );
    state.tracker.on_stop(&payload).await;
    let _ = state.app.emit("claude-session-update", "session_stop");
    StatusCode::OK
}
