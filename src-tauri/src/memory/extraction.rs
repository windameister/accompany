use reqwest::Client;
use serde::Deserialize;

const MINIMAX_CHAT_URL: &str = "https://api.minimaxi.com/v1/chat/completions";

/// Extracted memory item from a conversation.
#[derive(Debug, Deserialize)]
pub struct ExtractedMemory {
    pub memory_type: String,
    pub content: String,
    pub confidence: f64,
}

/// Use the LLM to extract memorable facts from a conversation.
/// Returns a list of memories to store.
pub async fn extract_memories(
    api_key: &str,
    user_message: &str,
    assistant_response: &str,
) -> Result<Vec<ExtractedMemory>, String> {
    let extraction_prompt = format!(
        r#"从以下对话中提取值得记住的信息。只提取关于用户的事实、偏好、习惯或工作相关信息。

用户说: "{}"
助手回复: "{}"

以 JSON 数组格式返回，每个元素包含:
- memory_type: "fact"(事实), "preference"(偏好), "habit"(习惯), "project"(工作项目)
- content: 简洁的记忆内容（一句话）
- confidence: 0.0-1.0 的置信度

如果没有值得记住的内容，返回空数组 []。
只返回 JSON，不要其他文字。"#,
        user_message.chars().take(500).collect::<String>(),
        assistant_response.chars().take(500).collect::<String>(),
    );

    let http = Client::new();
    let body = serde_json::json!({
        "model": "MiniMax-M2.7",
        "messages": [
            {"role": "system", "content": "你是一个记忆提取器。只输出 JSON 数组，不要其他内容。"},
            {"role": "user", "content": extraction_prompt}
        ],
        "max_tokens": 512,
        "temperature": 0.3
    });

    let response = http
        .post(MINIMAX_CHAT_URL)
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Extraction request failed: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Extraction API error: {}", response.status()));
    }

    let resp: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse extraction response: {}", e))?;

    let content = resp["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("[]");

    // Strip <think> blocks and find JSON array
    let clean = strip_think(content);
    let json_str = extract_json_array(&clean);

    match serde_json::from_str::<Vec<ExtractedMemory>>(json_str) {
        Ok(memories) => {
            tracing::info!("Extracted {} memories from conversation", memories.len());
            Ok(memories)
        }
        Err(e) => {
            tracing::warn!("Failed to parse extracted memories: {} from: {}", e, json_str);
            Ok(vec![])
        }
    }
}

fn strip_think(s: &str) -> String {
    let mut result = String::new();
    let mut remaining = s;
    while let Some(start) = remaining.find("<think>") {
        result.push_str(&remaining[..start]);
        if let Some(end) = remaining.find("</think>") {
            remaining = &remaining[end + 8..];
        } else {
            remaining = "";
        }
    }
    result.push_str(remaining);
    result
}

/// Find the first JSON array in a string.
fn extract_json_array(s: &str) -> &str {
    if let Some(start) = s.find('[') {
        if let Some(end) = s.rfind(']') {
            return &s[start..=end];
        }
    }
    "[]"
}
