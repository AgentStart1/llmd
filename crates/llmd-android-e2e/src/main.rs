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
use fantoccini::actions::{InputSource, PointerAction, TouchActions, MOUSE_BUTTON_LEFT};

const RELEASE_PACKAGE: &str = "com.storytellerf.llmd";
const ALPHA_PACKAGE: &str = "com.storytellerf.llmd.alpha";
const DEBUG_PACKAGE: &str = "com.storytellerf.llmd.debug";
const E2E_PACKAGE: &str = "com.storytellerf.llmd.e2e";
const MAIN_ACTIVITY: &str = "com.storytellerf.llmd.MainActivity";
const SAMPLE_ACTIVITY: &str = "com.storytellerf.llmd.sample.IpcSampleActivity";
const DEFAULT_MODEL: &str = "gemma-4-E2B-it";

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
    let mut appium = ensure_appium(&config)?;
    let sample_result = run_ipc_sample(&config).await;
    if let Some(child) = appium.as_mut() {
        let _ = child.kill();
        let _ = child.wait();
    }
    sample_result?;

    Ok(())
}

struct Config {
    root_dir: PathBuf,
    device_serial: Option<String>,
    android_target: String,
    android_build_type: AndroidBuildType,
    android_variant: String,
    android_package: String,
    sample_package: String,
    model_path: PathBuf,
    device_model_path: String,
    appium_url: String,
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
                .unwrap_or_else(|_| android_build_type.default_package().to_string()),
            sample_package: env::var("LLMD_SAMPLE_ANDROID_PACKAGE")
                .unwrap_or_else(|_| "com.storytellerf.llmd.sample.debug".to_string()),
            device_model_path: env::var("LLMD_ANDROID_DEVICE_MODEL_PATH")
                .unwrap_or_else(|_| format!("/sdcard/Download/{model_file_name}")),
            appium_url: env::var("APPIUM_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:4723/".to_string()),
            model_path,
        })
    }
}

#[derive(Clone, Copy)]
enum AndroidBuildType {
    Debug,
    Release,
    Alpha,
    E2e,
}

impl AndroidBuildType {
    fn from_env() -> Result<Self> {
        match env::var("LLMD_ANDROID_BUILD_TYPE")
            .unwrap_or_else(|_| "debug".to_string())
            .as_str()
        {
            "debug" => Ok(Self::Debug),
            "release" => Ok(Self::Release),
            "alpha" => Ok(Self::Alpha),
            "e2e" => Ok(Self::E2e),
            other => {
                bail!("LLMD_ANDROID_BUILD_TYPE must be debug, release, alpha, or e2e, got {other}")
            }
        }
    }

    fn gradle_suffix(self) -> &'static str {
        match self {
            Self::Debug => "Debug",
            Self::Release => "Release",
            Self::Alpha => "Alpha",
            Self::E2e => "E2e",
        }
    }

    fn gradle_name(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Release => "release",
            Self::Alpha => "alpha",
            Self::E2e => "e2e",
        }
    }

    fn default_package(self) -> &'static str {
        match self {
            Self::Debug => DEBUG_PACKAGE,
            Self::Release => RELEASE_PACKAGE,
            Self::Alpha => ALPHA_PACKAGE,
            Self::E2e => E2E_PACKAGE,
        }
    }
}

fn build_and_install_apk(config: &Config) -> Result<()> {
    run_status(&mut sync_android_overrides(config))?;

    build_apk(config)?;

    let apk = latest_apk(config)?;
    let sample_apk = build_sample_apk(config)?;
    let _ = command("adb", config)
        .args(["uninstall", &config.android_package])
        .status();
    run_status(
        command("adb", config)
            .args(["install", "-r", "-d"])
            .arg(apk),
    )?;
    let _ = command("adb", config)
        .args(["uninstall", &config.sample_package])
        .status();
    run_status(
        command("adb", config)
            .args(["install", "-r", "-d"])
            .arg(sample_apk),
    )
}

fn build_apk(config: &Config) -> Result<()> {
    let mut args = vec!["tauri", "android", "build", "--apk"];
    if matches!(config.android_build_type, AndroidBuildType::Debug) {
        args.push("--debug");
    }
    args.extend(["--target", tauri_target(&config.android_target), "--ci"]);

    run_status(
        Command::new(if cfg!(windows) { "npx.cmd" } else { "npx" })
            .current_dir(config.root_dir.join("app"))
            .args(args),
    )?;
    run_status(&mut sync_android_overrides(config))?;

    let gradle_task = format!(":app:assemble{}", config.android_variant);
    let rust_build_task = format!(":app:rustBuild{}", config.android_variant);
    run_status(
        gradle_command(config)
            .arg(gradle_task)
            .arg(format!(
                "-PtargetList={}",
                tauri_target(&config.android_target)
            ))
            .arg(format!("-ParchList={}", config.android_target))
            .arg(format!(
                "-PabiList={}",
                abi_for_target(&config.android_target)?
            ))
            // Tauri has already built and staged this ABI's native library. Running
            // this task again outside the Tauri process attempts to reconnect to its
            // transient WebSocket server, which no longer exists.
            .arg("-x")
            .arg(rust_build_task)
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

fn build_sample_apk(config: &Config) -> Result<PathBuf> {
    let gradle_task = ":llmd-sample:assembleDebug";
    run_status(gradle_command(config).arg(gradle_task).arg("--no-daemon"))?;
    let output_dir = config
        .root_dir
        .join("app/src-tauri/android/llmd-sample/build/outputs/apk");
    let mut latest = None;
    collect_latest_apk(&output_dir, "debug", &mut latest)?;
    latest.map(|(_, path)| path).ok_or_else(|| {
        anyhow!(
            "no {} sample APK found under {}",
            "debug",
            output_dir.display()
        )
    })
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
    let local_size = fs::metadata(&config.model_path)
        .with_context(|| format!("read local model metadata: {}", config.model_path.display()))?
        .len();
    if device_file_size(config, &config.device_model_path)? == Some(local_size) {
        println!(
            "Model already exists on device with matching size: {}",
            config.device_model_path
        );
        return Ok(());
    }

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

fn device_file_size(config: &Config, path: &str) -> Result<Option<u64>> {
    let output = command("adb", config)
        .args(["exec-out", "stat", "-c", "%s", path])
        .output()
        .with_context(|| format!("check model on device: {path}"))?;
    if !output.status.success() {
        return Ok(None);
    }
    let size = String::from_utf8(output.stdout)
        .context("read device model size")?
        .trim()
        .parse()
        .with_context(|| format!("parse device model size: {path}"))?;
    Ok(Some(size))
}

async fn import_model_with_appium(config: &Config) -> Result<()> {
    let mut capabilities = AndroidCapabilities::new_uiautomator();
    if let Some(serial) = &config.device_serial {
        capabilities.udid(serial);
    }
    capabilities.app_package(&config.android_package);
    capabilities.app_activity(MAIN_ACTIVITY);
    capabilities.set_bool("appium:autoGrantPermissions", true);
    capabilities.set_bool("appium:noReset", true);
    capabilities.set_number("appium:newCommandTimeout", 180u64.into());

    let client = ClientBuilder::rustls(capabilities)
        .connect(&config.appium_url)
        .await
        .with_context(|| format!("connect Appium server at {}", config.appium_url))?;

    let result = async {
        wait_click(&client, text("Models"), Duration::from_secs(180)).await?;
        wait_click(&client, text("Import model"), Duration::from_secs(180)).await?;
        select_model_in_picker(&client, config).await?;
        wait_for_any(&client, &[text(DEFAULT_MODEL)], Duration::from_secs(900)).await
    }
    .await;

    client.clone().close().await.ok();
    result
}

async fn run_ipc_sample(config: &Config) -> Result<()> {
    const SAMPLE_UI_DUMP: &str = "/sdcard/llmd-provider-sample.xml";

    let component = format!("{}/{}", config.sample_package, SAMPLE_ACTIVITY);
    run_status(command("adb", config).args([
        "shell",
        "am",
        "start",
        "-n",
        &component,
        "--es",
        "llmdPackage",
        &config.android_package,
    ]))?;
    let mut capabilities = AndroidCapabilities::new_uiautomator();
    if let Some(serial) = &config.device_serial {
        capabilities.udid(serial);
    }
    capabilities.app_package(&config.sample_package);
    capabilities.app_activity(SAMPLE_ACTIVITY);
    capabilities.set_bool("appium:autoGrantPermissions", true);
    capabilities.set_bool("appium:noReset", true);
    let client = ClientBuilder::rustls(capabilities)
        .connect(&config.appium_url)
        .await
        .with_context(|| format!("connect Appium server at {}", config.appium_url))?;
    let authorization_result = wait_click(&client, text("ALLOW"), Duration::from_secs(60)).await;
    client.clone().close().await.ok();
    authorization_result?;

    run_status(command("adb", config).args(["shell", "rm", "-f", SAMPLE_UI_DUMP]))?;
    let deadline = std::time::Instant::now() + Duration::from_secs(900);
    while std::time::Instant::now() < deadline {
        run_status(command("adb", config).args(["shell", "uiautomator", "dump", SAMPLE_UI_DUMP]))?;
        let output = command("adb", config)
            .args(["shell", "cat", SAMPLE_UI_DUMP])
            .output()
            .context("read LiteRT-LM sample UI state")?;
        let state = String::from_utf8_lossy(&output.stdout);
        if state.contains("PASS:") {
            return Ok(());
        }
        if state.contains("FAIL:") {
            bail!("LiteRT-LM provider sample failed: {state}");
        }
        std::thread::sleep(Duration::from_secs(1));
    }

    bail!("LiteRT-LM provider sample timed out")
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

    ensure_downloads_directory(client).await?;

    for _ in 0..20 {
        if try_click(
            client,
            contains_text(model_name),
            Duration::from_millis(500),
        )
        .await?
            || try_click(
                client,
                contains_text(model_without_extension),
                Duration::from_millis(500),
            )
            .await?
        {
            return Ok(());
        }
        scroll_picker_down(client).await?;
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    bail!("unable to select {model_name} in Android document picker")
}

async fn ensure_downloads_directory(client: &appium_client::AndroidClient) -> Result<()> {
    if picker_is_showing_downloads(client).await? {
        return Ok(());
    }

    let navigation_opened = try_click(
        client,
        content_desc_contains("Show roots"),
        Duration::from_secs(5),
    )
    .await?
        || try_click(
            client,
            content_desc_contains("Open navigation drawer"),
            Duration::from_secs(5),
        )
        .await?;
    if !navigation_opened {
        bail!("unable to open Android document picker navigation")
    }

    wait_click(client, clickable_text("Downloads"), Duration::from_secs(10)).await?;
    wait_for_picker_downloads(client, Duration::from_secs(10)).await
}

async fn picker_is_showing_downloads(client: &appium_client::AndroidClient) -> Result<bool> {
    Ok(client
        .appium_wait()
        .at_most(Duration::from_millis(500))
        .for_element(By::xpath(&documents_ui_downloads_breadcrumb()))
        .await
        .is_ok())
}

async fn wait_for_picker_downloads(
    client: &appium_client::AndroidClient,
    timeout: Duration,
) -> Result<()> {
    client
        .appium_wait()
        .at_most(timeout)
        .check_every(Duration::from_millis(250))
        .for_element(By::xpath(&documents_ui_downloads_breadcrumb()))
        .await?;
    Ok(())
}

async fn scroll_picker_down(client: &appium_client::AndroidClient) -> Result<()> {
    let (width, height) = client.get_window_size().await?;
    let x = (width / 2) as i64;
    let from_y = (height as f64 * 0.8) as i64;
    let to_y = (height as f64 * 0.25) as i64;
    let gesture = TouchActions::new("picker-scroll".to_string())
        .then(PointerAction::MoveTo {
            duration: Some(Duration::ZERO),
            x,
            y: from_y,
        })
        .then(PointerAction::Down {
            button: MOUSE_BUTTON_LEFT,
        })
        .then(PointerAction::MoveTo {
            duration: Some(Duration::from_millis(350)),
            x,
            y: to_y,
        })
        .then(PointerAction::Up {
            button: MOUSE_BUTTON_LEFT,
        });
    client.perform_actions(gesture).await?;
    Ok(())
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

fn ensure_appium(config: &Config) -> Result<Option<Child>> {
    if appium_is_ready(&config.appium_url) {
        return Ok(None);
    }

    let log_path = env::var_os("LLMD_APPIUM_LOG")
        .map(PathBuf::from)
        .unwrap_or_else(|| config.root_dir.join("appium.log"));
    let log = std::fs::File::create(&log_path)
        .with_context(|| format!("create Appium log {}", log_path.display()))?;
    let child = Command::new(if cfg!(windows) {
        "appium.cmd"
    } else {
        "appium"
    })
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

fn sync_android_overrides(config: &Config) -> Command {
    let script = config
        .root_dir
        .join("scripts/sync-tauri-android-overrides.sh");
    let mut command = if cfg!(windows) {
        let mut command = Command::new("bash");
        command.arg(script);
        command
    } else {
        Command::new(script)
    };
    command.current_dir(&config.root_dir);
    command
}

fn gradle_command(config: &Config) -> Command {
    let android_dir = config.root_dir.join("app/src-tauri/gen/android");
    let mut command = Command::new(android_dir.join(if cfg!(windows) {
        "gradlew.bat"
    } else {
        "gradlew"
    }));
    command.current_dir(android_dir);
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

fn clickable_text(value: &str) -> String {
    format!(
        "//*[@text={}]/ancestor::*[@clickable='true'][1]",
        xpath_literal(value)
    )
}

fn documents_ui_downloads_breadcrumb() -> String {
    "//*[@resource-id='com.android.documentsui:id/breadcrumb_text' and @text='Downloads']"
        .to_string()
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
