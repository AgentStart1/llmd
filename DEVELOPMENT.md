# Development

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
Set `GEMMA_MODEL_PATH` explicitly when running `scripts/test-android-appium.sh`.
`LLMD_ANDROID_E2E_SKIP_BUILD=1` uses already installed app/sample APKs;
`LLMD_ANDROID_E2E_IMPORT_ONLY=1` checks import, explicit selection, and service readiness
without claiming IPC generation coverage. Sessions force-launch the selected app so
coinstalled variants cannot receive another variant's test actions.
`LLMD_ANDROID_E2E_LOCK_HELD=1` is only for a caller that already owns the device lock.
The runner installs in place without clearing app data or allowing version downgrades.

## Physical validation, 2026-10-13

The full Gemma 4 E4B file was checked against its official SHA256, imported through the
system picker, and reported alongside the existing E2B file. Import did not select or
load it. Explicit service startup initialized its GPU language engine in about 28 seconds.
The first Summer image request then caused Android's low-memory killer to terminate LLMD
while its vision graph was being prepared (about 3.6 GB process RSS on a 6 GB device).
That recognition test failed; language initialization alone does not prove image readiness.
No model or CPU fallback was used. Imported models and authorization survived the restart.

Summer's opt-in Binder contract test passed with the service Off: exact model inventory,
missing/blank/unknown IDs, valid IDs while Off, repeated health calls, and rebinding all
left the engine unloaded. Successful contract checks do not establish E4B image accuracy.
The Ready-state contract also passed: the active ID stayed E4B, requests for the other
imported model failed, and disconnect/rebind did not unload or replace the engine.
The all-GPU build also failed a text call while lazily creating the vision executor.
Keeping language on GPU and fixing vision to CPU, then preparing a conversation before
Ready, initialized E4B successfully (18.5 seconds). Two subsequent text requests passed
across a client disconnect (5.8 and 4.5 seconds), with one initialization and the same
active model. No model weights, dependency versions, or context limits were reduced.
An actual order image still triggered low-memory termination with this fixed backend
configuration. E4B image recognition therefore remains unpassed on this device; executor
preparation and text success do not guarantee image allocation fits available RAM.
Switching the service off through its UI released the engine: process PSS fell from
about 3.66 GB to 172 MB. The Off-state Binder contract passed again after shutdown.
Appium import, explicit E4B selection, and service readiness passed in import-only mode.
The runner force-launches its variant, waits for atomic replacement to complete even when
the old ID remains visible, and preserves timeout causes while polling readiness.
Binder contract/text checks were exercised separately; image generation remains unpassed.
