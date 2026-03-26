use std::path::PathBuf;

/// Directory for soul files (soul.md, host.md).
fn soul_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("accompany")
}

pub fn soul_path() -> PathBuf {
    soul_dir().join("soul.md")
}

pub fn host_path() -> PathBuf {
    soul_dir().join("host.md")
}

/// Check if onboarding has been completed (soul.md exists).
pub fn is_onboarded() -> bool {
    soul_path().exists() && host_path().exists()
}

/// Read soul.md content, or return default if not onboarded.
pub fn read_soul() -> String {
    std::fs::read_to_string(soul_path()).unwrap_or_else(|_| default_soul())
}

/// Read host.md content, or empty if not onboarded.
pub fn read_host() -> String {
    std::fs::read_to_string(host_path()).unwrap_or_default()
}

/// Save soul.md.
pub fn write_soul(content: &str) -> Result<(), String> {
    let dir = soul_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir: {}", e))?;
    std::fs::write(soul_path(), content).map_err(|e| format!("write soul: {}", e))?;
    tracing::info!("Soul saved: {} bytes", content.len());
    Ok(())
}

/// Save host.md.
pub fn write_host(content: &str) -> Result<(), String> {
    let dir = soul_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir: {}", e))?;
    std::fs::write(host_path(), content).map_err(|e| format!("write host: {}", e))?;
    tracing::info!("Host saved: {} bytes", content.len());
    Ok(())
}

/// Default soul used during onboarding (before soul.md is generated).
fn default_soul() -> String {
    r#"# Soul

你是一个桌面猫娘助手的雏形，正在进行首次设定。

## 核心
- 你温柔、真诚、有好奇心
- 你会关心主人的工作和生活
- 你说话自然，不会过度卖萌

## 风格
- 简洁为主，偶尔带"喵"
- 中文为主，技术术语可用英文
"#.to_string()
}

/// Build the full system prompt from soul + host + memory context.
pub fn build_system_prompt(memory_context: Option<&str>) -> String {
    let soul = read_soul();
    let host = read_host();

    let mut prompt = soul;

    if !host.is_empty() {
        prompt.push_str("\n\n# 关于主人\n");
        prompt.push_str(&host);
    }

    if let Some(mem) = memory_context {
        if !mem.is_empty() {
            prompt.push_str("\n\n# 记忆\n");
            prompt.push_str(mem);
        }
    }

    prompt
}
