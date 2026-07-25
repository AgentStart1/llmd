#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ANDROID_ROOT_DIR="${ROOT_DIR}/app/src-tauri/gen/android"
ANDROID_APP_DIR="${ANDROID_ROOT_DIR}/app"
ANDROID_LIBRARY_DIR="${ROOT_DIR}/app/src-tauri/android/llmd-android"
MAIN_ACTIVITY_OVERRIDE="${ROOT_DIR}/app/src-tauri/android/app-overrides/MainActivity.kt"
PATCHES_DIR="${ROOT_DIR}/app/src-tauri/android/patches"
SETTINGS_FILE="${ANDROID_ROOT_DIR}/settings.gradle"
ROOT_BUILD_FILE="${ANDROID_ROOT_DIR}/build.gradle.kts"
BUILD_FILE="${ANDROID_APP_DIR}/build.gradle.kts"
MANIFEST_FILE="${ANDROID_APP_DIR}/src/main/AndroidManifest.xml"
PROGUARD_FILE="${ANDROID_APP_DIR}/proguard-rules.pro"

need_file() {
  local path="$1"
  if [[ ! -f "${path}" ]]; then
    echo "Missing generated Android file: ${path}" >&2
    echo "Run the Tauri Android generation step first, then rerun this script." >&2
    exit 1
  fi
}

apply_template() {
  local target="$1"
  local placeholder="$2"
  local template="$3"

  if ! grep -Fq "${placeholder}" "${target}"; then
    echo "Missing Android patch placeholder ${placeholder} in ${target}" >&2
    exit 1
  fi

  sed -i "\\|${placeholder}|r ${template}" "${target}"
  sed -i "\\|${placeholder}|d" "${target}"
}

need_file "${BUILD_FILE}"
need_file "${SETTINGS_FILE}"
need_file "${ROOT_BUILD_FILE}"
need_file "${MANIFEST_FILE}"
need_file "${PROGUARD_FILE}"
need_file "${MAIN_ACTIVITY_OVERRIDE}"

if [[ ! -d "${ANDROID_LIBRARY_DIR}" ]]; then
  echo "Missing Android library: ${ANDROID_LIBRARY_DIR}" >&2
  exit 1
fi

if ! grep -Eq "include [\"']:llmd-android[\"']" "${SETTINGS_FILE}"; then
  printf '\n__LLMD_ANDROID_SETTINGS__\n' >>"${SETTINGS_FILE}"
  apply_template "${SETTINGS_FILE}" "__LLMD_ANDROID_SETTINGS__" "${PATCHES_DIR}/settings.gradle"
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
  perl -0pi -e 's#(\ndependencies \{\n)#$1__LLMD_ANDROID_DEPENDENCY__\n#' "${BUILD_FILE}"
  apply_template "${BUILD_FILE}" "__LLMD_ANDROID_DEPENDENCY__" "${PATCHES_DIR}/app-dependency.gradle.kts"
fi

if ! grep -Fq 'storyteller_f_sign_key' "${BUILD_FILE}"; then
  perl -0pi -e 's#(\n    buildTypes \{)#\n__LLMD_ANDROID_SIGNING__$1#' "${BUILD_FILE}"
  apply_template "${BUILD_FILE}" "__LLMD_ANDROID_SIGNING__" "${PATCHES_DIR}/signing.gradle.kts"
fi

if ! grep -Fq 'create("daily")' "${BUILD_FILE}" && ! grep -Fq 'create("e2e")' "${BUILD_FILE}"; then
  perl -0pi -e 's#(\n    \}\n    kotlinOptions \{)#\n__LLMD_ANDROID_BUILD_TYPES__$1#' "${BUILD_FILE}"
  apply_template "${BUILD_FILE}" "__LLMD_ANDROID_BUILD_TYPES__" "${PATCHES_DIR}/build-types.gradle.kts"
elif ! grep -Fq 'create("daily")' "${BUILD_FILE}" || ! grep -Fq 'create("e2e")' "${BUILD_FILE}"; then
  echo "Generated Android project contains only one llmd custom build type." >&2
  echo "Regenerate the Android project before rerunning this script." >&2
  exit 1
fi

if ! grep -Fq 'MainActivity$ModelImportBridge' "${PROGUARD_FILE}"; then
  printf '\n__LLMD_MODEL_IMPORT_PROGUARD__\n' >>"${PROGUARD_FILE}"
  apply_template "${PROGUARD_FILE}" "__LLMD_MODEL_IMPORT_PROGUARD__" "${PATCHES_DIR}/proguard-rules.pro"
fi

MAIN_ACTIVITY_TARGET="${ANDROID_APP_DIR}/src/main/java/com/storytellerf/llmd/MainActivity.kt"
mkdir -p "$(dirname "${MAIN_ACTIVITY_TARGET}")"
cp "${MAIN_ACTIVITY_OVERRIDE}" "${MAIN_ACTIVITY_TARGET}"

echo "Synced Tauri Android llmd overrides."
