use base64::Engine;
use serde::Serialize;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, State};
use tokio::sync::mpsc;

use crate::agent::client::AgentClient;
use crate::agent::models::ModelTier;
use crate::agent::tts::TtsClient;
use crate::memory::db::{Memory, MemoryDb};
use crate::memory::extraction;

const DEFAULT_VOICE: &str = "Chinese (Mandarin)_Cute_Spirit";

/// Holds the API key for memory extraction calls.
pub struct ApiKeyState(pub String);

#[derive(Debug, Clone, Serialize)]
pub struct ChatResponse {
    pub content: String,
    pub model_tier: ModelTier,
}

fn is_sentence_end(c: char) -> bool {
    matches!(c, '。' | '！' | '？' | '.' | '!' | '?' | '\n' | '~' | '～')
}

/// Send a chat message with memory-augmented context + sentence-level TTS pipelining.
#[tauri::command]
pub async fn chat_send(
    app: tauri::AppHandle,
    message: String,
    agent: State<'_, AgentClient>,
    tts: State<'_, TtsClient>,
    memory_db: State<'_, MemoryDb>,
    api_key: State<'_, ApiKeyState>,
) -> Result<ChatResponse, String> {
    let _ = app.emit("character-mood", "thinking");

    // 1. Retrieve relevant memories and inject into context
    let memories = memory_db.search_memories(&message, 5).await.unwrap_or_default();
    if !memories.is_empty() {
        let memory_context = format_memories_for_prompt(&memories);
        agent.set_memory_context(&memory_context).await;
        tracing::info!("Injected {} memories into context", memories.len());
    }

    // 2. Stream LLM response with TTS pipelining
    let sentence_buf = Arc::new(Mutex::new(String::new()));
    let (sentence_tx, sentence_rx) = mpsc::unbounded_channel::<String>();

    let tts_client = tts.inner().clone();
    let app_for_tts = app.clone();
    let tts_handle = tokio::spawn(tts_pipeline(tts_client, app_for_tts, sentence_rx));

    let buf = sentence_buf.clone();
    let tx = sentence_tx.clone();
    let app_for_tokens = app.clone();

    let (content, tier) = agent
        .chat_stream(&message, move |token| {
            let _ = app_for_tokens.emit("chat-token", token);

            let mut buf = buf.lock().unwrap();
            buf.push_str(token);

            if let Some(pos) = buf
                .char_indices()
                .rev()
                .find(|(_, c)| is_sentence_end(*c))
                .map(|(i, c)| i + c.len_utf8())
            {
                let sentence = buf[..pos].trim().to_string();
                if !sentence.is_empty() {
                    let _ = tx.send(sentence);
                }
                *buf = buf[pos..].to_string();
            }
        })
        .await?;

    // Flush remaining
    {
        let buf = sentence_buf.lock().unwrap();
        let remaining = buf.trim().to_string();
        if !remaining.is_empty() {
            let _ = sentence_tx.send(remaining);
        }
    }
    drop(sentence_tx);

    let _ = app.emit("character-mood", "talking");
    let _ = tts_handle.await;
    let _ = app.emit("tts-done", ());
    let _ = app.emit("character-mood", "happy");

    // 3. Extract memories in background (don't block response)
    let db = memory_db.inner().clone();
    let key = api_key.0.clone();
    let msg = message.clone();
    let resp = content.clone();
    tokio::spawn(async move {
        match extraction::extract_memories(&key, &msg, &resp).await {
            Ok(extracted) => {
                for mem in extracted {
                    if mem.confidence >= 0.5 {
                        let _ = db
                            .add_memory(&mem.memory_type, &mem.content, "conversation", mem.confidence)
                            .await;
                    }
                }
            }
            Err(e) => tracing::warn!("Memory extraction failed: {}", e),
        }
    });

    Ok(ChatResponse {
        content,
        model_tier: tier,
    })
}

fn format_memories_for_prompt(memories: &[Memory]) -> String {
    let mut s = String::from("以下是你记住的关于主人的信息：\n");
    for m in memories {
        s.push_str(&format!("- [{}] {}\n", m.memory_type, m.content));
    }
    s
}

/// TTS pipeline: consumes sentences and emits audio chunks.
async fn tts_pipeline(
    tts: TtsClient,
    app: tauri::AppHandle,
    mut rx: mpsc::UnboundedReceiver<String>,
) {
    let mut seq: u32 = 0;
    while let Some(sentence) = rx.recv().await {
        if sentence.is_empty() {
            continue;
        }
        let preview: String = sentence.chars().take(30).collect();
        tracing::debug!("TTS sentence #{}: {}", seq, preview);
        match tts.synthesize(&sentence, DEFAULT_VOICE).await {
            Ok(bytes) => {
                let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                let _ = app.emit("tts-audio", serde_json::json!({
                    "seq": seq,
                    "audio": b64,
                }));
                seq += 1;
            }
            Err(e) => {
                tracing::warn!("TTS chunk #{} failed: {}", seq, e);
                seq += 1;
            }
        }
    }
}

/// Clear conversation history.
#[tauri::command]
pub async fn chat_clear(agent: State<'_, AgentClient>) -> Result<(), String> {
    agent.clear_history().await;
    Ok(())
}

/// TTS-only: convert text to speech audio.
#[tauri::command]
pub async fn tts_speak(
    text: String,
    voice: Option<String>,
    tts: State<'_, TtsClient>,
) -> Result<String, String> {
    let voice = voice.unwrap_or_else(|| DEFAULT_VOICE.to_string());
    let bytes = tts.synthesize(&text, &voice).await?;
    Ok(base64::engine::general_purpose::STANDARD.encode(&bytes))
}

/// Get all stored memories.
#[tauri::command]
pub async fn memory_list(db: State<'_, MemoryDb>) -> Result<Vec<Memory>, String> {
    db.all_memories().await
}

/// Delete a memory.
#[tauri::command]
pub async fn memory_delete(id: String, db: State<'_, MemoryDb>) -> Result<(), String> {
    db.delete_memory(&id).await
}
