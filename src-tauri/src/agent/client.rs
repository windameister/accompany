use futures::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;

use super::models::{classify_tier, ModelTier};
use super::prompt::system_prompt;

const MINIMAX_CHAT_URL: &str = "https://api.minimaxi.com/v1/chat/completions";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    max_tokens: u32,
    stream: bool,
}

#[derive(Debug, Deserialize)]
struct StreamChunk {
    choices: Option<Vec<StreamChoice>>,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    delta: Option<StreamDelta>,
}

#[derive(Debug, Deserialize)]
struct StreamDelta {
    content: Option<String>,
}

/// The AI agent client with conversation history and tiered model selection.
/// Uses MiniMax API (OpenAI-compatible).
pub struct AgentClient {
    http: Client,
    api_key: String,
    history: Arc<Mutex<Vec<ChatMessage>>>,
    memory_context: Arc<std::sync::Mutex<Option<String>>>,
}

impl AgentClient {
    pub fn new(api_key: String) -> Self {
        Self {
            http: Client::new(),
            api_key,
            history: Arc::new(Mutex::new(Vec::new())),
            memory_context: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    /// Set memory context to be included in the next system prompt.
    pub async fn set_memory_context(&self, context: &str) {
        *self.memory_context.lock().unwrap() = Some(context.to_string());
    }

    /// Send a message and stream the response token by token.
    /// Calls `on_token` for each chunk, returns the full assembled response.
    pub async fn chat_stream<F>(
        &self,
        user_message: &str,
        on_token: F,
    ) -> Result<(String, ModelTier), String>
    where
        F: Fn(&str) + Send + 'static,
    {
        let tier = classify_tier(user_message);
        let config = tier.config();

        tracing::info!(
            "Streaming with model: {} (tier: {:?})",
            config.display_name,
            tier
        );

        let mut history = self.history.lock().await;
        history.push(ChatMessage {
            role: "user".into(),
            content: user_message.to_string(),
        });

        // Build system prompt with memory context
        let base_prompt = system_prompt();
        let memory_ctx = self.memory_context.lock().unwrap().take();
        let full_prompt = if let Some(ctx) = memory_ctx {
            format!("{}\n\n{}", base_prompt, ctx)
        } else {
            base_prompt
        };

        let mut messages = vec![ChatMessage {
            role: "system".into(),
            content: full_prompt,
        }];
        let start = history.len().saturating_sub(20);
        messages.extend_from_slice(&history[start..]);

        let request = ChatRequest {
            model: config.model_id.to_string(),
            messages,
            max_tokens: config.max_tokens,
            stream: true,
        };

        let response = self
            .http
            .post(MINIMAX_CHAT_URL)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&request)
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            // Remove from history on failure
            history.pop();
            return Err(format!("API error ({}): {}", status, body));
        }

        let mut stream = response.bytes_stream();
        let mut raw_content = String::new(); // Full raw output (including <think>)
        let mut visible_content = String::new(); // Content shown to user (no <think>)
        let mut buffer = String::new();
        let mut in_think = false; // Track if inside <think> block

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| format!("Stream error: {}", e))?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(line_end) = buffer.find('\n') {
                let line = buffer[..line_end].trim().to_string();
                buffer = buffer[line_end + 1..].to_string();

                if line.starts_with("data: ") {
                    let data = &line[6..];
                    if data == "[DONE]" {
                        break;
                    }
                    if let Ok(chunk) = serde_json::from_str::<StreamChunk>(data) {
                        if let Some(choices) = chunk.choices {
                            if let Some(choice) = choices.first() {
                                if let Some(delta) = &choice.delta {
                                    if let Some(content) = &delta.content {
                                        raw_content.push_str(content);

                                        // Filter out <think>...</think> blocks
                                        let filtered = filter_think_tokens(
                                            content,
                                            &mut in_think,
                                        );
                                        if !filtered.is_empty() {
                                            visible_content.push_str(&filtered);
                                            on_token(&filtered);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Store full raw content in history (model needs it for context)
        history.push(ChatMessage {
            role: "assistant".into(),
            content: raw_content,
        });

        // Return only visible content (for display + TTS)
        Ok((visible_content, tier))
    }

    /// Clear conversation history.
    pub async fn clear_history(&self) {
        self.history.lock().await.clear();
    }
}

/// Filter out `<think>...</think>` content from streaming tokens.
///
/// Since tokens arrive in small chunks, the `<think>` and `</think>` tags
/// may be split across multiple tokens. We track state with `in_think`.
fn filter_think_tokens(token: &str, in_think: &mut bool) -> String {
    let mut result = String::new();
    let mut remaining = token;

    while !remaining.is_empty() {
        if *in_think {
            // Look for </think> to exit thinking mode
            if let Some(end_pos) = remaining.find("</think>") {
                *in_think = false;
                remaining = &remaining[end_pos + 8..]; // skip past </think>
            } else {
                // Still inside think block, skip everything
                // But check if we have a partial "</think" at the end
                break;
            }
        } else {
            // Look for <think> to enter thinking mode
            if let Some(start_pos) = remaining.find("<think>") {
                // Emit everything before <think>
                result.push_str(&remaining[..start_pos]);
                *in_think = true;
                remaining = &remaining[start_pos + 7..]; // skip past <think>
            } else {
                // No think tag, emit everything
                result.push_str(remaining);
                break;
            }
        }
    }

    result
}
