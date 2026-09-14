use async_trait::async_trait;
use futures_core::Stream;
use serde::{Deserialize, Serialize};
use std::pin::Pin;

pub const DEFAULT_MODEL: &str = "gemma-4-E2B-it";
pub const DEFAULT_HOST: &str = "127.0.0.1";
pub const DEFAULT_PORT: u16 = 11435;

pub type TokenStream = Pin<Box<dyn Stream<Item = Result<String, LlmdError>> + Send>>;

#[derive(Debug, thiserror::Error)]
pub enum LlmdError {
    #[error("{0}")]
    Backend(String),
    #[error("model not found: {0}")]
    ModelNotFound(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub owned_by: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub stream: bool,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub response_format: Option<ResponseFormat>,
}

/// OpenAI-compatible structured-output options.
///
/// Providers pass JSON schemas to LiteRT-LM's native constrained decoder.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponseFormat {
    Text,
    JsonObject,
    JsonSchema {
        json_schema: JsonSchemaResponseFormat,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonSchemaResponseFormat {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub schema: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strict: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub model: String,
    pub content: String,
}

#[async_trait]
pub trait ModelProvider: Send + Sync + 'static {
    async fn list_models(&self) -> Result<Vec<ModelInfo>, LlmdError>;
    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, LlmdError>;
    async fn chat_stream(&self, request: ChatRequest) -> Result<TokenStream, LlmdError>;
}

pub fn messages_to_prompt(messages: &[ChatMessage]) -> String {
    messages
        .iter()
        .map(|message| format!("{}: {}", message.role, message.content))
        .collect::<Vec<_>>()
        .join("\n")
}
