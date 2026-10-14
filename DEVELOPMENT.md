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

## Large-heap experiment, 2026-10-14

On the same 6 GB Android 15 device, the runtime reports a 256 MB ordinary managed-heap
limit and a 512 MB large-heap limit. An experimental Debug APK set `android:largeHeap`
and PackageManager confirmed `LARGE_HEAP` on the installed app. Model, precision,
backend configuration, dependencies, and context limits were unchanged.

E4B initialized in 16.8 seconds; process PSS was about 3.63 GB while the Dalvik heap
was about 4 MB. The synthetic 900×600 balance image reached native visual preprocessing
(2340 patches). The low-memory killer then terminated LLMD to reclaim 3,769,700 KB RSS.
No recognition result was produced. The waiting Summer test was stopped after the
process-death evidence was captured and is not counted as a pass.

The ordinary-heap baseline also exited after its WebView renderer was killed, so this
run does not establish a paired image-latency comparison. The enabled run independently
establishes that large heap does not prevent the observed E4B image memory failure.
The experimental flag was removed and the ordinary-heap Debug APK restored.

## ZRAM experiment, 2026-10-14

The same device initially had no swap and an uninitialized ZRAM device. Temporarily
enabling rooted ADB allowed configuring 3 GiB of ZRAM with the existing LZO compressor
and unchanged swappiness (100). `/proc/swaps` confirmed activation. No startup files,
model weights, precision, dependency versions, context limits, or backend choices changed;
the installed Debug APK did not have `LARGE_HEAP` enabled.

E4B reached Ready in 19.0 seconds. Before inference, roughly 349 MiB of system pages
were swapped out. The same synthetic 900×600 image reached native preprocessing at
2340 patches, but the low-memory killer terminated the WebView renderer and then LLMD
at 4,027,912 KB RSS with 52,640 KB of process swap. Android exit history confirmed
`LOW_MEMORY`. No recognition result was produced; the waiting test was stopped and is
not counted as passing. After the failure, swap held about 600 MiB of logical pages;
ZRAM's observed peak physical allocation was about 175 MiB.

Rooted debugging was restored to disabled. ZRAM remains enabled for the current boot
only, without persistent system changes. This configuration did not make E4B image
inference fit the device's memory budget; no fallback model was selected.

The follow-up test increased ZRAM to 6 GiB, retaining LZO and swappiness 100. E4B
initialized in 16.6 seconds, and the identical image again reached 2340-patch native
preprocessing. The low-memory killer terminated LLMD at 3,996,928 KB RSS with
99,840 KB process swap; exit history again reported `LOW_MEMORY`. A system swap
sample held approximately 529 MiB, far below the configured capacity. No recognition
result was produced and the waiting test was stopped. Increasing swap capacity alone
did not prevent the failure. Rooted debugging was disabled again; 6 GiB ZRAM remains
enabled only for the current boot. No model, precision, backend, or fallback changed.
