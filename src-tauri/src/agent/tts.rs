use reqwest::Client;
use serde::Deserialize;
use std::process::Stdio;

const MINIMAX_TTS_URL: &str = "https://api.minimaxi.com/v1/t2a_v2";

/// TTS client: MLX local (primary) → MiniMax API → edge-tts (fallbacks).
#[derive(Clone)]
pub struct TtsClient {
    http: Client,
    api_key: String,
    bridge_script: String,
}

#[derive(Debug, Deserialize)]
struct TtsResponse {
    data: Option<TtsData>,
    base_resp: Option<BaseResp>,
}

#[derive(Debug, Deserialize)]
struct TtsData {
    audio: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BaseResp {
    status_code: Option<i32>,
    status_msg: Option<String>,
}

impl TtsClient {
    pub fn new(api_key: String) -> Self {
        // Find bridge script
        let bridge = find_script("scripts/mlx_audio_bridge.py");
        if !bridge.is_empty() {
            tracing::info!("MLX audio bridge found at: {}", bridge);
        }
        Self {
            http: Client::new(),
            api_key,
            bridge_script: bridge,
        }
    }

    /// Synthesize speech. Tries: MLX local → MiniMax API → edge-tts.
    pub async fn synthesize(&self, text: &str, voice_id: &str) -> Result<Vec<u8>, String> {
        // 1. Try local MLX Qwen3-TTS (fastest, no network)
        if !self.bridge_script.is_empty() {
            match self.synthesize_mlx(text).await {
                Ok(bytes) => return Ok(bytes),
                Err(e) => tracing::warn!("MLX TTS failed ({}), trying MiniMax", e),
            }
        }

        // 2. Try MiniMax API
        match self.synthesize_minimax(text, voice_id).await {
            Ok(bytes) => return Ok(bytes),
            Err(e) => tracing::warn!("MiniMax TTS failed ({}), trying edge-tts", e),
        }

        // 3. Fallback: edge-tts
        self.synthesize_edge_tts(text).await
    }

    /// Local TTS via MLX Server (persistent process on port 17833).
    /// Falls back to bridge script if server is not running.
    async fn synthesize_mlx(&self, text: &str) -> Result<Vec<u8>, String> {
        let tmp = format!("/tmp/accompany_mlx_tts_{}.wav", ulid::Ulid::new());

        // Try MLX Server first (model already loaded, fast)
        let server_result = self.http
            .post("http://127.0.0.1:17833/tts")
            .json(&serde_json::json!({"text": text, "output": &tmp}))
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await;

        if let Ok(resp) = server_result {
            if resp.status().is_success() {
                if let Ok(json) = resp.json::<serde_json::Value>().await {
                    if json["status"] == "ok" {
                        let bytes = std::fs::read(&tmp).map_err(|e| format!("Read: {}", e))?;
                        let _ = std::fs::remove_file(&tmp);
                        let elapsed = json["elapsed"].as_f64().unwrap_or(0.0);
                        tracing::info!("MLX Server TTS: {} bytes in {:.1}s", bytes.len(), elapsed);
                        return Ok(bytes);
                    }
                }
            }
        }

        // Fallback: bridge script (cold start each time)
        if self.bridge_script.is_empty() {
            return Err("MLX not available".to_string());
        }

        let script = self.bridge_script.clone();
        let text = text.to_string();
        let tmp2 = tmp.clone();

        let result = tokio::task::spawn_blocking(move || {
            let output = std::process::Command::new("python3")
                .args([&script, "tts", &text, &tmp2])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output()
                .map_err(|e| format!("Bridge failed: {}", e))?;

            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !output.status.success() || !stdout.starts_with("OK:") {
                let _ = std::fs::remove_file(&tmp2);
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(format!("MLX TTS error: {}", stderr.chars().take(200).collect::<String>()));
            }

            let bytes = std::fs::read(&tmp2);
            let _ = std::fs::remove_file(&tmp2);
            bytes.map_err(|e| format!("Read failed: {}", e))
        })
        .await
        .map_err(|e| format!("Task error: {}", e))??;

        tracing::info!("MLX Bridge TTS: {} bytes", result.len());
        Ok(result)
    }

    async fn synthesize_minimax(&self, text: &str, voice_id: &str) -> Result<Vec<u8>, String> {
        let text = if text.len() > 9000 { &text[..9000] } else { text };

        let body = serde_json::json!({
            "model": "speech-2.8-hd",
            "text": text,
            "stream": false,
            "voice_setting": {
                "voice_id": voice_id,
                "speed": 1.0,
                "vol": 5.0,
                "emotion": "happy"
            },
            "audio_setting": {
                "sample_rate": 24000,
                "format": "mp3"
            },
            "output_format": "hex"
        });

        let response = self
            .http
            .post(MINIMAX_TTS_URL)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;

        if !response.status().is_success() {
            let err = response.text().await.unwrap_or_default();
            return Err(format!("API error: {}", err));
        }

        let tts_resp: TtsResponse = response.json().await
            .map_err(|e| format!("Parse failed: {}", e))?;

        if let Some(base) = &tts_resp.base_resp {
            if let Some(code) = base.status_code {
                if code != 0 {
                    let msg = base.status_msg.as_deref().unwrap_or("unknown");
                    return Err(format!("code {}: {}", code, msg));
                }
            }
        }

        let hex_audio = tts_resp.data.and_then(|d| d.audio)
            .ok_or("Missing audio data")?;
        let bytes = hex::decode(&hex_audio)
            .map_err(|e| format!("Hex decode failed: {}", e))?;

        tracing::info!("MiniMax TTS: {} bytes", bytes.len());
        Ok(bytes)
    }

    async fn synthesize_edge_tts(&self, text: &str) -> Result<Vec<u8>, String> {
        let tmp = format!("/tmp/accompany_tts_{}.mp3", ulid::Ulid::new());
        let text = text.to_string();
        let tmp2 = tmp.clone();

        let result = tokio::task::spawn_blocking(move || {
            let output = std::process::Command::new("python3")
                .args(["-m", "edge_tts", "--text", &text, "--voice", "zh-CN-XiaoxiaoNeural", "--write-media", &tmp2])
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .output()
                .map_err(|e| format!("edge-tts failed: {}", e))?;

            if !output.status.success() {
                let _ = std::fs::remove_file(&tmp2);
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(format!("edge-tts error: {}", stderr.chars().take(200).collect::<String>()));
            }

            let bytes = std::fs::read(&tmp2);
            let _ = std::fs::remove_file(&tmp2);
            bytes.map_err(|e| format!("Read failed: {}", e))
        })
        .await
        .map_err(|e| format!("Task error: {}", e))??;

        tracing::info!("edge-tts: {} bytes", result.len());
        Ok(result)
    }
}

/// Find a script relative to the binary or src-tauri directory.
fn find_script(relative: &str) -> String {
    let candidates = [
        std::env::current_dir().ok().map(|p| p.parent().unwrap_or(&p).join(relative)),
        std::env::current_dir().ok().map(|p| p.join(relative)),
        std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join(relative))),
    ];
    for c in candidates.into_iter().flatten() {
        if c.exists() {
            return c.to_string_lossy().to_string();
        }
    }
    String::new()
}
