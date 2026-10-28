#!/usr/bin/env bash
set -euo pipefail

BUILD_TYPE="${LLMD_ANDROID_BUILD_TYPE:-debug}"

while [[ "$#" -gt 0 ]]; do
  case "$1" in
    --debug)
      BUILD_TYPE="debug"
      ;;
    --e2e)
      BUILD_TYPE="e2e"
      ;;
    --release)
      BUILD_TYPE="release"
      ;;
    --alpha)
      BUILD_TYPE="alpha"
      ;;
    -h | --help)
      cat <<'EOF'
Usage: scripts/test-android-appium.sh [--debug|--release|--alpha|--e2e]

Options:
  --debug  Build and install the normal debug APK. This is the default.
  --release Build and install the release APK and IPC sample.
  --alpha  Build and install the alpha APK and IPC sample.
  --e2e    Build and install the minified debuggable APK for obfuscation checks.

Environment:
  LLMD_ANDROID_BUILD_TYPE=debug|release|alpha|e2e
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
  debug | release | alpha | e2e) ;;
  *)
    echo "LLMD_ANDROID_BUILD_TYPE must be debug, release, alpha, or e2e, got: ${BUILD_TYPE}" >&2
    exit 2
    ;;
esac

exec env LLMD_ANDROID_BUILD_TYPE="${BUILD_TYPE}" cargo run -p llmd-android-e2e
