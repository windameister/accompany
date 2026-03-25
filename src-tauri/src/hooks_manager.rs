use serde_json::{json, Value};
use std::path::PathBuf;

const HOOK_PORT: u16 = 17832;

/// The hooks config block that Accompany needs in Claude settings.
fn hooks_config() -> Value {
    json!({
        "SessionStart": [{
            "matcher": "",
            "hooks": [{
                "type": "http",
                "url": format!("http://127.0.0.1:{}/hooks/session-start", HOOK_PORT),
                "timeout": 5
            }]
        }],
        "PermissionRequest": [{
            "matcher": "",
            "hooks": [{
                "type": "http",
                "url": format!("http://127.0.0.1:{}/hooks/permission-request", HOOK_PORT),
                "timeout": 5
            }]
        }],
        "Notification": [{
            "matcher": "permission_prompt",
            "hooks": [{
                "type": "http",
                "url": format!("http://127.0.0.1:{}/hooks/notification", HOOK_PORT),
                "timeout": 5
            }]
        }],
        "Stop": [{
            "matcher": "",
            "hooks": [{
                "type": "http",
                "url": format!("http://127.0.0.1:{}/hooks/stop", HOOK_PORT),
                "timeout": 5
            }]
        }]
    })
}

fn global_settings_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".claude")
        .join("settings.json")
}

/// Check if hooks are installed in global Claude settings.
pub fn is_installed_global() -> bool {
    let path = global_settings_path();
    if let Ok(content) = std::fs::read_to_string(&path) {
        if let Ok(settings) = serde_json::from_str::<Value>(&content) {
            return settings.get("hooks").is_some()
                && settings["hooks"]
                    .as_object()
                    .map(|h| !h.is_empty())
                    .unwrap_or(false);
        }
    }
    false
}

/// Install hooks into global `~/.claude/settings.json`.
pub fn install_global() -> Result<(), String> {
    let path = global_settings_path();

    // Read existing settings or create new
    let mut settings: Value = if path.exists() {
        let content = std::fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read settings: {}", e))?;
        serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse settings: {}", e))?
    } else {
        // Ensure .claude directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create .claude dir: {}", e))?;
        }
        json!({})
    };

    // Merge hooks into settings
    settings["hooks"] = hooks_config();

    let content = serde_json::to_string_pretty(&settings)
        .map_err(|e| format!("Failed to serialize: {}", e))?;
    std::fs::write(&path, content)
        .map_err(|e| format!("Failed to write settings: {}", e))?;

    tracing::info!("Hooks installed globally at {:?}", path);
    Ok(())
}

/// Remove hooks from global `~/.claude/settings.json`.
pub fn uninstall_global() -> Result<(), String> {
    let path = global_settings_path();
    if !path.exists() {
        return Ok(());
    }

    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read settings: {}", e))?;
    let mut settings: Value = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse settings: {}", e))?;

    if let Some(obj) = settings.as_object_mut() {
        obj.remove("hooks");
    }

    let content = serde_json::to_string_pretty(&settings)
        .map_err(|e| format!("Failed to serialize: {}", e))?;
    std::fs::write(&path, content)
        .map_err(|e| format!("Failed to write settings: {}", e))?;

    tracing::info!("Hooks removed from global settings");
    Ok(())
}
