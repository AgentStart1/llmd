#!/usr/bin/env bash
set -euo pipefail

BASE_URL="${LLMD_OPENAI_BASE_URL:-http://127.0.0.1:11435}"
MODEL="${LLMD_TEST_MODEL:-gemma-4-E2B-it}"
PROMPT="${LLMD_TEST_PROMPT:-Reply with one short sentence.}"

need() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "Missing required command: $1" >&2
    exit 1
  }
}

need curl
need jq

echo "Testing OpenAI-compatible API at ${BASE_URL}"

health="$(curl -fsS "${BASE_URL}/health")"
echo "${health}" | jq -e '.status == "ok"' >/dev/null

models="$(curl -fsS "${BASE_URL}/v1/models")"
echo "${models}" | jq -e --arg model "${MODEL}" '.data | any(.id == $model)' >/dev/null

chat_payload="$(jq -nc --arg model "${MODEL}" --arg prompt "${PROMPT}" '{
  model: $model,
  messages: [{role: "user", content: $prompt}],
  stream: false
}')"

chat="$(curl -fsS "${BASE_URL}/v1/chat/completions" \
  -H "Content-Type: application/json" \
  -d "${chat_payload}")"

echo "${chat}" | jq -e '.choices[0].message.content | length > 0' >/dev/null
echo "OpenAI-compatible API test passed."
