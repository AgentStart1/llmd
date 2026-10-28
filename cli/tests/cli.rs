use reqwest::blocking::Client;
use serde_json::{json, Value};
use std::{fs, process::Stdio, thread, time::Instant};

mod supervision;
use supervision::{supervisor_command, with_cli, with_server, OwnedProcess, TIMEOUT};

fn client() -> Client {
    Client::builder()
        .no_proxy()
        .connect_timeout(std::time::Duration::from_secs(2))
        .timeout(TIMEOUT)
        .build()
        .unwrap()
}

#[test]
fn models_prints_nothing_for_an_empty_inventory() {
    with_cli(&["models"], |process| {
        assert_eq!(process.wait().unwrap(), 0, "{}", process.stderr());
        assert_eq!(process.stdout(), "");
        assert_eq!(process.stderr(), "");
    })
    .unwrap();
}

#[test]
fn chat_reports_a_missing_model_on_stderr() {
    with_cli(&["chat", "hello", "--model", "missing"], |process| {
        assert_eq!(process.wait().unwrap(), 1);
        assert_eq!(process.stdout(), "");
        assert!(
            process.stderr().contains("model not found: missing"),
            "{}",
            process.stderr()
        );
    })
    .unwrap();
}

#[test]
fn chat_requires_the_model_argument() {
    with_cli(&["chat", "hello"], |process| {
        assert_eq!(process.wait().unwrap(), 2);
        assert_eq!(process.stdout(), "");
        assert!(
            process.stderr().contains("--model <MODEL>"),
            "{}",
            process.stderr()
        );
    })
    .unwrap();
}

#[test]
fn serve_exposes_a_healthy_http_endpoint() {
    with_server(|_, base| {
        let response = client().get(format!("{base}/health")).send().unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.json::<Value>().unwrap(), json!({"status": "ok"}));
    })
    .unwrap();
}

#[test]
fn serve_exposes_an_empty_http_model_list() {
    with_server(|_, base| {
        let response = client().get(format!("{base}/v1/models")).send().unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(
            response.json::<Value>().unwrap(),
            json!({"object": "list", "data": []})
        );
    })
    .unwrap();
}

#[test]
fn serve_reports_the_unknown_model_id_over_http() {
    with_server(|_, base| {
        let response = client()
            .get(format!("{base}/v1/models/missing"))
            .send()
            .unwrap();
        assert_eq!(response.status(), 404);
        let body: Value = response.json().unwrap();
        assert_eq!(body["error"]["message"], "model not found: missing");
    })
    .unwrap();
}

#[test]
fn abnormal_supervisor_exit_cannot_pass_the_test_scope() {
    let mut directory = None;
    let error = with_cli(&["models"], |session| {
        assert_eq!(session.wait().unwrap(), 0);
        directory = Some(session.directory().to_owned());
        session.supervisor().0.kill().unwrap();
        session.supervisor().wait().unwrap();
    })
    .unwrap_err();
    // Fault injection intentionally killed the sole resource owner after the CLI exited.
    // Recover this fixture's files explicitly, then check that teardown reported failure.
    let directory = directory.unwrap();
    assert!(directory.exists());
    fs::remove_dir_all(directory).unwrap();
    assert!(error.to_string().contains("supervisor"), "{error:#}");
}

#[cfg(windows)]
#[test]
fn directory_removal_failure_is_reported_by_the_test_scope() {
    use std::os::windows::fs::OpenOptionsExt;
    let mut locked_file = None;
    let mut directory = None;
    let error = with_cli(&["models"], |session| {
        assert_eq!(session.wait().unwrap(), 0);
        directory = Some(session.directory().to_owned());
        locked_file = Some(
            fs::OpenOptions::new()
                .read(true)
                .share_mode(0)
                .open(session.directory().join("stdout"))
                .unwrap(),
        );
    })
    .unwrap_err();
    drop(locked_file);
    fs::remove_dir_all(directory.unwrap()).unwrap();
    assert!(
        error.to_string().contains("remove fixture directory"),
        "{error:#}"
    );
}

#[test]
#[ignore = "internal subprocess fixture, invoked with LLMD_TEST_CONTROL"]
fn supervise_cli() {
    if std::env::var_os("LLMD_TEST_CONTROL").is_some() {
        supervision::supervise_cli().unwrap();
    }
}

#[test]
#[ignore = "internal subprocess fixture, invoked with LLMD_TEST_READY"]
fn cancellation_worker() {
    if let Some(ready) = std::env::var_os("LLMD_TEST_READY") {
        with_server(|session, address| {
            client()
                .get(format!("{address}/health"))
                .send()
                .unwrap()
                .error_for_status()
                .unwrap();
            fs::write(
                ready,
                serde_json::to_vec(&json!({
                    "address": address, "directory": session.directory(),
                }))
                .unwrap(),
            )
            .unwrap();
            thread::sleep(TIMEOUT);
        })
        .unwrap();
    }
}

#[test]
fn terminating_the_runner_stops_its_server_and_removes_its_files() {
    let control = tempfile::tempdir().unwrap();
    let ready = control.path().join("ready");
    let mut worker = OwnedProcess(
        supervisor_command("cancellation_worker")
            .env("LLMD_TEST_READY", &ready)
            .stdin(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + TIMEOUT;
    let target = loop {
        if let Ok(bytes) = fs::read(&ready) {
            if let Ok(target) = serde_json::from_slice::<Value>(&bytes) {
                break target;
            }
        }
        assert!(
            worker.0.try_wait().unwrap().is_none() && Instant::now() < deadline,
            "cancellation worker did not become ready"
        );
        thread::sleep(std::time::Duration::from_millis(20));
    };
    worker.0.kill().unwrap();
    worker.wait().unwrap();
    let directory = std::path::Path::new(target["directory"].as_str().unwrap());
    let deadline = Instant::now() + TIMEOUT;
    while directory.exists() {
        assert!(
            Instant::now() < deadline,
            "supervisor did not remove the fixture directory"
        );
        thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(
        client()
            .get(format!("{}/health", target["address"].as_str().unwrap()))
            .send()
            .is_err(),
        "server survived runner termination"
    );
}
