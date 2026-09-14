#!/usr/bin/env bash
set -euo pipefail

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
cli_bin="${LLMD_CLI_BIN:-${root_dir}/target/debug/llmd}"
if [[ ! -f "${cli_bin}" && -f "${cli_bin}.exe" ]]; then
  cli_bin="${cli_bin}.exe"
fi

if [[ ! -x "${cli_bin}" ]]; then
  echo "CLI binary is missing or not executable: ${cli_bin}" >&2
  echo "Build it with: cargo build -p llmd --locked" >&2
  exit 1
fi

if ! command -v curl >/dev/null 2>&1; then
  echo "Required command is unavailable: curl" >&2
  exit 1
fi

python_command=""
for candidate in python3 python; do
  if command -v "${candidate}" >/dev/null 2>&1 && "${candidate}" -c "import json, socket" >/dev/null 2>&1; then
    python_command="${candidate}"
    break
  fi
done
if [[ -z "${python_command}" ]]; then
  echo "Python 3 is required for the CLI end-to-end test" >&2
  exit 1
fi

temp_root="${TMPDIR:-/tmp}"
mkdir -p "${temp_root}"
temp_root="$(cd "${temp_root}" && pwd -P)"
test_dir="$(mktemp -d "${temp_root}/llmd-cli-e2e.XXXXXX")"
model_dir="${test_dir}/models"
server_log="${test_dir}/server.log"
server_pid=""
mkdir -p "${model_dir}"

cleanup() {
  if [[ -n "${server_pid}" ]] && kill -0 "${server_pid}" 2>/dev/null; then
    kill "${server_pid}" 2>/dev/null || true
    wait "${server_pid}" 2>/dev/null || true
  fi

  case "${test_dir}" in
    "${temp_root}"/llmd-cli-e2e.*) rm -rf -- "${test_dir}" ;;
    *) echo "Refusing to remove unexpected test directory: ${test_dir}" >&2 ;;
  esac
}
trap cleanup EXIT

models_output="$(LLMD_MODEL_DIR="${model_dir}" "${cli_bin}" models)"
if [[ -n "${models_output}" ]]; then
  echo "Expected an empty model list, got: ${models_output}" >&2
  exit 1
fi

if LLMD_MODEL_DIR="${model_dir}" "${cli_bin}" chat "hello" --model missing \
  >"${test_dir}/chat.stdout" 2>"${test_dir}/chat.stderr"; then
  echo "Expected chat with a missing model to fail" >&2
  exit 1
fi
if ! grep -F "model not found: missing" "${test_dir}/chat.stderr" >/dev/null; then
  cat "${test_dir}/chat.stderr" >&2
  echo "Missing-model error was not reported" >&2
  exit 1
fi

port="${LLMD_CLI_E2E_PORT:-$("${python_command}" -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()')}"
LLMD_MODEL_DIR="${model_dir}" "${cli_bin}" serve \
  --host 127.0.0.1 --port "${port}" --pool-size 1 >"${server_log}" 2>&1 &
server_pid="$!"

health_json=""
for _ in {1..60}; do
  if ! kill -0 "${server_pid}" 2>/dev/null; then
    cat "${server_log}" >&2
    echo "CLI server exited before becoming healthy" >&2
    exit 1
  fi
  if health_json="$(curl --fail --silent --show-error "http://127.0.0.1:${port}/health" 2>/dev/null)"; then
    break
  fi
  sleep 0.5
done

if [[ -z "${health_json}" ]]; then
  cat "${server_log}" >&2
  echo "CLI server did not become healthy" >&2
  exit 1
fi

models_json="$(curl --fail --silent --show-error "http://127.0.0.1:${port}/v1/models")"
HEALTH_JSON="${health_json}" MODELS_JSON="${models_json}" "${python_command}" - <<'PY'
import json
import os

health = json.loads(os.environ["HEALTH_JSON"])
models = json.loads(os.environ["MODELS_JSON"])
assert health == {"status": "ok"}, health
assert models.get("object") == "list", models
assert models.get("data") == [], models
PY

echo "CLI end-to-end test passed"
