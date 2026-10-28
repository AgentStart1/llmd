use reqwest::blocking::Client;
use serde_json::{json, Value};
use std::{
    fs::{self, File},
    process::{Child, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};
use tempfile::TempDir;

const TIMEOUT: Duration = Duration::from_secs(30);

struct CliProcess {
    child: Child,
    directory: TempDir,
}

impl CliProcess {
    fn spawn(args: &[&str]) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let models = directory.path().join("models");
        fs::create_dir(&models).unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_llmd"))
            .args(args)
            .env("LLMD_MODEL_DIR", models)
            .env("RUST_LOG", "llmd_server=info")
            .env("NO_COLOR", "1")
            .stdin(Stdio::null())
            .stdout(File::create(directory.path().join("stdout")).unwrap())
            .stderr(File::create(directory.path().join("stderr")).unwrap())
            .spawn()
            .unwrap();
        Self { child, directory }
    }

    fn stdout(&self) -> String {
        fs::read_to_string(self.directory.path().join("stdout")).unwrap()
    }

    fn stderr(&self) -> String {
        fs::read_to_string(self.directory.path().join("stderr")).unwrap()
    }

    fn wait(&mut self) -> ExitStatus {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status;
            }
            assert!(
                Instant::now() < deadline,
                "CLI timed out: {} {}",
                self.stdout(),
                self.stderr()
            );
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn server() -> (Self, String) {
        let mut process = Self::spawn(&[
            "serve",
            "--host",
            "127.0.0.1",
            "--port",
            "0",
            "--pool-size",
            "1",
        ]);
        let deadline = Instant::now() + TIMEOUT;
        loop {
            assert!(
                process.child.try_wait().unwrap().is_none(),
                "server exited: {} {}",
                process.stdout(),
                process.stderr()
            );
            let output = process.stdout();
            if let Some(address) = output
                .split("llmd listening on ")
                .nth(1)
                .and_then(|tail| tail.split_whitespace().next())
            {
                return (process, address.to_owned());
            }
            assert!(
                Instant::now() < deadline,
                "server readiness timed out: {} {}",
                process.stdout(),
                process.stderr()
            );
            thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for CliProcess {
    fn drop(&mut self) {
        // Reap only our child before TempDir removes its model directory and logs.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn client() -> Client {
    Client::builder()
        .no_proxy()
        .connect_timeout(Duration::from_secs(2))
        .timeout(TIMEOUT)
        .build()
        .unwrap()
}

#[test]
fn models_prints_nothing_for_an_empty_inventory() {
    let mut process = CliProcess::spawn(&["models"]);
    assert!(process.wait().success(), "{}", process.stderr());
    assert_eq!(process.stdout(), "");
    assert_eq!(process.stderr(), "");
}

#[test]
fn chat_reports_a_missing_model_on_stderr() {
    let mut process = CliProcess::spawn(&["chat", "hello", "--model", "missing"]);
    assert_eq!(process.wait().code(), Some(1));
    assert_eq!(process.stdout(), "");
    assert!(
        process.stderr().contains("model not found: missing"),
        "{}",
        process.stderr()
    );
}

#[test]
fn chat_requires_the_model_argument() {
    let mut process = CliProcess::spawn(&["chat", "hello"]);
    assert_eq!(process.wait().code(), Some(2));
    assert_eq!(process.stdout(), "");
    assert!(
        process.stderr().contains("--model <MODEL>"),
        "{}",
        process.stderr()
    );
}

#[test]
fn serve_exposes_a_healthy_http_endpoint() {
    let (_server, base) = CliProcess::server();
    let response = client().get(format!("{base}/health")).send().unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.json::<Value>().unwrap(), json!({"status": "ok"}));
}

#[test]
fn serve_exposes_an_empty_http_model_list() {
    let (_server, base) = CliProcess::server();
    let response = client().get(format!("{base}/v1/models")).send().unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.json::<Value>().unwrap(),
        json!({"object": "list", "data": []})
    );
}
