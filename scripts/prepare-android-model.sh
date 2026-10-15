#!/usr/bin/env bash
set -euo pipefail

MODEL_PATH="${GEMMA_MODEL_PATH:?Set GEMMA_MODEL_PATH to the model file to prepare}"
MODEL_FILE="$(basename "${MODEL_PATH}")"
REMOTE_MODEL_PATH="${REMOTE_GEMMA_MODEL_PATH:-/data/local/tmp/llmd/${MODEL_FILE}}"
ANDROID_PACKAGE="${ANDROID_PACKAGE:-com.storytellerf.llmd.debug}"
APP_MODEL_PATH="${APP_GEMMA_MODEL_PATH:-files/models/${MODEL_FILE}}"
DEVICE_SERIAL="${ANDROID_UDID:-${ANDROID_SERIAL:-}}"

if [[ ! -s "${MODEL_PATH}" ]]; then
  echo "Gemma model is missing: ${MODEL_PATH}" >&2
  echo "Download the chosen model first, then set GEMMA_MODEL_PATH to its file." >&2
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
