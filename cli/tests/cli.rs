use reqwest::blocking::Client;
use serde_json::{json, Value};
use std::{
    fs::{self, File},
    process::{Child, Stdio},
    thread,
    time::{Duration, Instant},
};
use tempfile::TempDir;

mod supervision;
use supervision::supervisor_command;

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
        File::create(directory.path().join("stdout")).unwrap();
        File::create(directory.path().join("stderr")).unwrap();
        let child = supervisor_command("supervise_cli")
            .env("LLMD_TEST_DIRECTORY", directory.path())
            .env("LLMD_TEST_ARGS", serde_json::to_string(args).unwrap())
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        // The supervisor now owns removal, including when this process is killed.
        let mut directory = directory;
        directory.disable_cleanup(true);
        Self { child, directory }
    }

    fn stdout(&self) -> String {
        fs::read_to_string(self.directory.path().join("stdout")).unwrap()
    }

    fn stderr(&self) -> String {
        fs::read_to_string(self.directory.path().join("stderr")).unwrap()
    }

    fn wait(&mut self) -> i32 {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            if let Ok(status) = fs::read_to_string(self.directory.path().join("status")) {
                return status.parse().unwrap();
            }
            assert!(
                self.child.try_wait().unwrap().is_none(),
                "CLI supervisor exited early"
            );
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
                process.child.try_wait().unwrap().is_none()
                    && !process.directory.path().join("status").exists(),
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
        // Closing the lease also works when the OS terminates this test process.
        drop(self.child.stdin.take());
        let deadline = Instant::now() + TIMEOUT;
        while self.child.try_wait().ok().flatten().is_none() {
            if Instant::now() >= deadline {
                eprintln!("CLI supervisor cleanup timed out");
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
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
    assert_eq!(process.wait(), 0, "{}", process.stderr());
    assert_eq!(process.stdout(), "");
    assert_eq!(process.stderr(), "");
}

#[test]
fn chat_reports_a_missing_model_on_stderr() {
    let mut process = CliProcess::spawn(&["chat", "hello", "--model", "missing"]);
    assert_eq!(process.wait(), 1);
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
    assert_eq!(process.wait(), 2);
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

#[test]
#[ignore = "internal subprocess fixture, invoked with LLMD_TEST_DIRECTORY"]
fn supervise_cli() {
    if std::env::var_os("LLMD_TEST_DIRECTORY").is_some() {
        supervision::supervise_cli();
    }
}

#[test]
#[ignore = "internal subprocess fixture, invoked with LLMD_TEST_READY"]
fn cancellation_worker() {
    if let Some(ready) = std::env::var_os("LLMD_TEST_READY") {
        let (server, address) = CliProcess::server();
        // Make sure the endpoint really works before publishing the cancellation target.
        client()
            .get(format!("{address}/health"))
            .send()
            .unwrap()
            .error_for_status()
            .unwrap();
        fs::write(
            ready,
            serde_json::to_vec(&json!({
                "address": address,
                "directory": server.directory.path(),
            }))
            .unwrap(),
        )
        .unwrap();
        thread::sleep(TIMEOUT);
    }
}

#[test]
fn terminating_the_runner_stops_its_server_and_removes_its_files() {
    // Ensure a failed readiness assertion also closes the supervisor lease.
    struct Worker(Child);
    impl Drop for Worker {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let control = tempfile::tempdir().unwrap();
    let ready = control.path().join("ready");
    let mut worker = Worker(
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
        thread::sleep(Duration::from_millis(20));
    };
    worker.0.kill().unwrap();
    worker.0.wait().unwrap();
    let directory = std::path::Path::new(target["directory"].as_str().unwrap());
    let deadline = Instant::now() + TIMEOUT;
    while directory.exists() {
        assert!(
            Instant::now() < deadline,
            "supervisor did not remove the fixture directory"
        );
        thread::sleep(Duration::from_millis(20));
    }
    assert!(
        client()
            .get(format!("{}/health", target["address"].as_str().unwrap()))
            .send()
            .is_err(),
        "server survived runner termination"
    );
}
