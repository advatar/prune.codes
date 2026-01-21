#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

DB_PATH="${CE_DB_PATH:-${REPO_ROOT}/.ce/index.sqlite}"
HNSW_DIR="${CE_HNSW_DIR:-${REPO_ROOT}/.ce/hnsw}"

mkdir -p "$(dirname "${DB_PATH}")" "${HNSW_DIR}"

CE_MCP_BIN="${CE_MCP_BIN:-}"
if [[ -z "${CE_MCP_BIN}" ]]; then
  if command -v ce-mcp >/dev/null 2>&1; then
    CE_MCP_BIN="ce-mcp"
  elif [[ -x "${REPO_ROOT}/target/release/ce-mcp" ]]; then
    CE_MCP_BIN="${REPO_ROOT}/target/release/ce-mcp"
  elif [[ -x "${REPO_ROOT}/target/debug/ce-mcp" ]]; then
    CE_MCP_BIN="${REPO_ROOT}/target/debug/ce-mcp"
  else
    echo "ce-mcp not found. Install it (cargo install --path crates/ce-mcp --bin ce-mcp --force) or build in ${REPO_ROOT}/target." >&2
    exit 1
  fi
fi

exec "${CE_MCP_BIN}" --db "${DB_PATH}" --hnsw-dir "${HNSW_DIR}"
