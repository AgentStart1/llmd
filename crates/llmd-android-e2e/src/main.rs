use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, SystemTime},
};

use anyhow::{anyhow, bail, Context, Result};
use appium_client::{
    capabilities::{
        android::AndroidCapabilities, AppiumCapability, UdidCapable, UiAutomator2AppCompatible,
    },
    find::By,
    wait::AppiumWait,
    ClientBuilder,
};

const DEFAULT_PACKAGE: &str = "com.storytellerf.llmd";
const DEFAULT_MODEL: &str = "gemma-4-E2B-it";
const DEFAULT_PORT: &str = "11435";

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::from_env()?;
    ensure_model_exists(&config.model_path)?;

    run_status(command("adb", &config).arg("wait-for-device"))?;
    build_and_install_apk(&config)?;
    push_model_to_downloads(&config)?;

    let mut appium = ensure_appium(&config)?;
    let appium_result = import_model_with_appium(&config).await;
    if let Some(child) = appium.as_mut() {
        let _ = child.kill();
        let _ = child.wait();
    }
    appium_result?;

    forward_api_port(&config)?;
    run_openai_api_test(&config)?;

    Ok(())
}

struct Config {
    root_dir: PathBuf,
    device_serial: Option<String>,
    android_target: String,
    android_build_type: AndroidBuildType,
    android_variant: String,
    android_package: String,
    model_path: PathBuf,
    device_model_path: String,
    appium_url: String,
    local_port: String,
    remote_port: String,
}

impl Config {
    fn from_env() -> Result<Self> {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let root_dir = manifest_dir
            .ancestors()
            .nth(2)
            .context("unable to resolve repository root")?
            .to_path_buf();
        let model_path = env::var_os("GEMMA_MODEL_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| root_dir.join("models/gemma/gemma-4-E2B-it.litertlm"));
        let model_file_name = model_path
            .file_name()
            .and_then(|value| value.to_str())
            .context("model path must have a valid UTF-8 file name")?
            .to_owned();

        let android_target = env::var("ANDROID_TARGET").unwrap_or_else(|_| "arm64".to_string());
        let android_build_type = AndroidBuildType::from_env()?;
        let android_variant = env::var("LLMD_ANDROID_GRADLE_VARIANT").unwrap_or_else(|_| {
            format!(
                "{}{}",
                capitalize(&android_target),
                android_build_type.gradle_suffix()
            )
        });

        Ok(Self {
            root_dir,
            device_serial: env::var("ANDROID_UDID")
                .or_else(|_| env::var("ANDROID_SERIAL"))
                .ok()
                .filter(|value| !value.is_empty()),
            android_target,
            android_build_type,
            android_variant,
            android_package: env::var("ANDROID_PACKAGE")
                .unwrap_or_else(|_| DEFAULT_PACKAGE.to_string()),
            device_model_path: env::var("LLMD_ANDROID_DEVICE_MODEL_PATH")
                .unwrap_or_else(|_| format!("/sdcard/Download/{model_file_name}")),
            appium_url: env::var("APPIUM_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:4723/".to_string()),
            local_port: env::var("LLMD_ANDROID_LOCAL_PORT")
                .unwrap_or_else(|_| DEFAULT_PORT.to_string()),
            remote_port: env::var("LLMD_ANDROID_REMOTE_PORT")
                .unwrap_or_else(|_| DEFAULT_PORT.to_string()),
            model_path,
        })
    }
}

#[derive(Clone, Copy)]
enum AndroidBuildType {
    Debug,
    E2e,
}

impl AndroidBuildType {
    fn from_env() -> Result<Self> {
        match env::var("LLMD_ANDROID_BUILD_TYPE")
            .unwrap_or_else(|_| "debug".to_string())
            .as_str()
        {
            "debug" => Ok(Self::Debug),
            "e2e" => Ok(Self::E2e),
            other => bail!("LLMD_ANDROID_BUILD_TYPE must be debug or e2e, got {other}"),
        }
    }

    fn gradle_suffix(self) -> &'static str {
        match self {
            Self::Debug => "Debug",
            Self::E2e => "E2e",
        }
    }

    fn gradle_name(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::E2e => "e2e",
        }
    }
}

fn build_and_install_apk(config: &Config) -> Result<()> {
    run_status(
        Command::new(
            config
                .root_dir
                .join("scripts/sync-tauri-android-overrides.sh"),
        )
        .current_dir(&config.root_dir),
    )?;

    match config.android_build_type {
        AndroidBuildType::Debug => build_debug_apk(config)?,
        AndroidBuildType::E2e => build_e2e_apk(config)?,
    }

    let apk = latest_apk(config)?;
    let _ = command("adb", config)
        .args(["uninstall", &config.android_package])
        .status();
    run_status(
        command("adb", config)
            .args(["install", "-r", "-d"])
            .arg(apk),
    )
}

fn build_debug_apk(config: &Config) -> Result<()> {
    run_status(
        Command::new("npx")
            .current_dir(config.root_dir.join("app"))
            .args([
                "tauri",
                "android",
                "build",
                "--apk",
                "--debug",
                "--target",
                tauri_target(&config.android_target),
                "--ci",
            ]),
    )
}

fn build_e2e_apk(config: &Config) -> Result<()> {
    run_status(
        Command::new("npx")
            .current_dir(config.root_dir.join("app"))
            .args([
                "tauri",
                "android",
                "build",
                "--apk",
                "--target",
                tauri_target(&config.android_target),
                "--ci",
            ]),
    )?;
    run_status(
        Command::new(
            config
                .root_dir
                .join("scripts/sync-tauri-android-overrides.sh"),
        )
        .current_dir(&config.root_dir),
    )?;

    let gradle_task = format!(":app:assemble{}", config.android_variant);
    let rust_task = format!(":app:rustBuild{}", config.android_variant);
    run_status(
        Command::new("./gradlew")
            .current_dir(config.root_dir.join("app/src-tauri/gen/android"))
            .arg(gradle_task)
            .arg("-x")
            .arg(rust_task)
            .arg(format!(
                "-PtargetList={}",
                tauri_target(&config.android_target)
            ))
            .arg(format!("-ParchList={}", config.android_target))
            .arg(format!(
                "-PabiList={}",
                abi_for_target(&config.android_target)?
            ))
            .arg("--no-daemon"),
    )
}

fn latest_apk(config: &Config) -> Result<PathBuf> {
    let build_type = config.android_build_type.gradle_name();
    let output_dir = config
        .root_dir
        .join("app/src-tauri/gen/android/app/build/outputs/apk");
    let mut latest = None;
    collect_latest_apk(&output_dir, build_type, &mut latest)?;
    latest
        .map(|(_, path)| path)
        .ok_or_else(|| anyhow!("no {build_type} APK found under {}", output_dir.display()))
}

fn collect_latest_apk(
    dir: &Path,
    build_type: &str,
    latest: &mut Option<(SystemTime, PathBuf)>,
) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_latest_apk(&path, build_type, latest)?;
        } else if path.extension().and_then(|value| value.to_str()) == Some("apk")
            && path
                .parent()
                .and_then(|value| value.file_name())
                .and_then(|value| value.to_str())
                == Some(build_type)
        {
            let modified = path.metadata()?.modified()?;
            if latest
                .as_ref()
                .map(|(current, _)| modified > *current)
                .unwrap_or(true)
            {
                *latest = Some((modified, path));
            }
        }
    }

    Ok(())
}

fn push_model_to_downloads(config: &Config) -> Result<()> {
    let parent = Path::new(&config.device_model_path)
        .parent()
        .and_then(|value| value.to_str())
        .unwrap_or("/sdcard/Download");
    run_status(command("adb", config).args(["shell", "mkdir", "-p", parent]))?;
    run_status(
        command("adb", config)
            .arg("push")
            .arg(&config.model_path)
            .arg(&config.device_model_path),
    )
}

async fn import_model_with_appium(config: &Config) -> Result<()> {
    let mut capabilities = AndroidCapabilities::new_uiautomator();
    if let Some(serial) = &config.device_serial {
        capabilities.udid(serial);
    }
    capabilities.app_package(&config.android_package);
    capabilities.app_activity(&format!("{}.MainActivity", config.android_package));
    capabilities.set_bool("appium:autoGrantPermissions", true);
    capabilities.set_bool("appium:noReset", true);
    capabilities.set_number("appium:newCommandTimeout", 180u64.into());

    let client = ClientBuilder::rustls(capabilities)
        .connect(&config.appium_url)
        .await
        .with_context(|| format!("connect Appium server at {}", config.appium_url))?;

    let result = async {
        wait_click(&client, text("Import model"), Duration::from_secs(180)).await?;
        select_model_in_picker(&client, config).await?;
        wait_for_any(
            &client,
            &[text("Model imported."), text("Model is ready.")],
            Duration::from_secs(900),
        )
        .await
    }
    .await;

    client.clone().close().await.ok();
    result
}

async fn select_model_in_picker(
    client: &appium_client::AndroidClient,
    config: &Config,
) -> Result<()> {
    let model_name = Path::new(&config.device_model_path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(DEFAULT_MODEL);
    let model_without_extension = model_name.trim_end_matches(".litertlm");

    if try_click(client, contains_text(model_name), Duration::from_secs(10)).await? {
        return Ok(());
    }

    let _ = try_click(
        client,
        content_desc_contains("Show roots"),
        Duration::from_secs(5),
    )
    .await?;
    let _ = try_click(
        client,
        content_desc_contains("Open navigation drawer"),
        Duration::from_secs(5),
    )
    .await?;
    let _ = try_click(client, text("Downloads"), Duration::from_secs(5)).await?;
    let _ = try_click(client, text("Download"), Duration::from_secs(5)).await?;

    if try_click(client, contains_text(model_name), Duration::from_secs(180)).await? {
        return Ok(());
    }
    if try_click(
        client,
        contains_text(model_without_extension),
        Duration::from_secs(10),
    )
    .await?
    {
        return Ok(());
    }

    bail!("unable to select {model_name} in Android document picker")
}

async fn wait_click(
    client: &appium_client::AndroidClient,
    selector: String,
    timeout: Duration,
) -> Result<()> {
    let element = client
        .appium_wait()
        .at_most(timeout)
        .check_every(Duration::from_millis(500))
        .for_element(By::xpath(&selector))
        .await?;
    element.click().await?;
    Ok(())
}

async fn try_click(
    client: &appium_client::AndroidClient,
    selector: String,
    timeout: Duration,
) -> Result<bool> {
    match wait_click(client, selector, timeout).await {
        Ok(()) => Ok(true),
        Err(error) if error.to_string().contains("no such element") => Ok(false),
        Err(error) if error.to_string().contains("NoSuchElement") => Ok(false),
        Err(error) if error.to_string().contains("timeout") => Ok(false),
        Err(error) => Err(error),
    }
}

async fn wait_for_any(
    client: &appium_client::AndroidClient,
    selectors: &[String],
    timeout: Duration,
) -> Result<()> {
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        for selector in selectors {
            if try_click(client, selector.clone(), Duration::from_millis(500)).await? {
                return Ok(());
            }
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    bail!("timed out waiting for model import completion")
}

fn forward_api_port(config: &Config) -> Result<()> {
    let _ = command("adb", config)
        .args(["forward", "--remove", &format!("tcp:{}", config.local_port)])
        .status();
    run_status(command("adb", config).args([
        "forward",
        &format!("tcp:{}", config.local_port),
        &format!("tcp:{}", config.remote_port),
    ]))
}

fn run_openai_api_test(config: &Config) -> Result<()> {
    run_status(
        Command::new(config.root_dir.join("scripts/test-openai-api.sh"))
            .current_dir(&config.root_dir)
            .env(
                "LLMD_OPENAI_BASE_URL",
                format!("http://127.0.0.1:{}", config.local_port),
            ),
    )
}

fn ensure_appium(config: &Config) -> Result<Option<Child>> {
    if appium_is_ready(&config.appium_url) {
        return Ok(None);
    }

    let log_path = env::var_os("LLMD_APPIUM_LOG")
        .map(PathBuf::from)
        .unwrap_or_else(|| config.root_dir.join("appium.log"));
    let log = std::fs::File::create(&log_path)
        .with_context(|| format!("create Appium log {}", log_path.display()))?;
    let child = Command::new("appium")
        .args(["--address", "127.0.0.1", "--port", "4723"])
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log))
        .spawn()
        .context("start Appium server")?;

    for _ in 0..30 {
        if appium_is_ready(&config.appium_url) {
            return Ok(Some(child));
        }
        std::thread::sleep(Duration::from_secs(1));
    }

    bail!("Appium did not become ready; see {}", log_path.display())
}

fn appium_is_ready(appium_url: &str) -> bool {
    let status_url = format!("{}/status", appium_url.trim_end_matches('/'));
    Command::new("curl")
        .args(["--fail", "--silent", &status_url])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn ensure_model_exists(path: &Path) -> Result<()> {
    if path.is_file() && path.metadata()?.len() > 0 {
        return Ok(());
    }
    bail!(
        "Gemma model is missing: {}. Run scripts/download-gemma-model.sh or set GEMMA_MODEL_PATH.",
        path.display()
    )
}

fn command(program: &str, config: &Config) -> Command {
    let mut command = Command::new(program);
    if program == "adb" {
        if let Some(serial) = &config.device_serial {
            command.args(["-s", serial]);
        }
    }
    command
}

fn run_status(command: &mut Command) -> Result<()> {
    let status = command
        .status()
        .with_context(|| format!("run command {:?}", command))?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("command {:?} failed with {status}", command))
    }
}

fn tauri_target(target: &str) -> &str {
    match target {
        "arm64" => "aarch64",
        "arm" => "armv7",
        "x86" => "i686",
        "x86_64" => "x86_64",
        _ => target,
    }
}

fn abi_for_target(target: &str) -> Result<&'static str> {
    match target {
        "arm64" => Ok("arm64-v8a"),
        "arm" => Ok("armeabi-v7a"),
        "x86" => Ok("x86"),
        "x86_64" => Ok("x86_64"),
        _ => bail!("unsupported Android target {target}"),
    }
}

fn capitalize(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn text(value: &str) -> String {
    format!("//*[@text={}]", xpath_literal(value))
}

fn contains_text(value: &str) -> String {
    format!("//*[contains(@text,{})]", xpath_literal(value))
}

fn content_desc_contains(value: &str) -> String {
    format!("//*[contains(@content-desc,{})]", xpath_literal(value))
}

fn xpath_literal(value: &str) -> String {
    if !value.contains('\'') {
        return format!("'{value}'");
    }
    if !value.contains('"') {
        return format!("\"{value}\"");
    }
    format!("concat('{}')", value.replace('\'', "',\"'\",'"))
}
