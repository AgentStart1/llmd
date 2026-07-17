#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODEL_PATH="${GEMMA_MODEL_PATH:-${ROOT_DIR}/models/gemma/gemma-4-E2B-it.litertlm}"
REMOTE_MODEL_PATH="${REMOTE_GEMMA_MODEL_PATH:-/data/local/tmp/llmd/gemma-4-E2B-it.litertlm}"
ANDROID_PACKAGE="${ANDROID_PACKAGE:-com.storytellerf.llmd}"
APP_MODEL_PATH="${APP_GEMMA_MODEL_PATH:-files/models/gemma-4-E2B-it.litertlm}"
DEVICE_SERIAL="${ANDROID_UDID:-${ANDROID_SERIAL:-}}"

if [[ ! -s "${MODEL_PATH}" ]]; then
  echo "Gemma model is missing: ${MODEL_PATH}" >&2
  echo "Run scripts/download-gemma-model.sh or copy it from ../dush/models/gemma first." >&2
  exit 1
fi

adb_cmd=(adb)
if [[ -n "${DEVICE_SERIAL}" ]]; then
  adb_cmd+=(-s "${DEVICE_SERIAL}")
fi

"${adb_cmd[@]}" wait-for-device
"${adb_cmd[@]}" shell "mkdir -p '$(dirname "${REMOTE_MODEL_PATH}")'"
"${adb_cmd[@]}" push "${MODEL_PATH}" "${REMOTE_MODEL_PATH}"
"${adb_cmd[@]}" shell "chmod 755 '$(dirname "${REMOTE_MODEL_PATH}")' && chmod 644 '${REMOTE_MODEL_PATH}'"
"${adb_cmd[@]}" shell "run-as '${ANDROID_PACKAGE}' mkdir -p '$(dirname "${APP_MODEL_PATH}")'"
"${adb_cmd[@]}" shell "run-as '${ANDROID_PACKAGE}' cp '${REMOTE_MODEL_PATH}' '${APP_MODEL_PATH}'"

local_size="$(wc -c < "${MODEL_PATH}" | tr -d '[:space:]')"
remote_size="$("${adb_cmd[@]}" shell "run-as '${ANDROID_PACKAGE}' wc -c < '${APP_MODEL_PATH}'" | tr -d '\r[:space:]')"

if [[ "${local_size}" != "${remote_size}" ]]; then
  echo "Model size mismatch after adb push: local=${local_size}, remote=${remote_size}" >&2
  exit 1
fi

echo "Prepared model for ${ANDROID_PACKAGE}: ${APP_MODEL_PATH} (${remote_size} bytes)"
