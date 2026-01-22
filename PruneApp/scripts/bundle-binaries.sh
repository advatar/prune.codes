#!/bin/bash
set -euo pipefail

SRCROOT="${SRCROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
REPO_ROOT="$(cd "${SRCROOT}/.." && pwd)"
PRUNE_ROOT="${REPO_ROOT}/prune"

if [ -f "${REPO_ROOT}/Cargo.toml" ] && [ -d "${REPO_ROOT}/crates" ]; then
  PRUNE_ROOT="${REPO_ROOT}"
elif [ ! -f "${PRUNE_ROOT}/Cargo.toml" ]; then
  echo "error: could not find prune Cargo.toml from ${SRCROOT}" >&2
  exit 1
fi

BIN_DIR="${TARGET_BUILD_DIR}/${UNLOCALIZED_RESOURCES_FOLDER_PATH}/bin"
mkdir -p "${BIN_DIR}"

PROFILE_DIR="debug"
BUILD_FLAG=""
if [ "${CONFIGURATION:-Debug}" != "Debug" ]; then
  PROFILE_DIR="release"
  BUILD_FLAG="--release"
fi

REQUIRED_BINS=(ce ce-mcp prune-mcp prune-sync cloudflared)

SOURCE_DIR=""
if [ -n "${PRUNE_BUNDLE_BIN_DIR:-}" ]; then
  SOURCE_DIR="${PRUNE_BUNDLE_BIN_DIR}"
elif [ -d "${SRCROOT}/Resources/bin" ]; then
  SOURCE_DIR="${SRCROOT}/Resources/bin"
fi

if [ -z "${SOURCE_DIR}" ]; then
  if [ -f "${HOME}/.cargo/env" ]; then
    . "${HOME}/.cargo/env"
  fi
  export PATH="${HOME}/.cargo/bin:${PATH}"
  CARGO_BIN="${CARGO:-cargo}"
  CARGO_TARGET_DIR="${TARGET_TEMP_DIR}/cargo-target"

  "${CARGO_BIN}" build ${BUILD_FLAG} --target-dir "${CARGO_TARGET_DIR}" --manifest-path "${PRUNE_ROOT}/Cargo.toml" -p ce-cli --bin ce
  "${CARGO_BIN}" build ${BUILD_FLAG} --target-dir "${CARGO_TARGET_DIR}" --manifest-path "${PRUNE_ROOT}/Cargo.toml" -p ce-mcp --bin ce-mcp
  "${CARGO_BIN}" build ${BUILD_FLAG} --target-dir "${CARGO_TARGET_DIR}" --manifest-path "${PRUNE_ROOT}/Cargo.toml" -p prune-mcp --bin prune-mcp
  "${CARGO_BIN}" build ${BUILD_FLAG} --target-dir "${CARGO_TARGET_DIR}" --manifest-path "${PRUNE_ROOT}/Cargo.toml" -p prune-sync --bin prune-sync

  SOURCE_DIR="${CARGO_TARGET_DIR}/${PROFILE_DIR}"
fi

CLOUDFLARED_SOURCE="${PRUNE_CLOUDFLARED_PATH:-}"
if [ -z "${CLOUDFLARED_SOURCE}" ] && [ -f "${SRCROOT}/vendor/cloudflared" ]; then
  CLOUDFLARED_SOURCE="${SRCROOT}/vendor/cloudflared"
elif [ -z "${CLOUDFLARED_SOURCE}" ] && [ -f "${REPO_ROOT}/vendor/cloudflared" ]; then
  CLOUDFLARED_SOURCE="${REPO_ROOT}/vendor/cloudflared"
elif [ -z "${CLOUDFLARED_SOURCE}" ] && command -v cloudflared >/dev/null 2>&1; then
  CLOUDFLARED_SOURCE="$(command -v cloudflared)"
elif [ -z "${CLOUDFLARED_SOURCE}" ] && [ -x "/opt/homebrew/bin/cloudflared" ]; then
  CLOUDFLARED_SOURCE="/opt/homebrew/bin/cloudflared"
elif [ -z "${CLOUDFLARED_SOURCE}" ] && [ -x "/usr/local/bin/cloudflared" ]; then
  CLOUDFLARED_SOURCE="/usr/local/bin/cloudflared"
fi

missing=()
found_names=()
found_sources=()
for name in "${REQUIRED_BINS[@]}"; do
  if [ -f "${SOURCE_DIR}/${name}" ]; then
    found_names+=("${name}")
    found_sources+=("${SOURCE_DIR}/${name}")
    continue
  fi
  if [ "${name}" = "cloudflared" ] && [ -n "${CLOUDFLARED_SOURCE}" ] && [ -f "${CLOUDFLARED_SOURCE}" ]; then
    found_names+=("${name}")
    found_sources+=("${CLOUDFLARED_SOURCE}")
    continue
  fi
  missing+=("${name}")
done

if [ "${#missing[@]}" -gt 0 ]; then
  if [ "${CONFIGURATION:-Debug}" != "Debug" ]; then
    echo "error: missing required bundled binaries: ${missing[*]}" >&2
    echo "hint: set PRUNE_BUNDLE_BIN_DIR or PRUNE_CLOUDFLARED_PATH, or add ${SRCROOT}/vendor/cloudflared" >&2
    exit 1
  else
    echo "warning: missing bundled binaries: ${missing[*]}" >&2
  fi
fi

for idx in "${!found_names[@]}"; do
  name="${found_names[$idx]}"
  source="${found_sources[$idx]}"
  dest="${BIN_DIR}/${name}"
  if [ -f "${dest}" ]; then
    rm -f "${dest}"
  fi
  cp "${source}" "${dest}"
  chmod +x "${dest}" 2>/dev/null || true
done

if [ "${#missing[@]}" -eq 0 ]; then
  echo "Bundled binaries into ${BIN_DIR}"
fi
