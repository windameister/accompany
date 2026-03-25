use base64::Engine;
use serde::Serialize;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, State};
use tokio::sync::mpsc;

use crate::agent::client::AgentClient;
use crate::agent::models::ModelTier;
use crate::agent::tts::TtsClient;

const DEFAULT_VOICE: &str = "Chinese (Mandarin)_Cute_Spirit";

#[derive(Debug, Clone, Serialize)]
pub struct ChatResponse {
    pub content: String,
    pub model_tier: ModelTier,
}

fn is_sentence_end(c: char) -> bool {
    matches!(c, '。' | '！' | '？' | '.' | '!' | '?' | '\n' | '~' | '～')
}

/// Send a chat message with sentence-level TTS pipelining.
///
/// As LLM streams tokens, complete sentences are immediately sent to TTS.
/// Audio chunks are emitted as "tts-audio" events for the frontend to play in order.
#[tauri::command]
pub async fn chat_send(
    app: tauri::AppHandle,
    message: String,
    agent: State<'_, AgentClient>,
    tts: State<'_, TtsClient>,
) -> Result<ChatResponse, String> {
    let _ = app.emit("character-mood", "thinking");

    // Shared sentence buffer + channel for TTS pipeline
    let sentence_buf = Arc::new(Mutex::new(String::new()));
    let (sentence_tx, sentence_rx) = mpsc::unbounded_channel::<String>();

    // Start TTS consumer immediately (it will wait for sentences)
    let tts_client = tts.inner().clone();
    let app_for_tts = app.clone();
    let tts_handle = tokio::spawn(tts_pipeline(tts_client, app_for_tts, sentence_rx));

    // Stream LLM tokens, splitting into sentences
    let buf = sentence_buf.clone();
    let tx = sentence_tx.clone();
    let app_for_tokens = app.clone();

    let (content, tier) = agent
        .chat_stream(&message, move |token| {
            let _ = app_for_tokens.emit("chat-token", token);

            let mut buf = buf.lock().unwrap();
            buf.push_str(token);

            // Find the last sentence boundary
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

    // Flush remaining buffer as final sentence
    {
        let buf = sentence_buf.lock().unwrap();
        let remaining = buf.trim().to_string();
        if !remaining.is_empty() {
            let _ = sentence_tx.send(remaining);
        }
    }

    // Drop sender to signal TTS pipeline to finish
    drop(sentence_tx);

    let _ = app.emit("character-mood", "talking");

    // Wait for all TTS to complete
    let _ = tts_handle.await;

    let _ = app.emit("tts-done", ());
    // Mood will go back to idle after frontend finishes playing
    let _ = app.emit("character-mood", "happy");

    Ok(ChatResponse {
        content,
        model_tier: tier,
    })
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
                // Emit with sequence number so frontend can play in order
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
