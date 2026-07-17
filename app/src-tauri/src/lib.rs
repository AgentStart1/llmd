#[cfg(target_os = "android")]
mod android_litert;

use llmd_core::{DEFAULT_HOST, DEFAULT_PORT};

#[cfg_attr(target_os = "android", allow(dead_code))]
const DESKTOP_PROVIDER_NAME: &str = "rlitert-lm";
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
const ANDROID_PROVIDER_NAME: &str = "litert-lm-android";
const DISABLED_PROVIDER_NAME: &str = "disabled";

#[tauri::command]
fn health() -> serde_json::Value {
    health_payload()
}

fn health_payload() -> serde_json::Value {
    serde_json::json!({
        "status": "ok",
        "desktop_provider": desktop_provider_name(),
        "android_provider": android_provider_name(),
        "api_base_url": format!("http://{}:{}", DEFAULT_HOST, DEFAULT_PORT)
    })
}

fn desktop_provider_name() -> &'static str {
    #[cfg(not(target_os = "android"))]
    {
        DESKTOP_PROVIDER_NAME
    }
    #[cfg(target_os = "android")]
    {
        DISABLED_PROVIDER_NAME
    }
}

fn android_provider_name() -> &'static str {
    #[cfg(target_os = "android")]
    {
        android_litert::PROVIDER_NAME
    }
    #[cfg(not(target_os = "android"))]
    {
        DISABLED_PROVIDER_NAME
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|_app| {
            start_platform_api_server();
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![health])
        .run(tauri::generate_context!())
        .expect("failed to run llmd app");
}

#[cfg(not(target_os = "android"))]
fn start_platform_api_server() {
    tauri::async_runtime::spawn(async {
        match llmd_rlitert::RlitertProvider::new().await {
            Ok(provider) => {
                if let Err(error) =
                    llmd_server::serve(std::sync::Arc::new(provider), DEFAULT_HOST, DEFAULT_PORT)
                        .await
                {
                    eprintln!("failed to start desktop API server: {error}");
                }
            }
            Err(error) => eprintln!("failed to initialize desktop provider: {error}"),
        }
    });
}

#[cfg(target_os = "android")]
fn start_platform_api_server() {
    // Android starts the API server from MainActivity so it can use litertlm-android.
}

#[cfg(test)]
mod tests {
    use super::{
        android_provider_name, desktop_provider_name, health_payload, ANDROID_PROVIDER_NAME,
        DESKTOP_PROVIDER_NAME, DISABLED_PROVIDER_NAME,
    };

    #[test]
    fn health_payload_reports_host_providers() {
        let payload = health_payload();

        assert_eq!(payload["status"], "ok");
        assert_eq!(payload["desktop_provider"], DESKTOP_PROVIDER_NAME);
        assert_eq!(payload["android_provider"], DISABLED_PROVIDER_NAME);
        assert_eq!(payload["api_base_url"], "http://127.0.0.1:11435");
    }

    #[test]
    fn provider_names_document_android_boundary() {
        assert_eq!(desktop_provider_name(), DESKTOP_PROVIDER_NAME);
        assert_eq!(android_provider_name(), DISABLED_PROVIDER_NAME);
        assert_eq!(ANDROID_PROVIDER_NAME, "litert-lm-android");
    }
}
