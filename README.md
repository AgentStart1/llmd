# llmd

`llmd` hosts local LiteRT-LM models behind an OpenAI-compatible API.

## Platform strategy

- Desktop: Tauri v2 app, using `maceip/rlitert-lm` through the Rust `litert-lm` crate.
- Terminal: Rust CLI/TUI, using `maceip/rlitert-lm`.
- Android: Tauri mobile UI with native LiteRT-LM Android inference. Android does not use the host-platform binary download path from `rlitert-lm`.

Default model:

```text
gemma-4-E2B-it
```

## Workspace

```text
app                    Tauri desktop and mobile shell
cli                    Terminal CLI/TUI
crates/llmd-core       Shared API types and provider trait
crates/llmd-rlitert    Desktop/terminal provider backed by rlitert-lm
crates/llmd-server     OpenAI-compatible HTTP API
docs                   Architecture and test notes
```

## CLI

Start the local API server:

```bash
cargo run -p llmd -- serve --port 11435
```

List locally downloaded LiteRT-LM models:

```bash
cargo run -p llmd -- models
```

Run one prompt:

```bash
cargo run -p llmd -- chat "Hello"
```

Open the terminal UI shell:

```bash
cargo run -p llmd -- tui
```

## HTTP API

```bash
curl http://127.0.0.1:11435/health
```

```bash
curl http://127.0.0.1:11435/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gemma-4-E2B-it",
    "messages": [{"role": "user", "content": "Hello"}]
  }'
```

## Desktop

```bash
cd app
npm install
npm run tauri dev
```

The desktop app has Status, Models, and Logs pages. On Models, enter a LiteRT-LM
model name to download it, select a model to inspect its details, and delete it
from its detail page when it is no longer needed.

## Android

Android uses the shared Tauri UI in `app`. The device app should expose the same OpenAI-compatible API as desktop and CLI, backed by native `litertlm-android`.

Use the Models page to choose a local model file to import. Selecting an imported
model opens its details page, where the file can be deleted from app-private storage.

Prepare the default Gemma 4 E2B model:

```bash
scripts/download-gemma-model.sh
```

Push the model to a connected Android device for a debuggable APK:

```bash
ANDROID_UDID=<device-serial> scripts/prepare-android-model.sh
```

The default model path is `models/gemma/gemma-4-E2B-it.litertlm`. Model files are ignored by git.
The Android preparation script copies that file into the app-private `files/models` directory, so install a debuggable APK before running it.

Run the Android end-to-end test through Appium. By default this builds and installs the normal
`debug` APK, pushes the model to device Downloads, imports it through the Android document picker,
then verifies that the native Android bridge reports the imported model:

```bash
ANDROID_UDID=<device-serial> scripts/test-android-appium.sh
```

When investigating obfuscation issues, use the `e2e` buildType. It keeps release minification
enabled while remaining debug-signed for local installation:

```bash
ANDROID_UDID=<device-serial> scripts/test-android-appium.sh --e2e
```

Android exposes LiteRT-LM to other Android apps only through its authorized Binder IPC interface;
it does not listen on a device TCP port. See `docs/android-tauri-ipc-api.md`.

## Tests

Run the core workspace tests:

```bash
cargo test --workspace
```

Run the Tauri desktop tests:

```bash
cargo test --manifest-path app/src-tauri/Cargo.toml
```

Run OpenAI-compatible API tests against any running endpoint:

```bash
LLMD_OPENAI_BASE_URL=http://127.0.0.1:11435 scripts/test-openai-api.sh
```

Run the real LiteRT-LM smoke test manually when the runtime and model are installed:

```bash
cargo test -p llmd-rlitert real_rlitert_chat_smoke_test -- --ignored
```
