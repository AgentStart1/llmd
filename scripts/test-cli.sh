#!/usr/bin/env bash
set -euo pipefail

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
cd "${root_dir}"
exec cargo test -p llmd --locked --test cli "$@"
