# AGENTS.md

## Static Checks

Before handing off code changes, run the relevant static checks from the repository root:

```bash
./scripts/check-static.sh --host
```

Use Android checks when touching Android, JNI, Tauri mobile generation, or Android workflows:

```bash
./scripts/check-static.sh --android
```

If only the Android generated Gradle/Kotlin/AIDL side changed, this narrower check is acceptable:

```bash
./scripts/check-static.sh --android-kotlin
```

The frontend checks expect dependencies to be installed first:

```bash
npm --prefix app ci
```

Run a patch hygiene check before handing off:

```bash
git diff --check
```

Generated Android sources and resources under `app/src-tauri/gen/android` may be ignored by
`.gitignore` even when they are intentionally part of a change. When adding new generated
Android files, verify them with:

```bash
git status --short --untracked-files=all
```

Use `git add -f <path>` only for specific generated files that are intentionally required by the
change.

## Tests

Run focused Rust tests for server/API behavior changes:

```bash
cargo test -p llmd-server
```

Run all workspace Rust tests when shared crates, CLI behavior, or cross-crate contracts change:

```bash
cargo test --workspace
```

Run Tauri Rust tests when `app/src-tauri` behavior changes:

```bash
cargo test --manifest-path app/src-tauri/Cargo.toml
```

For OpenAI-compatible HTTP API changes, test against a running endpoint:

```bash
LLMD_OPENAI_BASE_URL=http://127.0.0.1:11435 scripts/test-openai-api.sh
```

For Android model and device-local API validation, install a debuggable APK first, then prepare the
model and run the Android API test:

```bash
ANDROID_UDID=<device-serial> scripts/prepare-android-model.sh
ANDROID_UDID=<device-serial> scripts/test-android-openai-api.sh
```

For Android IPC authorization changes, manually verify on device or emulator:

- an unauthorized caller receives an `authorization_required` IPC response,
- the authorization Activity opens from `com.storytellerf.llmd.action.AUTHORIZE_CALLER`,
- approving the caller persists authorization through DataStore,
- the same caller can call health, model list, and chat IPC methods after approval,
- no `SharedPreferences` or `runBlocking` usage is introduced in the IPC authorization path.
