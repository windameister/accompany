use axum::{extract::State, http::StatusCode, routing::post, Json, Router};
use base64::Engine;
use tauri::{AppHandle, Emitter};

use super::state::{HookPayload, SessionTracker};
use crate::agent::tts::TtsClient;

const ALERT_VOICE: &str = "Chinese (Mandarin)_Cute_Spirit";

#[derive(Clone)]
struct HookState {
    tracker: SessionTracker,
    app: AppHandle,
    tts: TtsClient,
}

pub async fn start_hook_server(app: AppHandle, tracker: SessionTracker, tts: TtsClient) {
    let state = HookState { tracker, app, tts };

    let router = Router::new()
        .route("/hooks/session-start", post(handle_session_start))
        .route("/hooks/permission-request", post(handle_permission_request))
        .route("/hooks/notification", post(handle_notification))
        .route("/hooks/stop", post(handle_stop))
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

/// Emit alert event + TTS audio to frontend.
async fn emit_alert(state: &HookState, session_id: &str, project: &str, tool: &str, message: &str, waiting_count: usize) {
    let _ = state.app.emit("claude-needs-approval", serde_json::json!({
        "session_id": session_id,
        "project": project,
        "tool": tool,
        "message": message,
        "waiting_count": waiting_count,
    }));
    let _ = state.app.emit("character-mood", "alert");

    // Generate TTS and emit audio
    match state.tts.synthesize(message, ALERT_VOICE).await {
        Ok(bytes) => {
            let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
            let _ = state.app.emit("tts-audio", serde_json::json!({
                "seq": 0,
                "audio": b64,
            }));
            tracing::info!("Alert TTS: {} bytes", bytes.len());
        }
        Err(e) => {
            tracing::warn!("Alert TTS failed: {}", e);
        }
    }
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

    emit_alert(&state, session_id, project, tool, &alert_msg, sessions.len()).await;

    StatusCode::OK
}

async fn handle_notification(
    State(state): State<HookState>,
    Json(payload): Json<HookPayload>,
) -> StatusCode {
    let ntype = payload.notification_type.as_deref().unwrap_or("?");
    tracing::info!("Notification: type={}", ntype);

    state.tracker.on_notification(&payload).await;

    if ntype == "permission_prompt" {
        let session_id = payload.session_id.as_deref().unwrap_or("?");
        let sessions = state.tracker.waiting_sessions().await;

        // Find this session's details (tool info from earlier PermissionRequest)
        let session = sessions.iter().find(|s| s.session_id == session_id);

        let alert_msg = if let Some(s) = session {
            let tool_desc = s.last_tool.as_deref().unwrap_or("某个操作");
            format!("{}项目的 Claude 想要使用 {}，需要你批准喵！", s.project_name, tool_desc)
        } else {
            // Fallback: use cwd
            let project = payload.cwd.as_deref()
                .and_then(|p| p.rsplit('/').next())
                .unwrap_or("unknown");
            format!("{}项目的 Claude 在等你操作喵！", project)
        };

        let project = session.map(|s| s.project_name.as_str())
            .or_else(|| payload.cwd.as_deref().and_then(|p| p.rsplit('/').next()))
            .unwrap_or("unknown");
        let tool = session.and_then(|s| s.last_tool.as_deref()).unwrap_or("unknown");

        emit_alert(&state, session_id, project, tool, &alert_msg, sessions.len()).await;
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
