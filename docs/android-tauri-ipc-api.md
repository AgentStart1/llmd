# Android LiteRT-LM IPC

Android uses the shared Tauri UI from `app`, but it does not expose a local HTTP server or listen
on port `11435`.

The app UI reads model state and health through its in-app JavaScript/native bridge. Other Android
apps bind to `com.storytellerf.llmd.action.BIND_IPC` and use the `ILlmdService` AIDL interface:

- `healthAsync`,
- `listModelsAsync`, and
- `chatCompletionAsync`.

Each external caller must be authorized through
`com.storytellerf.llmd.action.AUTHORIZE_CALLER` before IPC calls return model or chat results.

## End-to-end import check

`scripts/test-android-appium.sh` builds and installs the selected Android variant, pushes the
model to Downloads, and imports it through the system document picker. The test passes only after
the UI's native bridge reports that the model was imported.

```bash
ANDROID_UDID=<device-serial> scripts/test-android-appium.sh
```

Use `--e2e` to exercise the minified, debug-signed variant.
