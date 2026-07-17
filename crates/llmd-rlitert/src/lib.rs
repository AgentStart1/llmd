use async_trait::async_trait;
use futures_util::StreamExt;
use litert_lm::LitManager;
use llmd_core::{
    messages_to_prompt, ChatRequest, ChatResponse, LlmdError, ModelInfo, ModelProvider, TokenStream,
};

pub struct RlitertProvider {
    manager: LitManager,
}

impl RlitertProvider {
    pub async fn new() -> Result<Self, LlmdError> {
        let manager = LitManager::new()
            .await
            .map_err(|error| LlmdError::Backend(error.to_string()))?;
        Ok(Self { manager })
    }

    pub async fn with_pool_size(pool_size: usize) -> Result<Self, LlmdError> {
        let manager = LitManager::new_with_pool_size(pool_size)
            .await
            .map_err(|error| LlmdError::Backend(error.to_string()))?;
        Ok(Self { manager })
    }
}

#[async_trait]
impl ModelProvider for RlitertProvider {
    async fn list_models(&self) -> Result<Vec<ModelInfo>, LlmdError> {
        let output = self
            .manager
            .list_models(false)
            .await
            .map_err(|error| LlmdError::Backend(error.to_string()))?;

        Ok(parse_models(&output))
    }

    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, LlmdError> {
        let prompt = messages_to_prompt(&request.messages);
        let content = self
            .manager
            .run_completion(&request.model, &prompt)
            .await
            .map_err(|error| LlmdError::Backend(error.to_string()))?;

        Ok(ChatResponse {
            model: request.model,
            content,
        })
    }

    async fn chat_stream(&self, request: ChatRequest) -> Result<TokenStream, LlmdError> {
        let prompt = messages_to_prompt(&request.messages);
        let stream = self
            .manager
            .run_completion_stream(&request.model, &prompt)
            .await
            .map_err(|error| LlmdError::Backend(error.to_string()))?
            .map(|item| item.map_err(|error| LlmdError::Backend(error.to_string())));

        Ok(Box::pin(stream))
    }
}

fn parse_models(output: &str) -> Vec<ModelInfo> {
    output
        .lines()
        .map(str::trim)
        .filter(|line| {
            !line.is_empty()
                && !line.starts_with("Available")
                && !line.starts_with("Downloaded")
                && !line.starts_with("ALIAS")
        })
        .filter_map(|line| line.split_whitespace().next())
        .map(|id| ModelInfo {
            id: id.to_string(),
            owned_by: "litert-lm".to_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{parse_models, RlitertProvider};
    use llmd_core::{ChatMessage, ChatRequest, ModelProvider, DEFAULT_MODEL};

    #[test]
    fn parses_litert_model_table() {
        let output = "Downloaded models\nALIAS SIZE\ngemma-4-E2B-it 2.6GB\n";
        let models = parse_models(output);
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "gemma-4-E2B-it");
    }

    #[tokio::test]
    #[ignore = "requires LiteRT-LM runtime and a downloaded model"]
    async fn real_rlitert_chat_smoke_test() {
        let provider = RlitertProvider::new().await.unwrap();
        let response = provider
            .chat(ChatRequest {
                model: DEFAULT_MODEL.to_string(),
                messages: vec![ChatMessage {
                    role: "user".to_string(),
                    content: "Reply with one short sentence.".to_string(),
                }],
                stream: false,
                max_tokens: Some(32),
                temperature: Some(0.0),
            })
            .await
            .unwrap();

        assert_eq!(response.model, DEFAULT_MODEL);
        assert!(!response.content.trim().is_empty());
    }
}
