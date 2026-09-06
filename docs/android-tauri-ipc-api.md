# Android LiteRT-LM IPC

Android uses the shared Tauri UI from `app`, but it does not expose a local HTTP server or listen
on port `11435`.

The app UI reads model state and health through its in-app JavaScript/native bridge. Other Android
apps bind to `com.storytellerf.llmd.action.BIND_IPC` and use the `ILlmdService` AIDL interface:

- `healthAsync`,
- `listModelsAsync`, and
- `chatCompletionAsync`.

`chatCompletionAsync` accepts OpenAI-style message content as either a plain string or a content
array. Android multi-modal requests may include text parts and a `content://` URI in `image_url`
parts. The URI must come from the calling app's `FileProvider`, and the caller must grant llmd
temporary read access before making the Binder call. Keeping image bytes outside `requestJson`
prevents Base64 expansion from exceeding Binder's transaction limit.

For example, the calling app can create and grant an image URI before building the request:

```kotlin
val imageUri = FileProvider.getUriForFile(context, "${context.packageName}.fileprovider", imageFile)
val llmdPackageName = "com.storytellerf.llmd" // Use the package of the bound llmd variant.
context.grantUriPermission(
    llmdPackageName,
    imageUri,
    Intent.FLAG_GRANT_READ_URI_PERMISSION,
)
```

JPEG, PNG, and WebP images are supported, with a 750,000-byte limit per image; only one image may
appear across all messages in a request. The service verifies the URI owner, MIME type, decoded
image bounds, and read grant before passing image bytes to LiteRT-LM. System messages must contain
text only.

The LiteRT engine uses the GPU backend for language and vision execution. GPU initialization or
inference failure is returned to the caller instead of retrying on CPU. The health response includes
`engineReady`; callers should wait for it to become `true`, because generation requests fail
immediately while the engine is initializing or unavailable. Android manifests declare the optional
`libvndksupport.so` and `libOpenCL.so` vendor libraries so supported devices can load their OpenCL
driver. Logcat entries tagged `llmd` report initialization and generation duration.

Each external caller must be authorized through
`com.storytellerf.llmd.action.AUTHORIZE_CALLER` before IPC calls return model or chat results.
The Android application IDs are `com.storytellerf.llmd` for Release, `com.storytellerf.llmd.alpha`
for Alpha, `com.storytellerf.llmd.debug` for Debug, and `com.storytellerf.llmd.e2e` for E2E. The
exported component classes and intent actions retain the base `com.storytellerf.llmd` namespace
across all variants.

## End-to-end import check

`scripts/test-android-appium.sh` builds and installs the selected llmd variant, pushes the model
to Downloads, and imports it through the system document picker. It then installs the independent
IPC sample, authorizes it, and verifies health, model listing, text chat, and image chat through
the exported Binder service.

```bash
ANDROID_UDID=<device-serial> scripts/test-android-appium.sh
```

The IPC sample can select the Debug, Release, Alpha, or E2E llmd package at runtime. Use `--e2e`
to exercise the minified, debug-signed llmd variant.
