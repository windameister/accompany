use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Manager,
};

mod agent;
mod brain;
mod claude_monitor;
mod commands;
mod hooks_manager;
mod memory;
mod notifications;
mod soul;

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
            commands::classify_speech_intent,
            commands::voice_enroll,
            commands::voice_verify,
            commands::voice_is_enrolled,
            commands::is_onboarded,
            commands::complete_onboarding,
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

            // Create the brain (event queue + decision engine)
            let brain_queue = brain::queue::EventQueue::new();

            // Start brain engine
            let brain_app = app.handle().clone();
            let brain_tts = agent::tts::TtsClient::new(
                std::env::var("MINIMAX_API_KEY").unwrap_or_default(),
            );
            let brain_q = brain_queue.clone();
            tauri::async_runtime::spawn(brain::engine::run(brain_app, brain_q, brain_tts));

            // Start Claude Code hook server (pushes events to brain)
            let tracker = claude_monitor::state::SessionTracker::new();
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(claude_monitor::hook_server::start_hook_server(
                app_handle,
                tracker,
                brain_queue.clone(),
            ));

            // Start GitHub Actions monitor (pushes events to brain)
            let app_for_gh = app.handle().clone();
            tauri::async_runtime::spawn(
                notifications::github::start_github_monitor(app_for_gh, brain_queue.clone()),
            );

            // Start MLX Server (TTS/STT/Voiceprint) as a managed child process
            let mlx_child = start_mlx_server();
            app.manage(mlx_child);

            tracing::info!("Accompany started successfully! Hook server on :17832, GitHub monitor active, Cmd+Shift+A to toggle.");
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building Accompany")
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested { .. } = event {
                // Kill MLX Server on exit
                if let Some(child) = app.try_state::<MlxServerHandle>() {
                    child.kill();
                }
            }
        });
}

/// Handle to the MLX Server child process.
struct MlxServerHandle {
    pid: std::sync::Mutex<Option<u32>>,
}

impl MlxServerHandle {
    fn kill(&self) {
        if let Some(pid) = self.pid.lock().unwrap().take() {
            tracing::info!("Killing MLX Server (PID {})", pid);
            unsafe { libc::kill(pid as i32, libc::SIGTERM); }
        }
    }
}

impl Drop for MlxServerHandle {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Find and start the MLX Server script.
fn start_mlx_server() -> MlxServerHandle {
    let script = find_script("scripts/mlx_server.py");
    if script.is_empty() {
        tracing::warn!("MLX Server script not found, TTS/STT will use fallbacks");
        return MlxServerHandle { pid: std::sync::Mutex::new(None) };
    }

    // Check if already running
    if let Ok(resp) = std::process::Command::new("curl")
        .args(["--noproxy", "*", "-s", "-o", "/dev/null", "-w", "%{http_code}", "http://127.0.0.1:17833/health"])
        .output()
    {
        let code = String::from_utf8_lossy(&resp.stdout);
        if code.trim() == "200" {
            tracing::info!("MLX Server already running on :17833");
            return MlxServerHandle { pid: std::sync::Mutex::new(None) };
        }
    }

    tracing::info!("Starting MLX Server: {}", script);

    match std::process::Command::new("python3")
        .arg(&script)
        .stdout(std::process::Stdio::null())
        .stderr(std::fs::File::create("/tmp/mlx_server.log").unwrap_or_else(|_| {
            std::fs::File::create("/dev/null").unwrap()
        }))
        .spawn()
    {
        Ok(child) => {
            let pid = child.id();
            tracing::info!("MLX Server spawned (PID {}), warming up...", pid);
            MlxServerHandle { pid: std::sync::Mutex::new(Some(pid)) }
        }
        Err(e) => {
            tracing::error!("Failed to start MLX Server: {}", e);
            MlxServerHandle { pid: std::sync::Mutex::new(None) }
        }
    }
}

fn find_script(relative: &str) -> String {
    let candidates = [
        std::env::current_dir().ok().map(|p| p.parent().unwrap_or(&p).join(relative)),
        std::env::current_dir().ok().map(|p| p.join(relative)),
    ];
    for c in candidates.into_iter().flatten() {
        if c.exists() {
            return c.to_string_lossy().to_string();
        }
    }
    String::new()
}
