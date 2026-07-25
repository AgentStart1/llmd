#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LOCK_SCRIPT="/home/mint-dev/.codex/skills/android-appium-device-lock/scripts/adb-device-lock.sh"
DEVICE_SERIAL="${ANDROID_UDID:-${ANDROID_SERIAL:-}}"
BUILD_TYPE="${LLMD_ANDROID_BUILD_TYPE:-debug}"

while [[ "$#" -gt 0 ]]; do
  case "$1" in
    --debug)
      BUILD_TYPE="debug"
      ;;
    --e2e)
      BUILD_TYPE="e2e"
      ;;
    -h | --help)
      cat <<'EOF'
Usage: scripts/test-android-appium.sh [--debug|--e2e]

Options:
  --debug  Build and install the normal debug APK. This is the default.
  --e2e    Build and install the minified debuggable APK for obfuscation checks.

Environment:
  LLMD_ANDROID_BUILD_TYPE=debug|e2e
EOF
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      exit 2
      ;;
  esac
  shift
done

case "${BUILD_TYPE}" in
  debug | e2e) ;;
  *)
    echo "LLMD_ANDROID_BUILD_TYPE must be debug or e2e, got: ${BUILD_TYPE}" >&2
    exit 2
    ;;
esac

if [[ "${LLMD_ANDROID_E2E_LOCK_HELD:-}" == "1" ]]; then
  exec env LLMD_ANDROID_BUILD_TYPE="${BUILD_TYPE}" cargo run -p llmd-android-e2e
fi

lock_args=(
  run
  --project-dir "${ROOT_DIR}"
  --test-name "llmd-android-appium-e2e"
  --max-timeout-seconds "${LLMD_ANDROID_E2E_LOCK_MAX_SECONDS:-2400}"
  --wait-timeout-seconds "${LLMD_ANDROID_E2E_LOCK_WAIT_SECONDS:-3600}"
)
if [[ -n "${DEVICE_SERIAL}" ]]; then
  lock_args+=(--serial "${DEVICE_SERIAL}")
fi
lock_args+=(--)

exec "${LOCK_SCRIPT}" "${lock_args[@]}" env LLMD_ANDROID_E2E_LOCK_HELD=1 LLMD_ANDROID_BUILD_TYPE="${BUILD_TYPE}" "$0"
