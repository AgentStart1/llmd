use async_trait::async_trait;
use futures_util::StreamExt;
use llmd_core::{ChatRequest, ChatResponse, LlmdError, ModelInfo, ModelProvider, TokenStream};
use std::{path::PathBuf, sync::Arc};
use tokio::sync::Semaphore;

#[cfg(not(target_os = "android"))]
mod native;

pub struct LiteRtProvider {
    slots: Arc<Semaphore>,
    #[cfg(not(target_os = "android"))]
    engines: Arc<native::EngineCache>,
}

fn backend(error: impl std::fmt::Display) -> LlmdError {
    LlmdError::Backend(error.to_string())
}

fn model_dir() -> Result<PathBuf, LlmdError> {
    if let Some(path) = std::env::var_os("LLMD_MODEL_DIR") {
        return Ok(PathBuf::from(path));
    }
    dirs::data_local_dir()
        .map(|path| path.join("llmd").join("models"))
        .ok_or_else(|| backend("Cannot determine model directory; set LLMD_MODEL_DIR"))
}

fn validate_model_id(model: &str) -> Result<(), LlmdError> {
    if model.is_empty()
        || !model
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        || model == "."
        || model == ".."
        || model.ends_with('.')
    {
        return Err(backend("Invalid model ID"));
    }
    Ok(())
}

fn model_path(model: &str) -> Result<PathBuf, LlmdError> {
    validate_model_id(model)?;
    Ok(model_dir()?.join(format!("{model}.litertlm")))
}

impl LiteRtProvider {
    pub async fn new() -> Result<Self, LlmdError> {
        Self::with_pool_size(2).await
    }

    pub async fn with_pool_size(pool_size: usize) -> Result<Self, LlmdError> {
        if pool_size == 0 {
            return Err(backend("pool_size must be positive"));
        }
        Ok(Self {
            slots: Arc::new(Semaphore::new(pool_size)),
            #[cfg(not(target_os = "android"))]
            engines: Arc::new(native::EngineCache::default()),
        })
    }

    /// Import a local .litertlm file, or download the built-in default model.
    pub async fn import_model(model: &str) -> Result<(), LlmdError> {
        let source = PathBuf::from(model);
        let local = source.is_file();
        let id = if local {
            if source.extension().and_then(|s| s.to_str()) != Some("litertlm") {
                return Err(backend("Expected a .litertlm file"));
            }
            source
                .file_stem()
                .and_then(|s| s.to_str())
                .ok_or_else(|| backend("Invalid filename"))?
        } else {
            model
        };
        let destination = model_path(id)?;
        if destination.is_file() {
            return Ok(());
        }
        tokio::fs::create_dir_all(model_dir()?)
            .await
            .map_err(backend)?;
        let temp = tempfile::NamedTempFile::new_in(model_dir()?).map_err(backend)?;
        if local {
            tokio::fs::copy(&source, temp.path())
                .await
                .map_err(backend)?;
        } else {
            if id != llmd_core::DEFAULT_MODEL {
                return Err(backend(
                    "Unknown download alias; import a local .litertlm file instead",
                ));
            }
            let url = format!(
                "https://huggingface.co/litert-community/{id}-litert-lm/resolve/main/{id}.litertlm"
            );
            let client = reqwest::Client::new();
            let mut request = client.get(url);
            if let Ok(token) = std::env::var("HF_TOKEN") {
                request = request.bearer_auth(token);
            }
            let mut chunks = request
                .send()
                .await
                .map_err(backend)?
                .error_for_status()
                .map_err(backend)?
                .bytes_stream();
            let mut file = tokio::fs::File::from_std(temp.reopen().map_err(backend)?);
            use tokio::io::AsyncWriteExt;
            while let Some(chunk) = chunks.next().await {
                file.write_all(&chunk.map_err(backend)?)
                    .await
                    .map_err(backend)?;
            }
            file.flush().await.map_err(backend)?;
        }
        if temp.as_file().metadata().map_err(backend)?.len() == 0 {
            return Err(backend("Model file is empty"));
        }
        temp.persist_noclobber(destination).map_err(backend)?;
        Ok(())
    }

    pub async fn delete_model(model: &str) -> Result<(), LlmdError> {
        tokio::fs::remove_file(model_path(model)?)
            .await
            .map_err(backend)
    }
}

#[async_trait]
impl ModelProvider for LiteRtProvider {
    async fn list_models(&self) -> Result<Vec<ModelInfo>, LlmdError> {
        let mut entries = match tokio::fs::read_dir(model_dir()?).await {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(backend(e)),
        };
        let mut models = vec![];
        while let Some(entry) = entries.next_entry().await.map_err(backend)? {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("litertlm")
                && entry.file_type().await.map_err(backend)?.is_file()
                && entry.metadata().await.map_err(backend)?.len() > 0
            {
                if let Some(id) = path.file_stem().and_then(|s| s.to_str()) {
                    if validate_model_id(id).is_ok() {
                        models.push(ModelInfo {
                            id: id.to_owned(),
                            owned_by: "litertlm-rs".into(),
                        });
                    }
                }
            }
        }
        models.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(models)
    }

    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, LlmdError> {
        let model = request.model.clone();
        let mut stream = self.chat_stream(request).await?;
        let mut content = String::new();
        while let Some(chunk) = stream.next().await {
            content.push_str(&chunk?);
        }
        Ok(ChatResponse { model, content })
    }

    async fn chat_stream(&self, request: ChatRequest) -> Result<TokenStream, LlmdError> {
        let path = model_path(&request.model)?;
        if !path.is_file() {
            return Err(LlmdError::ModelNotFound(request.model));
        }
        let permit = self.slots.clone().acquire_owned().await.map_err(backend)?;
        #[cfg(not(target_os = "android"))]
        let engines = self.engines.clone();
        let (tx, rx) = tokio::sync::mpsc::channel(32);
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            #[cfg(not(target_os = "android"))]
            let result = native::generate(&engines, &path, &request, &tx);
            #[cfg(target_os = "android")]
            let result: Result<(), LlmdError> = {
                let _ = (path, request);
                Err(backend("Use the Android native bridge"))
            };
            if let Err(error) = result {
                let _ = tx.blocking_send(Err(error));
            }
        });
        Ok(Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn model_ids_cannot_escape_store() {
        for id in [
            "",
            ".",
            "..",
            "../model",
            "C:\\model",
            "a/b",
            "x:stream",
            "model.",
        ] {
            assert!(validate_model_id(id).is_err(), "{id}");
        }
        assert!(validate_model_id(llmd_core::DEFAULT_MODEL).is_ok());
    }
}
