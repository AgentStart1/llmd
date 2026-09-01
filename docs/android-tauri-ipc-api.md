# Android LiteRT-LM IPC

Android uses the shared Tauri UI from `app`, but it does not expose a local HTTP server or listen
on port `11435`.

The app UI reads model state and health through its in-app JavaScript/native bridge. Other Android
apps bind to `com.storytellerf.llmd.action.BIND_IPC` and use the `ILlmdService` AIDL interface:

- `healthAsync`,
- `listModelsAsync`, and
- `chatCompletionAsync`.

`chatCompletionAsync` accepts OpenAI-style message content as either a plain string or a content
array. Android multi-modal requests may include text parts and Base64 `data:` image URLs in
`image_url` parts. JPEG, PNG, and WebP images are supported, with a 750,000-byte decoded limit per
image; only one image may appear across all messages in a request. System messages must contain
text only. The Android LiteRT engine enables its vision backend for the default Gemma model and
passes image bytes directly to LiteRT-LM instead of tokenizing Base64 text.
The engine prefers the LiteRT GPU backend for both language and vision execution. If GPU engine
initialization or inference fails on a device, it closes that engine and retries the same request
with CPU. Android manifests declare the optional `libvndksupport.so` and `libOpenCL.so` vendor
libraries so supported devices can load their OpenCL driver. Logcat entries tagged `llmd` report
the selected backend and initialization/generation duration.

Each external caller must be authorized through
`com.storytellerf.llmd.action.AUTHORIZE_CALLER` before IPC calls return model or chat results.
The Android application IDs are `com.storytellerf.llmd` for Release, `com.storytellerf.llmd.daily`
for Daily, and `com.storytellerf.llmd.debug` for Debug. The exported component classes and intent
actions retain the base `com.storytellerf.llmd` namespace across all variants.

## End-to-end import check

`scripts/test-android-appium.sh` builds and installs the selected Android variant, pushes the
model to Downloads, and imports it through the system document picker. The test passes only after
the UI's native bridge reports that the model was imported.

```bash
ANDROID_UDID=<device-serial> scripts/test-android-appium.sh
```

Use `--e2e` to exercise the minified, debug-signed variant.
