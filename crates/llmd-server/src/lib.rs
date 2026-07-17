use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::sse::{Event, Sse},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use futures_util::{stream as futures_stream, StreamExt};
use llmd_core::{ChatMessage, ChatRequest, LlmdError, ModelProvider};
use serde::{Deserialize, Serialize};
use std::{
    convert::Infallible,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};

#[derive(Clone)]
pub struct AppState {
    provider: Arc<dyn ModelProvider>,
}

impl AppState {
    pub fn new(provider: Arc<dyn ModelProvider>) -> Self {
        Self { provider }
    }
}

#[derive(Debug, Deserialize)]
pub struct OpenAiChatRequest {
    pub model: String,
    pub messages: Vec<OpenAiMessage>,
    #[serde(default)]
    pub stream: bool,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct OpenAiMessage {
    pub role: String,
    #[serde(deserialize_with = "deserialize_content")]
    pub content: String,
}

fn deserialize_content<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::String(text) => Ok(text),
        serde_json::Value::Array(parts) => Ok(parts
            .into_iter()
            .filter_map(|part| {
                part.get("text")
                    .and_then(serde_json::Value::as_str)
                    .map(ToOwned::to_owned)
            })
            .collect::<Vec<_>>()
            .join("\n")),
        _ => Err(serde::de::Error::custom(
            "content must be a string or text parts",
        )),
    }
}

impl From<OpenAiChatRequest> for ChatRequest {
    fn from(request: OpenAiChatRequest) -> Self {
        Self {
            model: request.model,
            messages: request
                .messages
                .into_iter()
                .map(|message| ChatMessage {
                    role: message.role,
                    content: message.content,
                })
                .collect(),
            stream: request.stream,
            max_tokens: request.max_tokens,
            temperature: request.temperature,
        }
    }
}

#[derive(Debug, Serialize)]
struct OpenAiChatResponse {
    id: String,
    object: &'static str,
    created: u64,
    model: String,
    choices: Vec<OpenAiChoice>,
    usage: OpenAiUsage,
}

#[derive(Debug, Serialize)]
struct OpenAiChoice {
    index: u32,
    message: OpenAiMessage,
    finish_reason: &'static str,
}

#[derive(Debug, Serialize)]
struct OpenAiUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

#[derive(Debug, Serialize)]
struct OpenAiModelList {
    object: &'static str,
    data: Vec<OpenAiModel>,
}

#[derive(Debug, Serialize)]
struct OpenAiModel {
    id: String,
    object: &'static str,
    created: u64,
    owned_by: String,
}

#[derive(Debug, Serialize)]
struct OpenAiChunk {
    id: String,
    object: &'static str,
    created: u64,
    model: String,
    choices: Vec<OpenAiChunkChoice>,
}

#[derive(Debug, Serialize)]
struct OpenAiChunkChoice {
    index: u32,
    delta: OpenAiDelta,
    finish_reason: Option<&'static str>,
}

#[derive(Debug, Serialize)]
struct OpenAiDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    role: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
}

pub fn create_router(provider: Arc<dyn ModelProvider>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION]);

    Router::new()
        .route("/health", get(health))
        .route("/v1/models", get(list_models))
        .route("/v1/models/:model", get(get_model))
        .route("/v1/chat/completions", post(chat_completions))
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .with_state(AppState::new(provider))
}

pub async fn serve(provider: Arc<dyn ModelProvider>, host: &str, port: u16) -> anyhow::Result<()> {
    serve_with_shutdown(provider, host, port, std::future::pending::<()>()).await
}

pub async fn serve_with_shutdown(
    provider: Arc<dyn ModelProvider>,
    host: &str,
    port: u16,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> anyhow::Result<()> {
    let app = create_router(provider);
    let listener = tokio::net::TcpListener::bind(format!("{host}:{port}")).await?;
    tracing::info!("llmd listening on http://{host}:{port}");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await?;
    Ok(())
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}

async fn list_models(State(state): State<AppState>) -> Response {
    match state.provider.list_models().await {
        Ok(models) => {
            let data = models
                .into_iter()
                .map(|model| OpenAiModel {
                    id: model.id,
                    object: "model",
                    created: 0,
                    owned_by: model.owned_by,
                })
                .collect();
            Json(OpenAiModelList {
                object: "list",
                data,
            })
            .into_response()
        }
        Err(error) => provider_error(error),
    }
}

async fn get_model(State(state): State<AppState>, Path(model): Path<String>) -> Response {
    match state.provider.list_models().await {
        Ok(models) => match models.into_iter().find(|item| item.id == model) {
            Some(item) => Json(OpenAiModel {
                id: item.id,
                object: "model",
                created: 0,
                owned_by: item.owned_by,
            })
            .into_response(),
            None => provider_error(LlmdError::ModelNotFound(model)),
        },
        Err(error) => provider_error(error),
    }
}

async fn chat_completions(
    State(state): State<AppState>,
    Json(request): Json<OpenAiChatRequest>,
) -> Response {
    let request = ChatRequest::from(request);
    if request.stream {
        return chat_stream(state, request).await;
    }

    match state.provider.chat(request).await {
        Ok(response) => {
            let body = OpenAiChatResponse {
                id: format!("chatcmpl-{}", uuid::Uuid::new_v4()),
                object: "chat.completion",
                created: now(),
                model: response.model,
                choices: vec![OpenAiChoice {
                    index: 0,
                    message: OpenAiMessage {
                        role: "assistant".to_string(),
                        content: response.content,
                    },
                    finish_reason: "stop",
                }],
                usage: OpenAiUsage {
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    total_tokens: 0,
                },
            };
            Json(body).into_response()
        }
        Err(error) => provider_error(error),
    }
}

async fn chat_stream(state: AppState, request: ChatRequest) -> Response {
    let model = request.model.clone();
    let id = format!("chatcmpl-{}", uuid::Uuid::new_v4());
    let created = now();

    let stream = match state.provider.chat_stream(request).await {
        Ok(stream) => stream,
        Err(error) => return provider_error(error),
    };

    let mut first = true;
    let terminal_id = id.clone();
    let terminal_model = model.clone();
    let sse = stream
        .map(move |token| {
            let event = match token {
                Ok(content) => {
                    let chunk = OpenAiChunk {
                        id: id.clone(),
                        object: "chat.completion.chunk",
                        created,
                        model: model.clone(),
                        choices: vec![OpenAiChunkChoice {
                            index: 0,
                            delta: OpenAiDelta {
                                role: first.then_some("assistant"),
                                content: Some(content),
                            },
                            finish_reason: None,
                        }],
                    };
                    first = false;
                    Event::default().data(serde_json::to_string(&chunk).unwrap_or_default())
                }
                Err(error) => Event::default().event("error").data(error.to_string()),
            };
            Ok::<_, Infallible>(event)
        })
        .chain(futures_stream::iter([
            Ok(Event::default().data(
                serde_json::to_string(&OpenAiChunk {
                    id: terminal_id,
                    object: "chat.completion.chunk",
                    created,
                    model: terminal_model,
                    choices: vec![OpenAiChunkChoice {
                        index: 0,
                        delta: OpenAiDelta {
                            role: None,
                            content: None,
                        },
                        finish_reason: Some("stop"),
                    }],
                })
                .unwrap_or_default(),
            )),
            Ok(Event::default().data("[DONE]")),
        ]));

    Sse::new(sse).into_response()
}

fn provider_error(error: LlmdError) -> Response {
    let status = match error {
        LlmdError::ModelNotFound(_) => StatusCode::NOT_FOUND,
        LlmdError::Backend(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    let body = serde_json::json!({
        "error": {
            "message": error.to_string(),
            "type": "llmd_error"
        }
    });
    (status, Json(body)).into_response()
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::create_router;
    use async_trait::async_trait;
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
    };
    use futures_util::stream;
    use llmd_core::{ChatRequest, ChatResponse, LlmdError, ModelInfo, ModelProvider, TokenStream};
    use std::sync::Arc;
    use tower::ServiceExt;

    struct FakeProvider;

    #[async_trait]
    impl ModelProvider for FakeProvider {
        async fn list_models(&self) -> Result<Vec<ModelInfo>, LlmdError> {
            Ok(vec![ModelInfo {
                id: "fake-model".to_string(),
                owned_by: "llmd-test".to_string(),
            }])
        }

        async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, LlmdError> {
            let prompt = request
                .messages
                .last()
                .map(|message| message.content.clone())
                .unwrap_or_default();
            Ok(ChatResponse {
                model: request.model,
                content: format!("echo: {prompt}"),
            })
        }

        async fn chat_stream(&self, _request: ChatRequest) -> Result<TokenStream, LlmdError> {
            Ok(Box::pin(stream::iter([
                Ok("hello".to_string()),
                Ok(" world".to_string()),
            ])))
        }
    }

    fn app() -> axum::Router {
        create_router(Arc::new(FakeProvider))
    }

    async fn body_text(response: axum::response::Response) -> String {
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[tokio::test]
    async fn health_returns_ok() {
        let response = app()
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value = serde_json::from_str(&body_text(response).await).unwrap();
        assert_eq!(body["status"], "ok");
    }

    #[tokio::test]
    async fn models_returns_openai_model_list() {
        let response = app()
            .oneshot(
                Request::builder()
                    .uri("/v1/models")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value = serde_json::from_str(&body_text(response).await).unwrap();
        assert_eq!(body["object"], "list");
        assert_eq!(body["data"][0]["id"], "fake-model");
        assert_eq!(body["data"][0]["owned_by"], "llmd-test");
    }

    #[tokio::test]
    async fn chat_completions_returns_openai_response() {
        let body = serde_json::json!({
            "model": "fake-model",
            "messages": [{"role": "user", "content": "ping"}]
        });

        let response = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/chat/completions")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value = serde_json::from_str(&body_text(response).await).unwrap();
        assert_eq!(body["object"], "chat.completion");
        assert_eq!(body["model"], "fake-model");
        assert_eq!(body["choices"][0]["message"]["role"], "assistant");
        assert_eq!(body["choices"][0]["message"]["content"], "echo: ping");
    }

    #[tokio::test]
    async fn chat_completions_accepts_text_parts() {
        let body = serde_json::json!({
            "model": "fake-model",
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "hello"},
                    {"type": "text", "text": "parts"}
                ]
            }]
        });

        let response = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/chat/completions")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value = serde_json::from_str(&body_text(response).await).unwrap();
        assert_eq!(
            body["choices"][0]["message"]["content"],
            "echo: hello\nparts"
        );
    }

    #[tokio::test]
    async fn chat_completions_streams_sse_chunks() {
        let body = serde_json::json!({
            "model": "fake-model",
            "stream": true,
            "messages": [{"role": "user", "content": "ping"}]
        });

        let response = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/chat/completions")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = body_text(response).await;
        assert!(body.contains("data:"));
        assert!(body.contains("\"object\":\"chat.completion.chunk\""));
        assert!(body.contains("\"role\":\"assistant\""));
        assert!(body.contains("hello"));
        assert!(body.contains(" world"));
        assert!(body.contains("\"finish_reason\":\"stop\""));
        assert!(body.contains("data: [DONE]"));
    }
}
