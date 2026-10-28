# Development

The Tauri app requires Rust 1.90 or newer. After updating the Tauri CLI and Rust crates,
regenerate Android with `npm --prefix app run tauri -- android init --ci`, then run
`scripts/sync-tauri-android-overrides.sh`. Tauri 2.12 generates Gradle 9.6.1 / AGP 9.3.1
projects targeting Android API 37; install that SDK platform before Android checks.
The maintained Android library and IPC sample compile Java and Kotlin to JVM 17.
Keep the debug package suffix in `bundle.android.debugApplicationIdSuffix` so the CLI
preserves it when rewriting generated Gradle files.
Keep `app/package-lock.json` and `app/pnpm-lock.yaml` synchronized when updating npm dependencies.
Dependabot checks GitHub Actions, npm in `app`, both Cargo projects, and the Android library
and IPC sample Gradle dependencies weekly. Generated Android toolchain versions remain owned
by the Tauri template; the Kotlin override is maintained under `app/src-tauri/android/patches`.

Android sources belong in `app/src-tauri/android`. The generated Tauri checkout is ignored;
apply canonical activity overrides with `scripts/sync-tauri-android-overrides.sh` after
Tauri builds before packaging a variant, because regeneration can reset its application ID.
Frontend assets are embedded in the Rust library, so frontend edits require a Tauri build;
a Kotlin-only Gradle rebuild using an older library does not update the UI.

`LlmdServiceHost` owns the Off → Starting → Ready → Stopping → Off lifecycle on an injected
serial dispatcher. Engine operations run on an injected worker. The application singleton
owns it across activities and Binder connections. The private foreground service observes
it and exposes an ongoing stop action. Binder clients cannot start or replace a model.
Stopping cancels provider jobs and waits for conversation cleanup before closing the engine.
Import and deletion require a stopped service; imports retain the original filename and
publish only after the temporary copy completes.

Run `scripts/check-static.sh --host --android`, `cargo test --workspace`,
`cargo test --manifest-path app/src-tauri/Cargo.toml`, and `git diff --check` after changes.
Run `:llmd-android:testDebugUnitTest` in the generated Android project for lifecycle and
inventory tests. They cover explicit selection, readiness, cancelled loading, resource
release, failed initialization without fallback, and exact inventory resolution.

For a connected device, acquire the device-side test lock before installation or testing.
Device locking is managed externally by the caller or AI plugin; the test entry point
runs directly without requiring an agent plugin installation.
Set `GEMMA_MODEL_PATH` explicitly when running `scripts/test-android-appium.sh`.
`LLMD_ANDROID_E2E_SKIP_BUILD=1` uses already installed app/sample APKs;
`LLMD_ANDROID_E2E_IMPORT_ONLY=1` checks import, explicit selection, and service readiness
without claiming IPC generation coverage. Sessions force-launch the selected app so
coinstalled variants cannot receive another variant's test actions.
The runner installs in place without clearing app data or allowing version downgrades.

## Background operation

User-visible Binder clients should bind with `BIND_AUTO_CREATE | BIND_IMPORTANT` so
inference shares the foreground caller's process importance. This does not load models
or make the engine immune to system memory reclamation. The sample demonstrates the
binding; the explicit foreground model service retains ownership across disconnects.

On Android 13+, MainActivity requests notification permission before allocating the
selected engine. It retains the pending explicit model ID across Activity recreation.
The Status UI reports notification permission and opens standard app settings. Depending
on the manufacturer, users may also need to allow background running and protect or lock
LLMD in Recent apps. These system choices are separate from the service switch.

## Verification

The Test workflow runs on pull requests, relevant pushes to main, and manual dispatches.
Main runs seed Rust dependency caches that subsequent pull requests can restore. Linux
host checks cache both the root workspace's `target` and `app/src-tauri/target`; Windows
packaging caches only the latter. Android debug validation and release packaging cache
`app/src-tauri/target` under separate keys. Their target artifacts use different profiles
and are not interchangeable; each cache also contains Cargo registry/git downloads.
Keep cache keys separated by platform and build purpose. The Rust cache action retains
dependencies rather than workspace outputs or incremental compilation state. Compare
cold and warm runs, including cache restore/save time and size, before claiming savings.

Live Binder checks covered exact model inventory, missing/blank/unknown IDs, valid IDs
while Off, mismatched IDs while Ready, and disconnect/rebind without loading or replacing
a model. Explicit E4B text and synthetic balance-image generation passed, including
Summer's preview/persistence flow. Guided text and image response-format probes also
passed with the exact active model unchanged. Memory-pressure and OEM reclamation
failures were observed separately; passing fixtures do not establish arbitrary-image
accuracy or universal background stability. No model/backend fallback was used.

Background validation checks that no engine starts while the notification dialog is
visible, permission status and the settings shortcut work, and the first image request
after background idle retains the same service process. Keep raw logs, device identifiers,
hardware specifications, and device-specific memory measurements outside the repository.

Project host/Android static checks, Tauri packaging/tests, Android lifecycle/inventory
unit tests, and Summer build/unit/Lint checks passed. The optional full LLMD Android
Lint run still reports generated TV manifest issues `MissingTvBanner` and
`ImpliedTouchscreenHardware`; no new Lint errors were found in the changed code.
