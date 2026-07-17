# Android Tauri OpenAI API Plan

Android uses the shared Tauri UI from `app`.

The Android client should:

- import or discover local `.litertlm` models,
- select the active model,
- start a device-local OpenAI-compatible HTTP server,
- show service logs and model status.

Tests should target the OpenAI-compatible API rather than the prompt UI:

1. `scripts/prepare-android-model.sh` pushes `gemma-4-E2B-it.litertlm` to the device.
2. The Tauri Android app starts its local API server on device port `11435`.
3. `adb reverse tcp:11435 tcp:11435` exposes the device server to the host.
4. `scripts/test-openai-api.sh` verifies `/health`, `/v1/models`, and `/v1/chat/completions`.

Appium is reserved for UI management checks such as import controls, selected model display, and log views.
