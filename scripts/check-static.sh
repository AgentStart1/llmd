#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ANDROID_TARGET="${ANDROID_TARGET:-aarch64-linux-android}"
ANDROID_API_LEVEL="${ANDROID_API_LEVEL:-24}"

run_host=false
run_android_kotlin=false
run_android_rust=false

usage() {
  cat <<'EOF'
Usage: scripts/check-static.sh [--host] [--android] [--android-kotlin] [--android-rust]

Options:
  --host            Run host static checks. This is the default.
  --android         Run Android Kotlin/AIDL and Android Rust target checks.
  --android-kotlin  Run only Android Kotlin/AIDL compilation checks.
  --android-rust    Run only Android Rust target check/clippy.
  --all             Run host and Android checks.
  -h, --help        Show this help.

Install frontend dependencies first with:
  npm --prefix app ci
EOF
}

if [[ "$#" -eq 0 ]]; then
  run_host=true
fi

while [[ "$#" -gt 0 ]]; do
  case "$1" in
    --host)
      run_host=true
      ;;
    --android)
      run_android_kotlin=true
      run_android_rust=true
      ;;
    --android-kotlin)
      run_android_kotlin=true
      ;;
    --android-rust)
      run_android_rust=true
      ;;
    --all)
      run_host=true
      run_android_kotlin=true
      run_android_rust=true
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
  shift
done

step() {
  echo
  echo "==> $*"
}

run_host_checks() {
  step "Checking shell script syntax"
  bash -n "${ROOT_DIR}"/scripts/*.sh

  step "Checking Rust formatting"
  (cd "${ROOT_DIR}" && cargo fmt --all --check)

  step "Running workspace clippy"
  (cd "${ROOT_DIR}" && cargo clippy --workspace --locked --all-targets -- -D warnings)

  step "Checking Tauri Rust crate"
  (cd "${ROOT_DIR}" && cargo check --manifest-path app/src-tauri/Cargo.toml --locked)

  step "Running Tauri Rust clippy"
  (cd "${ROOT_DIR}" && cargo clippy --manifest-path app/src-tauri/Cargo.toml --locked --all-targets -- -D warnings)

  step "Building frontend"
  (cd "${ROOT_DIR}" && npm --prefix app run build)
}

android_ndk_home() {
  if [[ -n "${ANDROID_NDK_HOME:-}" ]]; then
    echo "${ANDROID_NDK_HOME}"
  elif [[ -n "${ANDROID_NDK:-}" ]]; then
    echo "${ANDROID_NDK}"
  elif [[ -n "${ANDROID_NDK_ROOT:-}" ]]; then
    echo "${ANDROID_NDK_ROOT}"
  elif [[ -n "${ANDROID_HOME:-}" && -d "${ANDROID_HOME}/ndk" ]]; then
    find "${ANDROID_HOME}/ndk" -mindepth 1 -maxdepth 1 -type d | sort -V | tail -n 1
  elif [[ -n "${ANDROID_SDK_ROOT:-}" && -d "${ANDROID_SDK_ROOT}/ndk" ]]; then
    find "${ANDROID_SDK_ROOT}/ndk" -mindepth 1 -maxdepth 1 -type d | sort -V | tail -n 1
  elif [[ -d "/usr/local/lib/android/sdk/ndk" ]]; then
    find "/usr/local/lib/android/sdk/ndk" -mindepth 1 -maxdepth 1 -type d | sort -V | tail -n 1
  elif [[ -d "${HOME}/Android/Sdk/ndk" ]]; then
    find "${HOME}/Android/Sdk/ndk" -mindepth 1 -maxdepth 1 -type d | sort -V | tail -n 1
  fi
}

configure_android_rust_toolchain() {
  local ndk_home toolchain
  ndk_home="$(android_ndk_home)"
  if [[ -z "${ndk_home}" ]]; then
    echo "ANDROID_NDK_HOME, ANDROID_NDK, ANDROID_NDK_ROOT, or an SDK ndk directory must be available for Android Rust checks." >&2
    exit 1
  fi

  toolchain="${ndk_home}/toolchains/llvm/prebuilt/linux-x86_64/bin"

  if [[ ! -x "${toolchain}/aarch64-linux-android${ANDROID_API_LEVEL}-clang" ]]; then
    echo "Missing Android clang at ${toolchain}/aarch64-linux-android${ANDROID_API_LEVEL}-clang" >&2
    exit 1
  fi

  export CC_aarch64_linux_android="${CC_aarch64_linux_android:-${toolchain}/aarch64-linux-android${ANDROID_API_LEVEL}-clang}"
  export CXX_aarch64_linux_android="${CXX_aarch64_linux_android:-${toolchain}/aarch64-linux-android${ANDROID_API_LEVEL}-clang++}"
  export AR_aarch64_linux_android="${AR_aarch64_linux_android:-${toolchain}/llvm-ar}"
}

run_android_kotlin_checks() {
  step "Syncing Tauri Android overrides"
  "${ROOT_DIR}/scripts/sync-tauri-android-overrides.sh"

  step "Compiling Android Kotlin and AIDL"
  (cd "${ROOT_DIR}/app/src-tauri/gen/android" && ./gradlew :app:compileArm64DebugKotlin --no-daemon)
}

run_android_rust_checks() {
  configure_android_rust_toolchain

  step "Checking Tauri Rust crate for ${ANDROID_TARGET}"
  (cd "${ROOT_DIR}" && cargo check --manifest-path app/src-tauri/Cargo.toml --locked --target "${ANDROID_TARGET}")

  step "Running Tauri Rust clippy for ${ANDROID_TARGET}"
  (cd "${ROOT_DIR}" && cargo clippy --manifest-path app/src-tauri/Cargo.toml --locked --target "${ANDROID_TARGET}" -- -D warnings)
}

if [[ "${run_host}" == true ]]; then
  run_host_checks
fi

if [[ "${run_android_kotlin}" == true ]]; then
  run_android_kotlin_checks
fi

if [[ "${run_android_rust}" == true ]]; then
  run_android_rust_checks
fi
