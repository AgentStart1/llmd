# Android Tauri OpenAI API Plan

Android uses the shared Tauri UI from `app`.

The Android client should:

- import or discover local `.litertlm` models,
- select the active model,
- start a device-local OpenAI-compatible HTTP server,
- show service logs and model status.

Android end-to-end tests should use Appium for model import and then target the
OpenAI-compatible API. The default test path uses the normal `debug` APK. When investigating
obfuscation regressions, pass `--e2e` or set `LLMD_ANDROID_BUILD_TYPE=e2e`; the `e2e` buildType is
debug-signed for local installation, but is not debuggable because AGP disables obfuscation for
debuggable builds.

1. `scripts/test-android-appium.sh` runs `cargo run -p llmd-android-e2e`.
2. The Rust runner builds the frontend and runs Gradle `:app:installArm64Debug` by default, or
   `:app:installArm64E2e` for obfuscation checks, so the installed APK is always produced from the
   current source tree.
3. The script pushes `gemma-4-E2B-it.litertlm` to device Downloads.
3. The Rust `appium-client` crate taps the app's import control and selects the model through
   Android's document picker.
4. The Tauri Android app starts its local API server on device port `11435`.
5. `adb forward tcp:11435 tcp:11435` exposes the device server to the host.
6. `scripts/test-openai-api.sh` verifies `/health`, `/v1/models`, and `/v1/chat/completions`.

`scripts/prepare-android-model.sh` remains available for debuggable APKs where `run-as` can write
the app-private model directory directly.

Appium is reserved for UI management checks such as import controls, selected model display, and log views.
