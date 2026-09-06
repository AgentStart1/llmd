#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ANDROID_ROOT_DIR="${ROOT_DIR}/app/src-tauri/gen/android"
ANDROID_APP_DIR="${ANDROID_ROOT_DIR}/app"
ANDROID_LIBRARY_DIR="${ROOT_DIR}/app/src-tauri/android/llmd-android"
MAIN_ACTIVITY_OVERRIDE="${ROOT_DIR}/app/src-tauri/android/app-overrides/MainActivity.kt"
SAMPLE_APP_DIR="${ROOT_DIR}/app/src-tauri/android/llmd-sample"
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

apply_patch_script() {
  local target="$1"
  local patch_script="$2"

  if [[ ! -f "${patch_script}" ]]; then
    echo "Missing Android patch script: ${patch_script}" >&2
    exit 1
  fi

  perl -0pi "${patch_script}" "${target}"
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

if [[ ! -d "${SAMPLE_APP_DIR}" ]]; then
  echo "Missing Android IPC sample app: ${SAMPLE_APP_DIR}" >&2
  exit 1
fi

if ! grep -Eq "include [\"']:llmd-android[\"']" "${SETTINGS_FILE}"; then
  printf '\n__LLMD_ANDROID_SETTINGS__\n' >>"${SETTINGS_FILE}"
  apply_template "${SETTINGS_FILE}" "__LLMD_ANDROID_SETTINGS__" "${PATCHES_DIR}/settings.gradle"
fi

if ! grep -Eq "include [\"']:llmd-sample[\"']" "${SETTINGS_FILE}"; then
  printf '\n__LLMD_SAMPLE_SETTINGS__\n' >>"${SETTINGS_FILE}"
  apply_template "${SETTINGS_FILE}" "__LLMD_SAMPLE_SETTINGS__" "${PATCHES_DIR}/sample-settings.gradle"
fi

if ! grep -Fq 'namespace = "com.storytellerf.llmd"' "${BUILD_FILE}"; then
  apply_patch_script "${BUILD_FILE}" "${PATCHES_DIR}/transformations/namespace.perl"
fi

if ! grep -Fq 'applicationId = "com.storytellerf.llmd"' "${BUILD_FILE}"; then
  apply_patch_script "${BUILD_FILE}" "${PATCHES_DIR}/transformations/application-id.perl"
fi

if ! grep -Fq 'applicationIdSuffix = ".debug"' "${BUILD_FILE}"; then
  apply_patch_script "${BUILD_FILE}" "${PATCHES_DIR}/insertion-points/debug-build-type.perl"
fi

apply_patch_script "${BUILD_FILE}" "${PATCHES_DIR}/transformations/min-sdk.perl"
apply_patch_script "${ROOT_BUILD_FILE}" "${PATCHES_DIR}/transformations/kotlin-gradle-plugin.perl"

if ! grep -Fq 'gradlePluginPortal()' "${ROOT_BUILD_FILE}"; then
  apply_patch_script "${ROOT_BUILD_FILE}" "${PATCHES_DIR}/insertion-points/gradle-plugin-portal.perl"
fi

if ! grep -Fq 'com.starter.easylauncher.gradle.plugin:6.4.1' "${ROOT_BUILD_FILE}"; then
  apply_patch_script "${ROOT_BUILD_FILE}" "${PATCHES_DIR}/insertion-points/easylauncher-classpath.perl"
  apply_template "${ROOT_BUILD_FILE}" "__LLMD_EASYLAUNCHER_CLASSPATH__" "${PATCHES_DIR}/easylauncher-classpath.gradle.kts"
fi

apply_patch_script "${BUILD_FILE}" "${PATCHES_DIR}/transformations/remove-generated-ipc-source-sets.perl"
apply_patch_script "${BUILD_FILE}" "${PATCHES_DIR}/transformations/remove-litertlm-dependency.perl"
apply_patch_script "${BUILD_FILE}" "${PATCHES_DIR}/transformations/remove-datastore-dependency.perl"
apply_patch_script "${BUILD_FILE}" "${PATCHES_DIR}/transformations/rename-daily-build-type.perl"

if ! grep -Fq 'implementation(project(":llmd-android"))' "${BUILD_FILE}"; then
  apply_patch_script "${BUILD_FILE}" "${PATCHES_DIR}/insertion-points/android-dependency.perl"
  apply_template "${BUILD_FILE}" "__LLMD_ANDROID_DEPENDENCY__" "${PATCHES_DIR}/app-dependency.gradle.kts"
fi

if ! grep -Fq 'storyteller_f_sign_key' "${BUILD_FILE}"; then
  apply_patch_script "${BUILD_FILE}" "${PATCHES_DIR}/insertion-points/android-signing.perl"
  apply_template "${BUILD_FILE}" "__LLMD_ANDROID_SIGNING__" "${PATCHES_DIR}/signing.gradle.kts"
fi

if ! grep -Fq 'create("alpha")' "${BUILD_FILE}" && ! grep -Fq 'create("e2e")' "${BUILD_FILE}"; then
  apply_patch_script "${BUILD_FILE}" "${PATCHES_DIR}/insertion-points/custom-build-types.perl"
  apply_template "${BUILD_FILE}" "__LLMD_ANDROID_BUILD_TYPES__" "${PATCHES_DIR}/build-types.gradle.kts"
elif ! grep -Fq 'create("alpha")' "${BUILD_FILE}" || ! grep -Fq 'create("e2e")' "${BUILD_FILE}"; then
  echo "Generated Android project does not contain all llmd custom build types." >&2
  echo "Regenerate the Android project before rerunning this script." >&2
  exit 1
fi

if ! grep -Fq 'apply(plugin = "com.starter.easylauncher")' "${BUILD_FILE}"; then
  apply_patch_script "${BUILD_FILE}" "${PATCHES_DIR}/insertion-points/easylauncher-plugin.perl"
  apply_template "${BUILD_FILE}" "__LLMD_EASYLAUNCHER_PLUGIN__" "${PATCHES_DIR}/easylauncher.gradle.kts"
fi

if ! grep -Fq 'MainActivity$ModelImportBridge' "${PROGUARD_FILE}"; then
  printf '\n__LLMD_MODEL_IMPORT_PROGUARD__\n' >>"${PROGUARD_FILE}"
  apply_template "${PROGUARD_FILE}" "__LLMD_MODEL_IMPORT_PROGUARD__" "${PATCHES_DIR}/proguard-rules.pro"
fi

MAIN_ACTIVITY_TARGET="${ANDROID_APP_DIR}/src/main/java/com/storytellerf/llmd/MainActivity.kt"
mkdir -p "$(dirname "${MAIN_ACTIVITY_TARGET}")"
cp "${MAIN_ACTIVITY_OVERRIDE}" "${MAIN_ACTIVITY_TARGET}"

echo "Synced Tauri Android llmd overrides."
