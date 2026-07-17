#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ANDROID_APP_DIR="${ROOT_DIR}/app/src-tauri/gen/android/app"
BUILD_FILE="${ANDROID_APP_DIR}/build.gradle.kts"
MANIFEST_FILE="${ANDROID_APP_DIR}/src/main/AndroidManifest.xml"

need_file() {
  local path="$1"
  if [[ ! -f "${path}" ]]; then
    echo "Missing generated Android file: ${path}" >&2
    echo "Run the Tauri Android generation step first, then rerun this script." >&2
    exit 1
  fi
}

need_file "${BUILD_FILE}"
need_file "${MANIFEST_FILE}"

if ! grep -Fq 'aidl = true' "${BUILD_FILE}"; then
  perl -0pi -e 's/buildFeatures \{\n/buildFeatures {\n        aidl = true\n/' "${BUILD_FILE}"
fi

if ! grep -Fq 'android/llmd-ipc/src/main/java' "${BUILD_FILE}"; then
  perl -0pi -e 's/(\n\s*buildFeatures \{\n(?:.|\n)*?\n\s*\}\n)/$1    sourceSets {\n        getByName("main") {\n            java.srcDir("..\/..\/..\/android\/llmd-ipc\/src\/main\/java")\n            aidl.srcDir("..\/..\/..\/android\/llmd-ipc\/src\/main\/aidl")\n        }\n    }\n/s' "${BUILD_FILE}"
else
  perl -0pi -e 's#"\.\./\.\./android/llmd-ipc/#"../../../android/llmd-ipc/#g' "${BUILD_FILE}"
fi

if ! grep -Fq 'namespace = "com.storytellerf.llmd"' "${BUILD_FILE}"; then
  perl -0pi -e 's/namespace = "[^"]+"/namespace = "com.storytellerf.llmd"/' "${BUILD_FILE}"
fi

if ! grep -Fq 'applicationId = "com.storytellerf.llmd"' "${BUILD_FILE}"; then
  perl -0pi -e 's/applicationId = "[^"]+"/applicationId = "com.storytellerf.llmd"/' "${BUILD_FILE}"
fi

if ! grep -Fq 'androidx.datastore:datastore-preferences' "${BUILD_FILE}"; then
  perl -0pi -e 's#(\n\s*implementation\("org\.jetbrains\.kotlinx:kotlinx-coroutines-android:[^"]+"\))#$1\n    implementation("androidx.datastore:datastore-preferences:1.2.1")#' "${BUILD_FILE}"
fi

if ! grep -Fq 'LlmdIpcService' "${MANIFEST_FILE}"; then
  perl -0pi -e 's#(\n\s*</application>)#\n\n        <service\n            android:name=".LlmdIpcService"\n            android:exported="true">\n            <intent-filter>\n                <action android:name="com.storytellerf.llmd.action.BIND_IPC" />\n            </intent-filter>\n        </service>$1#' "${MANIFEST_FILE}"
fi

if ! grep -Fq 'com.storytellerf.llmd.action.BIND_IPC' "${MANIFEST_FILE}"; then
  perl -0pi -e 's/[-_A-Za-z0-9.]+\.action\.BIND_IPC/com.storytellerf.llmd.action.BIND_IPC/g' "${MANIFEST_FILE}"
fi

echo "Synced Tauri Android llmd overrides."
