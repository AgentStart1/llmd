use reqwest::blocking::Client;
use serde_json::{json, Value};
use std::time::Duration;

struct Endpoint {
    client: Client,
    base: String,
    model: String,
}

impl Endpoint {
    fn configured() -> Self {
        let model = std::env::var("LLMD_TEST_MODEL")
            .expect("set LLMD_TEST_MODEL to an imported model ID before enabling real-model tests");
        assert!(
            !model.trim().is_empty(),
            "LLMD_TEST_MODEL must not be empty"
        );
        Self {
            client: Client::builder()
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(180))
                .build()
                .unwrap(),
            base: std::env::var("LLMD_OPENAI_BASE_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:11435".into())
                .trim_end_matches('/')
                .into(),
            model,
        }
    }

    fn chat(&self, mut payload: Value) -> Value {
        payload["model"] = json!(self.model);
        payload["stream"] = json!(false);
        let response = self
            .client
            .post(format!("{}/v1/chat/completions", self.base))
            .json(&payload)
            .send()
            .unwrap();
        assert_eq!(response.status(), 200);
        response.json().unwrap()
    }
}

#[test]
#[ignore = "requires a running endpoint and LLMD_TEST_MODEL; never downloads a model"]
fn real_model_endpoint_is_healthy() {
    let endpoint = Endpoint::configured();
    let response = endpoint
        .client
        .get(format!("{}/health", endpoint.base))
        .send()
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.json::<Value>().unwrap(), json!({"status": "ok"}));
}

#[test]
#[ignore = "requires a running endpoint and LLMD_TEST_MODEL; never downloads a model"]
fn real_model_is_present_in_inventory() {
    let endpoint = Endpoint::configured();
    let response = endpoint
        .client
        .get(format!("{}/v1/models", endpoint.base))
        .send()
        .unwrap();
    assert_eq!(response.status(), 200);
    let body: Value = response.json().unwrap();
    assert_eq!(body["object"], "list");
    assert!(body["data"]
        .as_array()
        .unwrap()
        .iter()
        .any(|model| model["id"] == endpoint.model));
}

#[test]
#[ignore = "requires a running endpoint and LLMD_TEST_MODEL; never downloads a model"]
fn real_model_generates_nonempty_text() {
    let endpoint = Endpoint::configured();
    let prompt = std::env::var("LLMD_TEST_PROMPT")
        .unwrap_or_else(|_| "Reply with one short sentence.".into());
    let body = endpoint.chat(json!({"messages": [{"role": "user", "content": prompt}]}));
    assert!(!body["choices"][0]["message"]["content"]
        .as_str()
        .unwrap()
        .trim()
        .is_empty());
}

#[test]
#[ignore = "requires a running endpoint and LLMD_TEST_MODEL; never downloads a model"]
fn real_model_generates_the_exact_requested_json_object() {
    let endpoint = Endpoint::configured();
    let body = endpoint.chat(json!({
        "messages": [{"role": "user", "content": "Set value to ok."}],
        "temperature": 0,
        "response_format": {"type": "json_schema", "json_schema": {
            "name": "answer", "strict": true,
            "schema": {"type": "object", "properties": {"value": {"type": "string", "enum": ["ok"]}},
                "required": ["value"], "additionalProperties": false}
        }}
    }));
    let content: Value =
        serde_json::from_str(body["choices"][0]["message"]["content"].as_str().unwrap()).unwrap();
    // Exact equality checks the object type, required key, string enum and absence of extra keys.
    assert_eq!(content, json!({"value": "ok"}));
}

#[tokio::test]
async fn external_endpoint_tests_honor_environment_proxies() {
    use std::process::Stdio;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let proxy = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_url = format!("http://{}", proxy.local_addr().unwrap());
    // A fresh process avoids changing environment variables underneath other tests
    // and exercises the same client construction as the opt-in endpoint checks.
    let child = tokio::process::Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "real_model_endpoint_is_healthy"])
        .env("LLMD_TEST_MODEL", "proxy-fixture")
        .env("LLMD_OPENAI_BASE_URL", "http://llmd-proxy-test.invalid")
        .env("HTTP_PROXY", &proxy_url)
        .env("http_proxy", &proxy_url)
        .env("HTTPS_PROXY", &proxy_url)
        .env("https_proxy", &proxy_url)
        .env("ALL_PROXY", &proxy_url)
        .env("all_proxy", &proxy_url)
        .env("NO_PROXY", "")
        .env("no_proxy", "")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(15), async {
        let (stream, _) = proxy.accept().await.unwrap();
        let mut stream = BufReader::new(stream);
        let mut request = String::new();
        stream.read_line(&mut request).await.unwrap();
        assert_eq!(request, "GET http://llmd-proxy-test.invalid/health HTTP/1.1\r\n");
        loop {
            let mut header = String::new();
            stream.read_line(&mut header).await.unwrap();
            if header == "\r\n" || header.is_empty() { break; }
        }
        stream.get_mut().write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 15\r\nConnection: close\r\n\r\n{\"status\":\"ok\"}").await.unwrap();
        let output = child.wait_with_output().await.unwrap();
        assert!(output.status.success(), "{} {}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    }).await.expect("endpoint test did not complete through the environment proxy");
}
