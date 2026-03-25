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
                    "source": "chat",
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

/// Speech-to-text: receive base64 audio, return recognized text.
#[tauri::command]
pub async fn stt_recognize(audio_base64: String) -> Result<String, String> {
    let audio_bytes = base64::engine::general_purpose::STANDARD
        .decode(&audio_base64)
        .map_err(|e| format!("Invalid base64: {}", e))?;

    let tmp = format!(
        "/tmp/accompany_stt_{}_{}.wav",
        std::process::id(),
        ulid::Ulid::new()
    );
    let tmp2 = tmp.clone();

    let text = tokio::task::spawn_blocking(move || {
        // Save raw audio and convert to WAV
        let raw_path = format!("{}.audio", tmp2);
        let wav_path = format!("{}.wav", tmp2);
        std::fs::write(&raw_path, &audio_bytes)
            .map_err(|e| format!("Write failed: {}", e))?;

        let ffmpeg = std::process::Command::new("ffmpeg")
            .args(["-y", "-i", &raw_path, "-ar", "16000", "-ac", "1", &wav_path])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|e| format!("ffmpeg: {}", e))?;
        let _ = std::fs::remove_file(&raw_path);
        if !ffmpeg.success() {
            return Err("ffmpeg conversion failed".to_string());
        }

        // Try MLX Whisper first (local, offline)
        let mlx_result = std::process::Command::new("python3")
            .args(["-c", &format!(
                "import mlx_whisper; r = mlx_whisper.transcribe('{}', language='zh', path_or_hf_repo='mlx-community/whisper-large-v3-turbo'); print('OK:' + r.get('text','').strip())",
                wav_path
            )])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output();

        if let Ok(output) = mlx_result {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if output.status.success() && stdout.starts_with("OK:") {
                let text = stdout[3..].to_string();
                let _ = std::fs::remove_file(&wav_path);
                if !text.is_empty() {
                    tracing::info!("MLX STT: {}", text);
                    return Ok(text);
                }
            }
            tracing::warn!("MLX STT failed, trying Google STT");
        }

        // Fallback: Google STT via speech_recognition
        let output = std::process::Command::new("python3")
            .args(["-c", &format!(
                r#"
import speech_recognition as sr
r = sr.Recognizer()
with sr.AudioFile("{}") as source:
    audio = r.record(source)
try:
    print(r.recognize_google(audio, language="zh-CN"))
except sr.UnknownValueError:
    print("")
except sr.RequestError as e:
    print("ERROR:" + str(e))
"#, wav_path
            )])
            .output()
            .map_err(|e| format!("Google STT failed: {}", e))?;

        let _ = std::fs::remove_file(&wav_path);

        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if stdout.starts_with("ERROR:") {
            return Err(format!("STT error: {}", &stdout[6..]));
        }
        if stdout.is_empty() {
            return Err("未识别到语音".to_string());
        }

        tracing::info!("Google STT: {}", stdout);
        Ok(stdout)
    })
    .await
    .map_err(|e| format!("Task error: {}", e))??;

    Ok(text)
}

/// Classify if a speech segment is directed at the assistant.
/// Returns "direct", "self_talk", or "ignore".
#[tauri::command]
pub async fn classify_speech_intent(
    text: String,
    agent: State<'_, AgentClient>,
) -> Result<String, String> {
    // Quick heuristic for very short utterances
    if text.chars().count() < 3 {
        return Ok("ignore".to_string());
    }

    // Use a lightweight LLM call to classify intent
    let prompt = format!(
        r#"你是一个意图分类器。用户独自坐在电脑前，旁边有一个AI猫娘桌面助手在监听。判断这句话的意图。

语音内容: "{}"

重要背景：用户身边只有AI猫娘助手，没有其他人。所以大部分情况下用户说话都是在跟助手对话或者自言自语。

分类规则:
- "direct": 用户在说话、打招呼、提问、请求、聊天、或任何可以回应的内容（默认选这个）
- "self_talk": 用户明显在自言自语、碎碎念、叹气
- "ignore": 只有在内容确实是无意义的噪音、咳嗽、或明显不是对话时才选这个

如果不确定，选 direct。只回复一个词。"#,
        text.chars().take(200).collect::<String>()
    );

    let http = reqwest::Client::new();
    let api_key = std::env::var("MINIMAX_API_KEY").unwrap_or_default();

    let body = serde_json::json!({
        "model": "MiniMax-M2.7",
        "messages": [
            {"role": "user", "content": prompt}
        ],
        "max_tokens": 10,
        "temperature": 0.1
    });

    let resp = http
        .post("https://api.minimaxi.com/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Intent classify failed: {}", e))?;

    let json: serde_json::Value = resp.json().await
        .map_err(|e| format!("Parse failed: {}", e))?;

    let content = json["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("ignore")
        .trim()
        .to_lowercase();

    // Extract the classification word
    let intent = if content.contains("direct") {
        "direct"
    } else if content.contains("self_talk") {
        "self_talk"
    } else {
        "ignore"
    };

    tracing::info!("Speech intent: '{}' → {}", &text[..text.len().min(30)], intent);
    Ok(intent.to_string())
}

/// Voiceprint store path.
fn voiceprint_path() -> String {
    dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join("accompany")
        .join("voiceprint.json")
        .to_string_lossy()
        .to_string()
}

/// Path to voiceprint.py script (relative to project root).
fn voiceprint_script() -> String {
    // In dev: scripts/voiceprint.py relative to src-tauri parent
    let candidates = [
        std::env::current_dir()
            .ok()
            .map(|p| p.parent().unwrap_or(&p).join("scripts/voiceprint.py")),
        std::env::current_dir()
            .ok()
            .map(|p| p.join("scripts/voiceprint.py")),
    ];
    for c in candidates.into_iter().flatten() {
        if c.exists() {
            return c.to_string_lossy().to_string();
        }
    }
    "scripts/voiceprint.py".to_string()
}

/// Enroll a voice sample for host recognition.
/// audio_base64: base64 encoded audio from MediaRecorder.
#[tauri::command]
pub async fn voice_enroll(audio_base64: String) -> Result<serde_json::Value, String> {
    let audio_bytes = base64::engine::general_purpose::STANDARD
        .decode(&audio_base64)
        .map_err(|e| format!("Invalid base64: {}", e))?;

    let tmp_base = format!("/tmp/accompany_enroll_{}_{}", std::process::id(), ulid::Ulid::new());
    let script = voiceprint_script();
    let store = voiceprint_path();

    tokio::task::spawn_blocking(move || {
        // Save and convert to WAV
        let raw_path = format!("{}.audio", tmp_base);
        let wav_path = format!("{}.wav", tmp_base);
        std::fs::write(&raw_path, &audio_bytes)
            .map_err(|e| format!("Write failed: {}", e))?;

        let ffmpeg = std::process::Command::new("ffmpeg")
            .args(["-y", "-i", &raw_path, "-ar", "16000", "-ac", "1", &wav_path])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|e| format!("ffmpeg: {}", e))?;
        let _ = std::fs::remove_file(&raw_path);
        if !ffmpeg.success() {
            return Err("ffmpeg conversion failed".to_string());
        }

        // Call voiceprint.py enroll
        let output = std::process::Command::new("python3")
            .args([&script, "enroll", &wav_path, &store])
            .output()
            .map_err(|e| format!("voiceprint.py: {}", e))?;
        let _ = std::fs::remove_file(&wav_path);

        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Enrollment failed: {}", stderr));
        }

        let result: serde_json::Value = serde_json::from_str(&stdout)
            .map_err(|e| format!("Parse failed: {}", e))?;
        tracing::info!("Voice enrolled: {} samples", result["sample_count"]);
        Ok(result)
    })
    .await
    .map_err(|e| format!("Task error: {}", e))?
}

/// Verify if audio matches the host voiceprint.
/// Returns { is_host: bool, similarity: f64 }
#[tauri::command]
pub async fn voice_verify(audio_base64: String) -> Result<serde_json::Value, String> {
    let audio_bytes = base64::engine::general_purpose::STANDARD
        .decode(&audio_base64)
        .map_err(|e| format!("Invalid base64: {}", e))?;

    let tmp_base = format!("/tmp/accompany_verify_{}_{}", std::process::id(), ulid::Ulid::new());
    let script = voiceprint_script();
    let store = voiceprint_path();

    tokio::task::spawn_blocking(move || {
        let raw_path = format!("{}.audio", tmp_base);
        let wav_path = format!("{}.wav", tmp_base);
        std::fs::write(&raw_path, &audio_bytes)
            .map_err(|e| format!("Write failed: {}", e))?;

        let ffmpeg = std::process::Command::new("ffmpeg")
            .args(["-y", "-i", &raw_path, "-ar", "16000", "-ac", "1", &wav_path])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|e| format!("ffmpeg: {}", e))?;
        let _ = std::fs::remove_file(&raw_path);
        if !ffmpeg.success() {
            return Err("ffmpeg conversion failed".to_string());
        }

        let output = std::process::Command::new("python3")
            .args([&script, "verify", &wav_path, &store])
            .output()
            .map_err(|e| format!("voiceprint.py: {}", e))?;
        let _ = std::fs::remove_file(&wav_path);

        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Verify failed: {}", stderr));
        }

        let result: serde_json::Value = serde_json::from_str(&stdout)
            .map_err(|e| format!("Parse failed: {}", e))?;
        tracing::info!(
            "Voice verify: is_host={}, similarity={}",
            result["is_host"], result["similarity"]
        );
        Ok(result)
    })
    .await
    .map_err(|e| format!("Task error: {}", e))?
}

/// Check if host voiceprint is enrolled.
#[tauri::command]
pub async fn voice_is_enrolled() -> Result<bool, String> {
    Ok(std::path::Path::new(&voiceprint_path()).exists())
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
