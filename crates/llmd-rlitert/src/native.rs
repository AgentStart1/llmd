use super::backend;
use litertlm_rs::*;
use llmd_core::{ChatRequest, LlmdError, ResponseFormat};
use serde_json::json;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tokio::sync::mpsc::Sender;

#[derive(Default)]
pub struct EngineCache {
    engines: Mutex<HashMap<PathBuf, Arc<Mutex<Engine>>>>,
}

impl EngineCache {
    fn get(&self, path: &Path) -> Result<Arc<Mutex<Engine>>, LlmdError> {
        let mut engines = self
            .engines
            .lock()
            .map_err(|_| backend("Engine cache lock was poisoned"))?;
        if let Some(engine) = engines.get(path) {
            return Ok(engine.clone());
        }
        let settings = EngineSettings::new(
            path.to_str().ok_or_else(|| backend("Invalid model path"))?,
            "cpu",
            None,
            None,
        )
        .map_err(backend)?;
        let engine = Arc::new(Mutex::new(Engine::new(&settings).map_err(backend)?));
        engines.insert(path.to_owned(), engine.clone());
        Ok(engine)
    }

    pub fn invalidate(&self, path: &Path) -> Result<(), LlmdError> {
        self.engines
            .lock()
            .map_err(|_| backend("Engine cache lock was poisoned"))?
            .remove(path);
        Ok(())
    }
}

pub fn generate(
    cache: &EngineCache,
    path: &Path,
    request: &ChatRequest,
    tx: &Sender<Result<String, LlmdError>>,
) -> Result<(), LlmdError> {
    let engine = cache.get(path)?;
    let engine = engine
        .lock()
        .map_err(|_| backend("Model engine lock was poisoned"))?;
    let mut config = ConversationConfig::new().map_err(backend)?;
    let mut session = SessionConfig::new().map_err(backend)?;
    let mut sampler = SamplerParams::new(SamplerType::TopP).map_err(backend)?;
    sampler.set_top_k(40).set_top_p(0.95);
    sampler.set_temperature(request.temperature.unwrap_or(0.7));
    session.set_sampler_params(&sampler);
    config.set_session_config(&session);
    let mut args = ConversationOptionalArgs::new().map_err(backend)?;
    if let Some(max) = request.max_tokens {
        args.set_max_output_tokens(i32::try_from(max).map_err(backend)?);
    }
    let schema = match request.response_format.as_ref() {
        Some(ResponseFormat::JsonObject) => Some(json!({"type":"object"})),
        Some(ResponseFormat::JsonSchema { json_schema }) => Some(json_schema.schema.clone()),
        _ => None,
    };
    let mut thinking = ThinkingConfig::new().map_err(backend)?;
    if let Some(schema) = schema {
        config.set_constraint_provider(Some(ConstraintProviderType::LlGuidance));
        args.set_constraint(ConstraintType::JsonSchema, &schema.to_string())
            .map_err(backend)?;
        thinking.set_enable_thinking(false);
        args.set_thinking_config(&thinking);
    }
    let system = request
        .messages
        .iter()
        .filter(|m| m.role == "system")
        .map(|m| m.content.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");
    config
        .set_system_message(&json!({"role":"system","content":system}).to_string())
        .map_err(backend)?;
    let messages = request
        .messages
        .iter()
        .filter(|m| m.role != "system")
        .map(|m| json!({"role":native_role(&m.role),"content":m.content}))
        .collect::<Vec<_>>();
    let (last, history) = messages
        .split_last()
        .ok_or_else(|| backend("No message to send"))?;
    config
        .set_messages(&json!(history).to_string())
        .map_err(backend)?;
    let conversation = engine
        .create_conversation_with_config(&config)
        .map_err(backend)?;
    conversation
        .send_message_stream_with_args(&last.to_string(), None, Some(&args), |chunk| {
            if tx.is_closed() {
                conversation.cancel_process();
                return;
            }
            if let Some(error) = chunk.error() {
                let _ = tx.blocking_send(Err(backend(error)));
            }
            if let Some(message) = chunk.text() {
                let text = extract_text(&message);
                if !text.is_empty() && tx.blocking_send(Ok(text)).is_err() {
                    conversation.cancel_process();
                }
            }
        })
        .map_err(backend)
}

fn native_role(role: &str) -> &str {
    match role {
        "assistant" => "model",
        role => role,
    }
}

#[cfg(test)]
mod tests {
    use super::native_role;

    #[test]
    fn translates_openai_assistant_role() {
        assert_eq!(native_role("assistant"), "model");
        assert_eq!(native_role("user"), "user");
    }
}
