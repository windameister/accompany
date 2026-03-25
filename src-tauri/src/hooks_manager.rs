use serde_json::{json, Map, Value};
use std::path::PathBuf;

const HOOK_PORT: u16 = 17832;

/// Unique marker to identify Accompany-owned hook entries.
/// Embedded as "_source" field in each hook entry.
const ACCOMPANY_MARKER: &str = "accompany-desktop-companion";

/// The hook entries Accompany needs, keyed by event name.
fn accompany_hook_entry(matcher: &str, path: &str) -> Value {
    // Read the current auth token (generated on each app startup)
    let token = crate::claude_monitor::hook_server::read_hook_token()
        .unwrap_or_default();

    json!({
        "_source": ACCOMPANY_MARKER,
        "matcher": matcher,
        "hooks": [{
            "type": "http",
            "url": format!("http://127.0.0.1:{}{}", HOOK_PORT, path),
            "timeout": 5,
            "headers": {
                "X-Accompany-Token": token
            }
        }]
    })
}

fn accompany_hooks() -> Vec<(&'static str, Value)> {
    vec![
        ("SessionStart", accompany_hook_entry("", "/hooks/session-start")),
        ("PermissionRequest", accompany_hook_entry("", "/hooks/permission-request")),
        ("Notification", accompany_hook_entry("permission_prompt", "/hooks/notification")),
        ("Stop", accompany_hook_entry("", "/hooks/stop")),
    ]
}

fn global_settings_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".claude")
        .join("settings.json")
}

/// Check if Accompany hooks are installed (by checking for our _source marker).
pub fn is_installed_global() -> bool {
    let path = global_settings_path();
    if let Ok(content) = std::fs::read_to_string(&path) {
        // Check for our specific marker field value
        return content.contains(ACCOMPANY_MARKER);
    }
    false
}

/// Install Accompany hooks by merging into existing hooks config.
/// Preserves user's other hooks.
pub fn install_global() -> Result<(), String> {
    let path = global_settings_path();

    let mut settings: Value = if path.exists() {
        let content = std::fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read settings: {}", e))?;
        serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse settings: {}", e))?
    } else {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create .claude dir: {}", e))?;
        }
        json!({})
    };

    // Ensure hooks object exists
    if !settings.get("hooks").map_or(false, |h| h.is_object()) {
        settings["hooks"] = json!({});
    }

    let hooks = settings["hooks"].as_object_mut().unwrap();

    // Merge: for each event, append our hook entry to the existing array
    for (event, entry) in accompany_hooks() {
        let arr = hooks
            .entry(event.to_string())
            .or_insert_with(|| json!([]));

        if let Some(arr) = arr.as_array_mut() {
            // Remove any existing Accompany entries (by URL marker) to avoid duplicates
            arr.retain(|item| {
                item.get("_source").and_then(|s| s.as_str()) != Some(ACCOMPANY_MARKER)
            });
            arr.push(entry);
        }
    }

    // Atomic-ish write: write to temp then rename
    let tmp_path = path.with_extension("json.tmp");
    let content = serde_json::to_string_pretty(&settings)
        .map_err(|e| format!("Failed to serialize: {}", e))?;
    std::fs::write(&tmp_path, &content)
        .map_err(|e| format!("Failed to write temp: {}", e))?;
    std::fs::rename(&tmp_path, &path)
        .map_err(|e| format!("Failed to rename: {}", e))?;

    tracing::info!("Hooks installed (merged) at {:?}", path);
    Ok(())
}

/// Remove only Accompany hooks, preserving user's other hooks.
pub fn uninstall_global() -> Result<(), String> {
    let path = global_settings_path();
    if !path.exists() {
        return Ok(());
    }

    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read settings: {}", e))?;
    let mut settings: Value = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse settings: {}", e))?;

    if let Some(hooks) = settings.get_mut("hooks").and_then(|h| h.as_object_mut()) {
        for (_, arr) in hooks.iter_mut() {
            if let Some(arr) = arr.as_array_mut() {
                arr.retain(|item| {
                    let s = serde_json::to_string(item).unwrap_or_default();
                    !s.contains(ACCOMPANY_MARKER)
                });
            }
        }

        // Clean up empty event arrays
        let empty_keys: Vec<String> = hooks
            .iter()
            .filter(|(_, v)| v.as_array().map_or(false, |a| a.is_empty()))
            .map(|(k, _)| k.clone())
            .collect();
        for key in empty_keys {
            hooks.remove(&key);
        }
    }

    let tmp_path = path.with_extension("json.tmp");
    let content = serde_json::to_string_pretty(&settings)
        .map_err(|e| format!("Failed to serialize: {}", e))?;
    std::fs::write(&tmp_path, &content)
        .map_err(|e| format!("Failed to write temp: {}", e))?;
    std::fs::rename(&tmp_path, &path)
        .map_err(|e| format!("Failed to rename: {}", e))?;

    tracing::info!("Accompany hooks removed from global settings");
    Ok(())
}
