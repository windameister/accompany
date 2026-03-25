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

        // Build messages snapshot without modifying history yet.
        // History is only updated on success (avoids race condition on failure rollback).
        let user_msg = ChatMessage {
            role: "user".into(),
            content: user_message.to_string(),
        };

        let messages = {
            let history = self.history.lock().await;

            let base_prompt = system_prompt();
            let memory_ctx = self.memory_context.lock().unwrap().take();
            let full_prompt = if let Some(ctx) = memory_ctx {
                format!("{}\n\n{}", base_prompt, ctx)
            } else {
                base_prompt
            };

            let mut msgs = vec![ChatMessage {
                role: "system".into(),
                content: full_prompt,
            }];
            let start = history.len().saturating_sub(20);
            msgs.extend_from_slice(&history[start..]);
            msgs.push(user_msg.clone()); // Include user msg in request but not in history yet
            msgs
        }; // history lock released here

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
            return Err(format!("API error ({}): {}", status, body));
        }

        let mut stream = response.bytes_stream();
        let mut raw_content = String::new(); // Full raw output (including <think>)
        let mut visible_content = String::new(); // Content shown to user (no <think>)
        let mut buffer = String::new();
        let mut think_filter = ThinkFilter::new();

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
                                        let filtered = think_filter.process(content);
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

        // Flush any pending content from think filter
        let remaining = think_filter.flush();
        if !remaining.is_empty() {
            visible_content.push_str(&remaining);
            on_token(&remaining);
        }

        // Success: commit both user message and assistant response to history
        {
            let mut history = self.history.lock().await;
            history.push(user_msg);
            history.push(ChatMessage {
                role: "assistant".into(),
                content: raw_content,
            });
        }

        // Return only visible content (for display + TTS)
        Ok((visible_content, tier))
    }

    /// Clear conversation history.
    pub async fn clear_history(&self) {
        self.history.lock().await.clear();
    }
}

/// Stateful filter for `<think>...</think>` blocks in streaming tokens.
/// Handles tags split across multiple tokens by buffering partial matches.
struct ThinkFilter {
    in_think: bool,
    /// Buffer for potential partial tag at the end of a token
    pending: String,
}

impl ThinkFilter {
    fn new() -> Self {
        Self {
            in_think: false,
            pending: String::new(),
        }
    }

    /// Process a new token. Returns the visible (non-think) content to emit.
    fn process(&mut self, token: &str) -> String {
        // Prepend any pending partial from last token
        let input = if self.pending.is_empty() {
            token.to_string()
        } else {
            let mut s = std::mem::take(&mut self.pending);
            s.push_str(token);
            s
        };

        let mut result = String::new();
        let mut pos = 0;
        let bytes = input.as_bytes();

        while pos < input.len() {
            if self.in_think {
                if let Some(end) = input[pos..].find("</think>") {
                    self.in_think = false;
                    pos += end + 8;
                } else {
                    // Check for partial </think> at end (only ASCII chars, safe to slice)
                    let tail = &input[pos..];
                    if let Some(partial) = find_tag_suffix(tail, "</think>") {
                        self.pending = partial.to_string();
                    }
                    break;
                }
            } else {
                if let Some(start) = input[pos..].find("<think>") {
                    result.push_str(&input[pos..pos + start]);
                    self.in_think = true;
                    pos += start + 7;
                } else {
                    let tail = &input[pos..];
                    if let Some(partial) = find_tag_suffix(tail, "<think>") {
                        let safe_end = tail.len() - partial.len();
                        result.push_str(&tail[..safe_end]);
                        self.pending = partial.to_string();
                    } else {
                        result.push_str(tail);
                    }
                    break;
                }
            }
        }

        result
    }

    /// Flush any remaining pending content (call at end of stream).
    fn flush(&mut self) -> String {
        if self.in_think {
            String::new()
        } else {
            std::mem::take(&mut self.pending)
        }
    }
}

/// Find if any suffix of `text` is a prefix of `tag`.
/// Returns the matching suffix, or None. Only searches from `<` positions
/// to avoid splitting multi-byte UTF-8 characters.
fn find_tag_suffix<'a>(text: &'a str, tag: &str) -> Option<&'a str> {
    // Tags like <think> and </think> start with '<', which is ASCII.
    // So we only need to check suffixes starting with '<'.
    for (i, ch) in text.char_indices().rev() {
        if ch == '<' {
            let suffix = &text[i..];
            if tag.starts_with(suffix) {
                return Some(suffix);
            }
        }
    }
    None
}
