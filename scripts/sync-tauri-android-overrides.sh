#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ANDROID_ROOT_DIR="${ROOT_DIR}/app/src-tauri/gen/android"
ANDROID_APP_DIR="${ANDROID_ROOT_DIR}/app"
ANDROID_LIBRARY_DIR="${ROOT_DIR}/app/src-tauri/android/llmd-android"
MAIN_ACTIVITY_OVERRIDE="${ROOT_DIR}/app/src-tauri/android/app-overrides/MainActivity.kt"
SETTINGS_FILE="${ANDROID_ROOT_DIR}/settings.gradle"
ROOT_BUILD_FILE="${ANDROID_ROOT_DIR}/build.gradle.kts"
BUILD_FILE="${ANDROID_APP_DIR}/build.gradle.kts"
MANIFEST_FILE="${ANDROID_APP_DIR}/src/main/AndroidManifest.xml"
PROGUARD_FILE="${ANDROID_APP_DIR}/proguard-rules.pro"
RUST_PLUGIN_FILE="$(find "${ANDROID_ROOT_DIR}/buildSrc" -type f -name RustPlugin.kt -print -quit)"

need_file() {
  local path="$1"
  if [[ ! -f "${path}" ]]; then
    echo "Missing generated Android file: ${path}" >&2
    echo "Run the Tauri Android generation step first, then rerun this script." >&2
    exit 1
  fi
}

need_file "${BUILD_FILE}"
need_file "${SETTINGS_FILE}"
need_file "${ROOT_BUILD_FILE}"
need_file "${MANIFEST_FILE}"
need_file "${PROGUARD_FILE}"
need_file "${RUST_PLUGIN_FILE}"
need_file "${MAIN_ACTIVITY_OVERRIDE}"

if [[ ! -d "${ANDROID_LIBRARY_DIR}" ]]; then
  echo "Missing Android library: ${ANDROID_LIBRARY_DIR}" >&2
  exit 1
fi

if ! grep -Eq "include [\"']:llmd-android[\"']" "${SETTINGS_FILE}"; then
  cat >>"${SETTINGS_FILE}" <<'EOF'
include ':llmd-android'
project(':llmd-android').projectDir = new File(rootDir, '../../android/llmd-android')
EOF
fi

if ! grep -Fq 'namespace = "com.storytellerf.llmd"' "${BUILD_FILE}"; then
  perl -0pi -e 's/namespace = "[^"]+"/namespace = "com.storytellerf.llmd"/' "${BUILD_FILE}"
fi

if ! grep -Fq 'applicationId = "com.storytellerf.llmd"' "${BUILD_FILE}"; then
  perl -0pi -e 's/applicationId = "[^"]+"/applicationId = "com.storytellerf.llmd"/' "${BUILD_FILE}"
fi

perl -0pi -e 's/minSdk = \d+/minSdk = 35/' "${BUILD_FILE}"
perl -0pi -e 's#org\.jetbrains\.kotlin:kotlin-gradle-plugin:[^"]+#org.jetbrains.kotlin:kotlin-gradle-plugin:2.2.21#' "${ROOT_BUILD_FILE}"

perl -0pi -e 's#\n\s*sourceSets\s*\{\s*getByName\("main"\)\s*\{\s*java\.srcDir\("../../../android/llmd-ipc/src/main/java"\)\s*aidl\.srcDir\("../../../android/llmd-ipc/src/main/aidl"\)\s*\}\s*\}\n#\n#s' "${BUILD_FILE}"
perl -0pi -e 's#\n\s*implementation\("com\.google\.ai\.edge\.litertlm:litertlm-android:[^"]+"\)##' "${BUILD_FILE}"
perl -0pi -e 's#\n\s*implementation\("androidx\.datastore:datastore-preferences:[^"]+"\)##' "${BUILD_FILE}"

if ! grep -Fq 'implementation(project(":llmd-android"))' "${BUILD_FILE}"; then
  perl -0pi -e 's#(\ndependencies \{\n)#$1    implementation(project(":llmd-android"))\n#' "${BUILD_FILE}"
fi

if ! grep -Fq 'create("e2e")' "${BUILD_FILE}"; then
  perl -0pi -e 's#(\n\s*create\("daily"\) \{\n\s*initWith\(getByName\("release"\)\)\n\s*applicationIdSuffix = "\.daily"\n\s*versionNameSuffix = "-daily"\n\s*matchingFallbacks \+= listOf\("release"\)\n\s*\}\n)#$1        create("e2e") {\n            initWith(getByName("release"))\n            manifestPlaceholders["usesCleartextTraffic"] = "true"\n            isDebuggable = false\n            isJniDebuggable = false\n            signingConfig = signingConfigs.getByName("debug")\n            matchingFallbacks += listOf("release")\n            packaging {\n                jniLibs.keepDebugSymbols.add("*/arm64-v8a/*.so")\n                jniLibs.keepDebugSymbols.add("*/armeabi-v7a/*.so")\n                jniLibs.keepDebugSymbols.add("*/x86/*.so")\n                jniLibs.keepDebugSymbols.add("*/x86_64/*.so")\n            }\n        }\n#s' "${BUILD_FILE}"
fi

if ! grep -Fq 'MainActivity$ModelImportBridge' "${PROGUARD_FILE}"; then
  cat >>"${PROGUARD_FILE}" <<'EOF'

# Android WebView JavaScript bridge methods are invoked by name from the bundled UI.
-keepclassmembers class com.storytellerf.llmd.MainActivity$ModelImportBridge {
    @android.webkit.JavascriptInterface <methods>;
}
EOF
fi

MAIN_ACTIVITY_TARGET="${ANDROID_APP_DIR}/src/main/java/com/storytellerf/llmd/MainActivity.kt"
mkdir -p "$(dirname "${MAIN_ACTIVITY_TARGET}")"
cp "${MAIN_ACTIVITY_OVERRIDE}" "${MAIN_ACTIVITY_TARGET}"

if ! grep -Fq '"e2e" to true' "${RUST_PLUGIN_FILE}"; then
  perl -0pi -e 's#for \(profile in listOf\("debug", "release"\)\) \{#val profiles = mapOf(\n                "debug" to false,\n                "release" to true,\n                "daily" to true,\n                "e2e" to true,\n            )\n            for ((profile, isRelease) in profiles) {#' "${RUST_PLUGIN_FILE}"
  perl -0pi -e 's#release = profile == "release"#release = isRelease#' "${RUST_PLUGIN_FILE}"
fi

perl -0pi -e 's#tasks\["mergeUniversal\$\{profileCapitalized\}JniLibFolders"\]\.dependsOn\(buildTask\)#tasks.findByName("mergeUniversal\${profileCapitalized}JniLibFolders")?.dependsOn(buildTask)#' "${RUST_PLUGIN_FILE}"
perl -0pi -e 's#tasks\["merge\$targetArchCapitalized\$\{profileCapitalized\}JniLibFolders"\]\.dependsOn\(\n\s*targetBuildTask\n\s*\)#tasks.findByName("merge\$targetArchCapitalized\${profileCapitalized}JniLibFolders")?.dependsOn(targetBuildTask)#' "${RUST_PLUGIN_FILE}"

echo "Synced Tauri Android llmd overrides."
