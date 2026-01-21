#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

export FASTEMBED_CACHE_DIR="${FASTEMBED_CACHE_DIR:-${REPO_ROOT}/.fastembed_cache}"

if ! command -v cargo-llvm-cov >/dev/null 2>&1; then
  echo "cargo-llvm-cov not found. Install with: cargo install cargo-llvm-cov" >&2
  exit 1
fi

cd "${REPO_ROOT}"
cargo llvm-cov --summary-only --workspace
