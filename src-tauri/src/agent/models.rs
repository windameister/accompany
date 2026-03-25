use serde::{Deserialize, Serialize};

/// Model tier — determines which model to use based on task complexity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelTier {
    /// Casual chat, greetings, simple Q&A — fast and cheap
    Light,
    /// General conversation, moderate reasoning, summaries
    Standard,
    /// Complex tasks, multi-step reasoning, tool use, code analysis
    Heavy,
}

/// A model configuration.
#[derive(Debug, Clone)]
pub struct ModelConfig {
    pub tier: ModelTier,
    pub model_id: &'static str,
    pub display_name: &'static str,
    pub max_tokens: u32,
}

/// MiniMax model lineup. M2.7 for all tiers, varying max_tokens.
pub const MODELS: &[ModelConfig] = &[
    ModelConfig {
        tier: ModelTier::Light,
        model_id: "MiniMax-M2.7",
        display_name: "MiniMax M2.7",
        max_tokens: 1024,
    },
    ModelConfig {
        tier: ModelTier::Standard,
        model_id: "MiniMax-M2.7",
        display_name: "MiniMax M2.7",
        max_tokens: 4096,
    },
    ModelConfig {
        tier: ModelTier::Heavy,
        model_id: "MiniMax-M2.7",
        display_name: "MiniMax M2.7",
        max_tokens: 8192,
    },
];

impl ModelTier {
    pub fn config(&self) -> &'static ModelConfig {
        MODELS
            .iter()
            .find(|m| m.tier == *self)
            .expect("every tier must have a model")
    }
}

/// Simple heuristic to classify a user message into a tier.
pub fn classify_tier(message: &str) -> ModelTier {
    let msg = message.to_lowercase();
    let len = message.len();

    let heavy_keywords = [
        "分析", "analyze", "debug", "refactor", "设计", "architect",
        "review", "explain the code", "帮我写", "implement",
        "代码", "code", "bug", "error", "报错", "优化",
    ];
    if len > 500 || heavy_keywords.iter().any(|k| msg.contains(k)) {
        return ModelTier::Heavy;
    }

    let light_keywords = [
        "你好", "hi", "hello", "hey", "喵", "早上好", "晚安",
        "good morning", "good night", "谢谢", "thanks", "ok",
        "嗯", "哈哈", "好的",
    ];
    if len < 30 || light_keywords.iter().any(|k| msg.contains(k)) {
        return ModelTier::Light;
    }

    ModelTier::Standard
}
