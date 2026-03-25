use reqwest::Client;
use serde::Deserialize;
use std::process::Stdio;

const MINIMAX_TTS_URL: &str = "https://api.minimaxi.com/v1/t2a_v2";

/// TTS client with MiniMax API primary + macOS `say` fallback.
#[derive(Clone)]
pub struct TtsClient {
    http: Client,
    api_key: String,
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
        Self {
            http: Client::new(),
            api_key,
        }
    }

    /// Synthesize speech. Tries MiniMax API first, falls back to macOS `say`.
    pub async fn synthesize(&self, text: &str, voice_id: &str) -> Result<Vec<u8>, String> {
        // Try MiniMax API first
        match self.synthesize_minimax(text, voice_id).await {
            Ok(bytes) => return Ok(bytes),
            Err(e) => {
                tracing::warn!("MiniMax TTS failed ({}), using macOS say fallback", e);
            }
        }

        // Fallback: edge-tts (Microsoft neural voices, free)
        self.synthesize_edge_tts(text).await
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

        let tts_resp: TtsResponse = response
            .json()
            .await
            .map_err(|e| format!("Parse failed: {}", e))?;

        if let Some(base) = &tts_resp.base_resp {
            if let Some(code) = base.status_code {
                if code != 0 {
                    let msg = base.status_msg.as_deref().unwrap_or("unknown");
                    return Err(format!("code {}: {}", code, msg));
                }
            }
        }

        let hex_audio = tts_resp
            .data
            .and_then(|d| d.audio)
            .ok_or("Missing audio data")?;

        let bytes = hex::decode(&hex_audio)
            .map_err(|e| format!("Hex decode failed: {}", e))?;

        tracing::info!("MiniMax TTS: {} bytes", bytes.len());
        Ok(bytes)
    }

    /// Fallback: use edge-tts (Microsoft Edge neural voices, free, no API key).
    /// Requires: `pip3 install edge-tts`
    async fn synthesize_edge_tts(&self, text: &str) -> Result<Vec<u8>, String> {
        let tmp = format!("/tmp/accompany_tts_{}_{}.mp3", std::process::id(), ulid::Ulid::new());
        let text = text.to_string();
        let tmp2 = tmp.clone();

        let result = tokio::task::spawn_blocking(move || {
            let output = std::process::Command::new("python3")
                .args([
                    "-m", "edge_tts",
                    "--text", &text,
                    "--voice", "zh-CN-XiaoxiaoNeural",
                    "--write-media", &tmp2,
                ])
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .output()
                .map_err(|e| format!("edge-tts launch failed (is python3 installed?): {}", e))?;

            if !output.status.success() {
                let _ = std::fs::remove_file(&tmp2);
                let stderr = String::from_utf8_lossy(&output.stderr);
                if stderr.contains("No module named") {
                    return Err("edge-tts not installed. Run: pip3 install edge-tts".to_string());
                }
                return Err(format!("edge-tts failed: {}", stderr.chars().take(200).collect::<String>()));
            }

            let bytes = std::fs::read(&tmp2);
            let _ = std::fs::remove_file(&tmp2); // Always clean up
            let bytes = bytes.map_err(|e| format!("Failed to read audio: {}", e))?;

            Ok(bytes)
        })
        .await
        .map_err(|e| format!("Task join error: {}", e))??;

        tracing::info!("edge-tts fallback: {} bytes", result.len());
        Ok(result)
    }
}
