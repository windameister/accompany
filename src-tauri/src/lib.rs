use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Manager,
};

mod agent;
mod claude_monitor;
mod commands;
mod hooks_manager;
mod memory;
mod notifications;

fn setup_tray(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    use tauri::menu::PredefinedMenuItem;

    let show = MenuItem::with_id(app, "show", "显示猫娘", true, None::<&str>)?;
    let hide = MenuItem::with_id(app, "hide", "隐藏", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;

    // Hooks toggle
    let hooks_installed = hooks_manager::is_installed_global();
    let hooks_label = if hooks_installed {
        "✅ Claude Hooks (已安装 · 点击卸载)"
    } else {
        "⬜ Claude Hooks (未安装 · 点击安装)"
    };
    let hooks_toggle = MenuItem::with_id(app, "hooks_toggle", hooks_label, true, None::<&str>)?;

    let separator2 = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;

    let menu = Menu::with_items(app, &[&show, &hide, &separator, &hooks_toggle, &separator2, &quit])?;

    TrayIconBuilder::new()
        .menu(&menu)
        .tooltip("Accompany - 你的猫娘助手")
        .icon(app.default_window_icon().unwrap().clone())
        .on_menu_event(move |app, event| match event.id.as_ref() {
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
            "hooks_toggle" => {
                let currently_installed = hooks_manager::is_installed_global();
                if currently_installed {
                    match hooks_manager::uninstall_global() {
                        Ok(_) => {
                            tracing::info!("Hooks uninstalled from global settings");
                            let _ = hooks_toggle.set_text("⬜ Claude Hooks (未安装 · 点击安装)");
                        }
                        Err(e) => tracing::error!("Failed to uninstall hooks: {}", e),
                    }
                } else {
                    match hooks_manager::install_global() {
                        Ok(_) => {
                            tracing::info!("Hooks installed to global settings");
                            let _ = hooks_toggle.set_text("✅ Claude Hooks (已安装 · 点击卸载)");
                        }
                        Err(e) => tracing::error!("Failed to install hooks: {}", e),
                    }
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
    let tts_for_hooks = agent::tts::TtsClient::new(minimax_key.clone());

    // Initialize memory database
    let data_dir = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("accompany");
    let memory_db = memory::db::MemoryDb::open(&data_dir.join("memory.db"))
        .expect("Failed to initialize memory database");

    // Store API key for memory extraction
    let api_key_for_state = minimax_key;

    tracing::info!("AI agent + TTS + Memory initialized (MiniMax)");

    tauri::Builder::default()
        .manage(agent)
        .manage(tts)
        .manage(memory_db)
        .manage(commands::ApiKeyState(api_key_for_state))
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
        .invoke_handler(tauri::generate_handler![
            commands::chat_send,
            commands::chat_clear,
            commands::tts_speak,
            commands::stt_recognize,
            commands::memory_list,
            commands::memory_delete,
        ])
        .setup(|app| {
            if let Err(e) = setup_tray(app) {
                tracing::error!("Failed to setup tray: {}", e);
            }

            // macOS: accept first mouse click even when window is not active
            #[cfg(target_os = "macos")]
            if let Some(window) = app.get_webview_window("character") {
                use objc2_app_kit::NSWindow;
                use objc2::runtime::AnyObject;

                let ns_win_ptr = window.ns_window()
                    .expect("Failed to get NSWindow") as *mut AnyObject;
                let ns_window: &NSWindow = unsafe { &*(ns_win_ptr as *const NSWindow) };
                unsafe {
                    ns_window.setAcceptsMouseMovedEvents(true);
                    ns_window.setMovableByWindowBackground(true);
                }
                tracing::info!("macOS: configured window for first-mouse events");
            }

            // Start Claude Code hook server (runs in background on port 17832)
            let tracker = claude_monitor::state::SessionTracker::new();
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(claude_monitor::hook_server::start_hook_server(
                app_handle,
                tracker,
                tts_for_hooks,
            ));

            // Start GitHub Actions monitor
            let app_for_gh = app.handle().clone();
            let tts_for_gh = agent::tts::TtsClient::new(
                std::env::var("MINIMAX_API_KEY").unwrap_or_default(),
            );
            tauri::async_runtime::spawn(
                notifications::github::start_github_monitor(app_for_gh, tts_for_gh),
            );

            tracing::info!("Accompany started successfully! Hook server on :17832, GitHub monitor active, Cmd+Shift+A to toggle.");
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building Accompany")
        // Hooks stay installed after exit — they timeout gracefully (5s) when
        // the server isn't running. This is intentional: user explicitly installs
        // hooks via tray menu, and they persist until explicitly uninstalled.
        .run(|_app, _event| {});
}
