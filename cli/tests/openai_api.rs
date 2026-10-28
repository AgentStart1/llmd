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
                .no_proxy()
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
