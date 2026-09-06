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
  local sdk_root

  if [[ -n "${ANDROID_NDK_HOME:-}" ]]; then
    normalize_shell_path "${ANDROID_NDK_HOME}"
    return
  fi
  if [[ -n "${ANDROID_NDK:-}" ]]; then
    normalize_shell_path "${ANDROID_NDK}"
    return
  fi
  if [[ -n "${ANDROID_NDK_ROOT:-}" ]]; then
    normalize_shell_path "${ANDROID_NDK_ROOT}"
    return
  fi
  if [[ -n "${ANDROID_HOME:-}" ]]; then
    sdk_root="$(normalize_shell_path "${ANDROID_HOME}")"
    if [[ -d "${sdk_root}/ndk" ]]; then
      find "${sdk_root}/ndk" -mindepth 1 -maxdepth 1 -type d | sort -V | tail -n 1
      return
    fi
  fi
  if [[ -n "${ANDROID_SDK_ROOT:-}" ]]; then
    sdk_root="$(normalize_shell_path "${ANDROID_SDK_ROOT}")"
    if [[ -d "${sdk_root}/ndk" ]]; then
      find "${sdk_root}/ndk" -mindepth 1 -maxdepth 1 -type d | sort -V | tail -n 1
      return
    fi
  fi
  if [[ -d "/usr/local/lib/android/sdk/ndk" ]]; then
    find "/usr/local/lib/android/sdk/ndk" -mindepth 1 -maxdepth 1 -type d | sort -V | tail -n 1
    return
  fi
  if [[ -d "${HOME}/Android/Sdk/ndk" ]]; then
    find "${HOME}/Android/Sdk/ndk" -mindepth 1 -maxdepth 1 -type d | sort -V | tail -n 1
  fi
}

android_host_family() {
  case "$(uname -s)" in
    Linux*) echo "linux" ;;
    Darwin*) echo "darwin" ;;
    MINGW* | MSYS* | CYGWIN*) echo "windows" ;;
    *)
      echo "Unsupported Android NDK host: $(uname -s)" >&2
      return 1
      ;;
  esac
}

normalize_shell_path() {
  local path="$1"

  if [[ "$(android_host_family)" == "windows" ]] && command -v cygpath >/dev/null 2>&1; then
    cygpath -u "${path}"
  else
    echo "${path}"
  fi
}

android_ndk_toolchain_bin() {
  local ndk_home="$1"
  local host_family prebuilt_root host_pattern
  local -a host_directories

  host_family="$(android_host_family)"
  prebuilt_root="${ndk_home}/toolchains/llvm/prebuilt"
  if [[ ! -d "${prebuilt_root}" ]]; then
    echo "Missing Android NDK prebuilt directory: ${prebuilt_root}" >&2
    return 1
  fi

  if [[ -n "${ANDROID_NDK_HOST_TAG:-}" ]]; then
    host_directories=("${prebuilt_root}/${ANDROID_NDK_HOST_TAG}")
  else
    host_pattern="${host_family}-*"
    shopt -s nullglob
    host_directories=("${prebuilt_root}"/${host_pattern})
    shopt -u nullglob
  fi

  if [[ "${#host_directories[@]}" -ne 1 || ! -d "${host_directories[0]}" ]]; then
    echo "Expected exactly one ${host_family} Android NDK toolchain under ${prebuilt_root}." >&2
    echo "Set ANDROID_NDK_HOST_TAG to select an explicit NDK host directory." >&2
    return 1
  fi

  echo "${host_directories[0]}/bin"
}

android_ndk_tool() {
  local toolchain="$1"
  local tool_name="$2"
  local host_family candidate
  local -a suffixes

  host_family="$(android_host_family)"
  if [[ "${host_family}" == "windows" ]]; then
    suffixes=(".cmd" ".exe" "")
  else
    suffixes=("")
  fi

  for suffix in "${suffixes[@]}"; do
    candidate="${toolchain}/${tool_name}${suffix}"
    if [[ -f "${candidate}" && ("${host_family}" == "windows" || -x "${candidate}") ]]; then
      if [[ "${host_family}" == "windows" ]] && command -v cygpath >/dev/null 2>&1; then
        cygpath -w "${candidate}"
      else
        echo "${candidate}"
      fi
      return 0
    fi
  done

  echo "Missing Android NDK tool ${tool_name} under ${toolchain}" >&2
  return 1
}

configure_android_rust_toolchain() {
  local ndk_home toolchain compiler_prefix target_env
  local cc_path cxx_path ar_path cc_variable cxx_variable ar_variable
  ndk_home="$(android_ndk_home)"
  if [[ -z "${ndk_home}" ]]; then
    echo "ANDROID_NDK_HOME, ANDROID_NDK, ANDROID_NDK_ROOT, or an SDK ndk directory must be available for Android Rust checks." >&2
    exit 1
  fi

  case "${ANDROID_TARGET}" in
    aarch64-linux-android) compiler_prefix="aarch64-linux-android" ;;
    armv7-linux-androideabi) compiler_prefix="armv7a-linux-androideabi" ;;
    i686-linux-android) compiler_prefix="i686-linux-android" ;;
    x86_64-linux-android) compiler_prefix="x86_64-linux-android" ;;
    *)
      echo "Unsupported Android Rust target: ${ANDROID_TARGET}" >&2
      exit 1
      ;;
  esac

  toolchain="$(android_ndk_toolchain_bin "${ndk_home}")"
  cc_path="$(android_ndk_tool "${toolchain}" "${compiler_prefix}${ANDROID_API_LEVEL}-clang")"
  cxx_path="$(android_ndk_tool "${toolchain}" "${compiler_prefix}${ANDROID_API_LEVEL}-clang++")"
  ar_path="$(android_ndk_tool "${toolchain}" "llvm-ar")"

  target_env="${ANDROID_TARGET//-/_}"
  cc_variable="CC_${target_env}"
  cxx_variable="CXX_${target_env}"
  ar_variable="AR_${target_env}"
  export "${cc_variable}=${!cc_variable:-${cc_path}}"
  export "${cxx_variable}=${!cxx_variable:-${cxx_path}}"
  export "${ar_variable}=${!ar_variable:-${ar_path}}"

  echo "Using Android NDK toolchain: ${toolchain}"
}

run_android_kotlin_checks() {
  step "Syncing Tauri Android overrides"
  "${ROOT_DIR}/scripts/sync-tauri-android-overrides.sh"

  step "Compiling Android Kotlin and AIDL"
  (
    cd "${ROOT_DIR}/app/src-tauri/gen/android" &&
      ./gradlew :app:compileArm64DebugKotlin :llmd-sample:assembleDebug --no-daemon
  )
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
