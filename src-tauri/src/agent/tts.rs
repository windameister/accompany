use reqwest::Client;
use serde::Deserialize;

const MINIMAX_TTS_URL: &str = "https://api.minimaxi.com/v1/t2a_v2";

/// TTS client using MiniMax Speech 2.8 API.
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
    audio: Option<String>, // hex-encoded audio bytes
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

    /// Synthesize speech from text. Returns raw audio bytes (mp3).
    pub async fn synthesize(&self, text: &str, voice_id: &str) -> Result<Vec<u8>, String> {
        // Truncate to API limit (10000 chars)
        let text = if text.len() > 9000 {
            &text[..9000]
        } else {
            text
        };

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

        tracing::info!("TTS request: {} chars, voice={}", text.len(), voice_id);

        let response = self
            .http
            .post(MINIMAX_TTS_URL)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("TTS request failed: {}", e))?;

        let status = response.status();
        if !status.is_success() {
            let err_body = response.text().await.unwrap_or_default();
            return Err(format!("TTS API error ({}): {}", status, err_body));
        }

        let tts_resp: TtsResponse = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse TTS response: {}", e))?;

        // Check for API-level errors
        if let Some(base) = &tts_resp.base_resp {
            if let Some(code) = base.status_code {
                if code != 0 {
                    let msg = base.status_msg.as_deref().unwrap_or("unknown");
                    return Err(format!("TTS error (code {}): {}", code, msg));
                }
            }
        }

        // Decode hex string to bytes
        let hex_audio = tts_resp
            .data
            .and_then(|d| d.audio)
            .ok_or("TTS response missing audio data")?;

        let bytes = hex::decode(&hex_audio)
            .map_err(|e| format!("Failed to decode hex audio: {}", e))?;

        tracing::info!("TTS response: {} bytes audio", bytes.len());
        Ok(bytes)
    }
}
