#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEVICE_SERIAL="${ANDROID_UDID:-${ANDROID_SERIAL:-}}"
REMOTE_PORT="${LLMD_ANDROID_REMOTE_PORT:-11435}"
LOCAL_PORT="${LLMD_ANDROID_LOCAL_PORT:-11435}"

adb_cmd=(adb)
if [[ -n "${DEVICE_SERIAL}" ]]; then
  adb_cmd+=(-s "${DEVICE_SERIAL}")
fi

"${ROOT_DIR}/scripts/prepare-android-model.sh"
"${adb_cmd[@]}" forward --remove "tcp:${LOCAL_PORT}" >/dev/null 2>&1 || true
"${adb_cmd[@]}" forward "tcp:${LOCAL_PORT}" "tcp:${REMOTE_PORT}"

LLMD_OPENAI_BASE_URL="http://127.0.0.1:${LOCAL_PORT}" \
  "${ROOT_DIR}/scripts/test-openai-api.sh"
