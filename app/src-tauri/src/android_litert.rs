use async_trait::async_trait;
use jni::{
    objects::{GlobalRef, JObject, JString, JValue},
    sys::{jboolean, jlong, JNI_FALSE, JNI_TRUE},
    JNIEnv, JavaVM,
};
use llmd_core::{
    ChatRequest, ChatResponse, LlmdError, ModelInfo, ModelProvider, TokenStream, DEFAULT_HOST,
    DEFAULT_MODEL, DEFAULT_PORT,
};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicI64, Ordering},
        Mutex, OnceLock,
    },
    thread::JoinHandle,
};
use tokio::sync::oneshot;

pub const PROVIDER_NAME: &str = super::ANDROID_PROVIDER_NAME;

static JAVA_VM: OnceLock<JavaVM> = OnceLock::new();
static BRIDGE_INSTANCE: OnceLock<GlobalRef> = OnceLock::new();
static SERVER_HANDLE: Mutex<Option<NativeServerHandle>> = Mutex::new(None);
static NEXT_CHAT_COMPLETION_ID: AtomicI64 = AtomicI64::new(1);
static PENDING_CHAT_COMPLETIONS: OnceLock<PendingChatCompletions> = OnceLock::new();

type ChatCompletionSender = oneshot::Sender<Result<String, LlmdError>>;
type PendingChatCompletions = Mutex<HashMap<i64, ChatCompletionSender>>;

struct NativeServerHandle {
    shutdown: oneshot::Sender<()>,
    thread: JoinHandle<()>,
}

pub struct AndroidLiteRtProvider;

#[async_trait]
impl ModelProvider for AndroidLiteRtProvider {
    async fn list_models(&self) -> Result<Vec<ModelInfo>, LlmdError> {
        call_bridge_list_models().map(|models| {
            models
                .into_iter()
                .map(|id| ModelInfo {
                    id,
                    owned_by: PROVIDER_NAME.to_string(),
                })
                .collect()
        })
    }

    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, LlmdError> {
        if request.model != DEFAULT_MODEL {
            return Err(LlmdError::ModelNotFound(request.model));
        }

        let response_model = request.model.clone();
        let request_json = serde_json::to_string(&request)
            .map_err(|error| LlmdError::Backend(error.to_string()))?;
        let content = call_bridge_chat_completion(request_json).await?;

        Ok(ChatResponse {
            model: response_model,
            content,
        })
    }

    async fn chat_stream(&self, request: ChatRequest) -> Result<TokenStream, LlmdError> {
        let response = self.chat(request).await?;
        Ok(Box::pin(futures_util::stream::once(async move {
            Ok(response.content)
        })))
    }
}

fn call_bridge_list_models() -> Result<Vec<String>, LlmdError> {
    let json = call_bridge_string("listModelsJson")?;
    serde_json::from_str(&json).map_err(|error| LlmdError::Backend(error.to_string()))
}

fn call_bridge_string(method: &str) -> Result<String, LlmdError> {
    let vm = JAVA_VM
        .get()
        .ok_or_else(|| LlmdError::Backend("Android JavaVM is not initialized".to_string()))?;
    let bridge = BRIDGE_INSTANCE.get().ok_or_else(|| {
        LlmdError::Backend("Android LiteRT bridge is not initialized".to_string())
    })?;

    let mut env = vm
        .attach_current_thread()
        .map_err(|error| LlmdError::Backend(error.to_string()))?;
    let value = env
        .call_method(bridge.as_obj(), method, "()Ljava/lang/String;", &[])
        .and_then(|value| value.l())
        .map_err(|error| LlmdError::Backend(error.to_string()))?;
    let value = JString::from(value);
    env.get_string(&value)
        .map(|value| value.into())
        .map_err(|error| LlmdError::Backend(error.to_string()))
}

async fn call_bridge_chat_completion(request_json: String) -> Result<String, LlmdError> {
    let request_id = NEXT_CHAT_COMPLETION_ID.fetch_add(1, Ordering::Relaxed);
    let (completion_tx, completion_rx) = oneshot::channel();
    pending_chat_completions()
        .lock()
        .map_err(|_| LlmdError::Backend("Android chat completion lock is poisoned".to_string()))?
        .insert(request_id, completion_tx);

    if let Err(error) = start_bridge_chat_completion(request_id, request_json) {
        let _ = pending_chat_completions()
            .lock()
            .map(|mut pending| pending.remove(&request_id));
        return Err(error);
    }

    completion_rx.await.map_err(|_| {
        LlmdError::Backend("Android chat completion callback was dropped".to_string())
    })?
}

fn pending_chat_completions() -> &'static PendingChatCompletions {
    PENDING_CHAT_COMPLETIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn start_bridge_chat_completion(request_id: i64, request_json: String) -> Result<(), LlmdError> {
    let vm = JAVA_VM
        .get()
        .ok_or_else(|| LlmdError::Backend("Android JavaVM is not initialized".to_string()))?;
    let bridge = BRIDGE_INSTANCE.get().ok_or_else(|| {
        LlmdError::Backend("Android LiteRT bridge is not initialized".to_string())
    })?;

    let mut env = vm
        .attach_current_thread()
        .map_err(|error| LlmdError::Backend(error.to_string()))?;
    let request_json = env
        .new_string(request_json)
        .map_err(|error| LlmdError::Backend(error.to_string()))?;
    env
        .call_method(
            bridge.as_obj(),
            "chatCompletionAsync",
            "(JLjava/lang/String;)V",
            &[JValue::Long(request_id), JValue::Object(&request_json)],
        )
        .map(|_| ())
        .map_err(|error| LlmdError::Backend(error.to_string()))
}

fn cache_android_handles(env: &mut JNIEnv<'_>) -> Result<(), String> {
    if JAVA_VM.get().is_none() {
        let vm = env.get_java_vm().map_err(|error| error.to_string())?;
        let _ = JAVA_VM.set(vm);
    }

    if BRIDGE_INSTANCE.get().is_none() {
        let class = env
            .find_class("com/storytellerf/llmd/LlmdAndroidBridge")
            .map_err(|error| error.to_string())?;
        let instance = env
            .get_static_field(
                class,
                "INSTANCE",
                "Lcom/storytellerf/llmd/LlmdAndroidBridge;",
            )
            .and_then(|value| value.l())
            .map_err(|error| error.to_string())?;
        let instance = env
            .new_global_ref(instance)
            .map_err(|error| error.to_string())?;
        let _ = BRIDGE_INSTANCE.set(instance);
    }

    Ok(())
}

fn start_server_inner(env: &mut JNIEnv<'_>) -> Result<(), String> {
    cache_android_handles(env)?;

    let mut handle = SERVER_HANDLE
        .lock()
        .map_err(|_| "Android server lock is poisoned".to_string())?;
    if handle.is_some() {
        return Ok(());
    }

    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let thread = std::thread::Builder::new()
        .name("llmd-android-api".to_string())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    eprintln!("failed to initialize Android API runtime: {error}");
                    return;
                }
            };

            runtime.block_on(async move {
                let provider = std::sync::Arc::new(AndroidLiteRtProvider);
                if let Err(error) =
                    llmd_server::serve_with_shutdown(provider, DEFAULT_HOST, DEFAULT_PORT, async {
                        let _ = shutdown_rx.await;
                    })
                    .await
                {
                    eprintln!("failed to start Android API server: {error}");
                }
            });
        })
        .map_err(|error| error.to_string())?;

    *handle = Some(NativeServerHandle {
        shutdown: shutdown_tx,
        thread,
    });
    Ok(())
}

fn stop_server_inner() -> Result<(), String> {
    let handle = SERVER_HANDLE
        .lock()
        .map_err(|_| "Android server lock is poisoned".to_string())?
        .take();

    if let Some(handle) = handle {
        let _ = handle.shutdown.send(());
        let _ = handle.thread.join();
    }

    Ok(())
}

#[no_mangle]
pub unsafe extern "system" fn Java_com_storytellerf_llmd_LlmdNativeServer_startServer(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
) -> jboolean {
    match start_server_inner(&mut env) {
        Ok(()) => JNI_TRUE,
        Err(error) => {
            eprintln!("failed to start native Android server: {error}");
            JNI_FALSE
        }
    }
}

#[no_mangle]
pub unsafe extern "system" fn Java_com_storytellerf_llmd_LlmdNativeServer_stopServer(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
) {
    if let Err(error) = stop_server_inner() {
        eprintln!("failed to stop native Android server: {error}");
    }
}

#[no_mangle]
pub unsafe extern "system" fn Java_com_storytellerf_llmd_LlmdNativeServer_completeChatCompletion(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    request_id: jlong,
    response: JObject<'_>,
    error: JObject<'_>,
) {
    let result = match read_nullable_string(&mut env, error) {
        Ok(Some(message)) => Err(LlmdError::Backend(message)),
        Ok(None) => match read_nullable_string(&mut env, response) {
            Ok(Some(response)) => Ok(response),
            Ok(None) => Err(LlmdError::Backend(
                "Android chat completion returned no response".to_string(),
            )),
            Err(error) => Err(error),
        },
        Err(error) => Err(error),
    };

    match pending_chat_completions()
        .lock()
        .map(|mut pending| pending.remove(&request_id))
    {
        Ok(Some(sender)) => {
            let _ = sender.send(result);
        }
        Ok(None) => {
            eprintln!("received unknown Android chat completion id: {request_id}");
        }
        Err(error) => {
            eprintln!("failed to complete Android chat completion: {error}");
        }
    }
}

fn read_nullable_string(
    env: &mut JNIEnv<'_>,
    value: JObject<'_>,
) -> Result<Option<String>, LlmdError> {
    if value.is_null() {
        return Ok(None);
    }

    let value = JString::from(value);
    env.get_string(&value)
        .map(|value| Some(value.into()))
        .map_err(|error| LlmdError::Backend(error.to_string()))
}
