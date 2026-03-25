use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Manager,
};

mod agent;
mod claude_monitor;
mod commands;

fn setup_tray(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let show = MenuItem::with_id(app, "show", "显示猫娘", true, None::<&str>)?;
    let hide = MenuItem::with_id(app, "hide", "隐藏", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &hide, &quit])?;

    TrayIconBuilder::new()
        .menu(&menu)
        .tooltip("Accompany - 你的猫娘助手")
        .icon(app.default_window_icon().unwrap().clone())
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => {
                if let Some(window) = app.get_webview_window("character") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "hide" => {
                if let Some(window) = app.get_webview_window("character") {
                    let _ = window.hide();
                }
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .build(app)?;

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Load .env from project root (one level up from src-tauri)
    let env_path = std::env::current_dir()
        .ok()
        .and_then(|p| {
            // Try project root first, then current dir
            let project_root = p.parent().unwrap_or(&p).to_path_buf();
            let candidates = [project_root.join(".env"), p.join(".env")];
            candidates.into_iter().find(|path| path.exists())
        });

    if let Some(path) = env_path {
        if let Err(e) = dotenvy::from_path(&path) {
            eprintln!("Warning: failed to load .env from {:?}: {}", path, e);
        }
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "accompany=debug,info".into()),
        )
        .init();

    // Initialize AI agent + TTS (both use MiniMax API)
    let minimax_key = std::env::var("MINIMAX_API_KEY")
        .expect("MINIMAX_API_KEY must be set in .env or environment");

    let agent = agent::client::AgentClient::new(minimax_key.clone());
    let tts = agent::tts::TtsClient::new(minimax_key.clone());
    let tts_for_hooks = agent::tts::TtsClient::new(minimax_key);

    tracing::info!("AI agent + TTS initialized (MiniMax)");

    tauri::Builder::default()
        .manage(agent)
        .manage(tts)
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_shortcut("CmdOrCtrl+Shift+A")
                .expect("failed to register shortcut")
                .with_handler(|app, _shortcut, event| {
                    if event.state == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        if let Some(window) = app.get_webview_window("character") {
                            if window.is_visible().unwrap_or(false) {
                                let _ = window.hide();
                            } else {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                    }
                })
                .build(),
        )
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            commands::chat_send,
            commands::chat_clear,
            commands::tts_speak,
        ])
        .setup(|app| {
            if let Err(e) = setup_tray(app) {
                tracing::error!("Failed to setup tray: {}", e);
            }

            // Start Claude Code hook server (runs in background on port 17832)
            let tracker = claude_monitor::state::SessionTracker::new();
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(claude_monitor::hook_server::start_hook_server(
                app_handle,
                tracker,
                tts_for_hooks,
            ));

            tracing::info!("Accompany started successfully! Hook server on :17832, Cmd+Shift+A to toggle.");
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Accompany");
}
