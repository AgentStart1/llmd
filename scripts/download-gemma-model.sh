#!/usr/bin/env bash
set -euo pipefail

REPO_ID="${GEMMA_REPO_ID:?Set GEMMA_REPO_ID to the model repository}"
MODEL_FILE="${GEMMA_MODEL_FILE:?Set GEMMA_MODEL_FILE to the exact .litertlm filename}"
OUTPUT_DIR="${GEMMA_MODEL_DIR:-models/gemma}"
OUTPUT_PATH="${OUTPUT_DIR}/${MODEL_FILE}"
URL="https://huggingface.co/${REPO_ID}/resolve/main/${MODEL_FILE}"

mkdir -p "${OUTPUT_DIR}"

if [[ -s "${OUTPUT_PATH}" ]]; then
  echo "Model already exists: ${OUTPUT_PATH}"
  exit 0
fi

headers=()
if [[ -n "${HF_TOKEN:-}" ]]; then
  headers=(-H "Authorization: Bearer ${HF_TOKEN}")
fi

tmp_path="${OUTPUT_PATH}.part"
echo "Downloading ${REPO_ID}/${MODEL_FILE}"
echo "Target: ${OUTPUT_PATH}"

curl -L --fail --continue-at - "${headers[@]}" "${URL}" -o "${tmp_path}"
mv "${tmp_path}" "${OUTPUT_PATH}"

echo "Downloaded: ${OUTPUT_PATH}"
