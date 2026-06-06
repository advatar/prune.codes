#!/usr/bin/env bash
set -euo pipefail

if [ -f "${HOME}/.cargo/env" ]; then
  # shellcheck disable=SC1091
  . "${HOME}/.cargo/env"
fi

export PATH="${HOME}/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:${PATH}"

REPO_ROOT="${SRCROOT}/.."
PRUNE_ROOT="${REPO_ROOT}/prune"
if [ -f "${REPO_ROOT}/Cargo.toml" ] && [ -d "${REPO_ROOT}/crates" ]; then
  PRUNE_ROOT="${REPO_ROOT}"
elif [ ! -f "${PRUNE_ROOT}/Cargo.toml" ]; then
  echo "error: could not find prune Cargo.toml from ${SRCROOT}" >&2
  exit 1
fi

BIN_DIR="${TARGET_BUILD_DIR}/${UNLOCALIZED_RESOURCES_FOLDER_PATH}/bin"
mkdir -p "${BIN_DIR}"

PROFILE_DIR=debug
BUILD_FLAG=""
if [ "${CONFIGURATION}" != "Debug" ]; then
  PROFILE_DIR=release
  BUILD_FLAG="--release"
fi

CARGO_BIN="${CARGO:-cargo}"
CARGO_TARGET_DIR="${TARGET_TEMP_DIR}/cargo-target"
"${CARGO_BIN}" build ${BUILD_FLAG} --target-dir "${CARGO_TARGET_DIR}" --manifest-path "${PRUNE_ROOT}/Cargo.toml" -p ce-cli --bin ce
"${CARGO_BIN}" build ${BUILD_FLAG} --target-dir "${CARGO_TARGET_DIR}" --manifest-path "${PRUNE_ROOT}/Cargo.toml" -p ce-mcp --bin ce-mcp
"${CARGO_BIN}" build ${BUILD_FLAG} --target-dir "${CARGO_TARGET_DIR}" --manifest-path "${PRUNE_ROOT}/Cargo.toml" -p prune-mcp --bin prune-mcp
"${CARGO_BIN}" build ${BUILD_FLAG} --target-dir "${CARGO_TARGET_DIR}" --manifest-path "${PRUNE_ROOT}/Cargo.toml" -p prune-sync --bin prune-sync

cp "${CARGO_TARGET_DIR}/${PROFILE_DIR}/ce" "${BIN_DIR}/"
cp "${CARGO_TARGET_DIR}/${PROFILE_DIR}/ce-mcp" "${BIN_DIR}/"
cp "${CARGO_TARGET_DIR}/${PROFILE_DIR}/prune-mcp" "${BIN_DIR}/"
cp "${CARGO_TARGET_DIR}/${PROFILE_DIR}/prune-sync" "${BIN_DIR}/"

find_cloudflared() {
  if [ -n "${CLOUDFLARED_SOURCE:-}" ] && [ -f "${CLOUDFLARED_SOURCE}" ]; then
    printf '%s\n' "${CLOUDFLARED_SOURCE}"
    return 0
  fi

  if command -v cloudflared >/dev/null 2>&1; then
    command -v cloudflared
    return 0
  fi

  for candidate in \
    /opt/homebrew/bin/cloudflared \
    /opt/homebrew/opt/cloudflared/bin/cloudflared \
    /usr/local/bin/cloudflared \
    /usr/local/opt/cloudflared/bin/cloudflared \
    "${REPO_ROOT}/../iCodex/bridge/vendor/cloudflared"
  do
    if [ -f "${candidate}" ]; then
      printf '%s\n' "${candidate}"
      return 0
    fi
  done

  return 1
}

if cloudflared_source="$(find_cloudflared)"; then
  echo "Bundling cloudflared from ${cloudflared_source}"
  cp "${cloudflared_source}" "${BIN_DIR}/cloudflared"
else
  echo "warning: cloudflared not found; install with 'brew install cloudflared' or set CLOUDFLARED_SOURCE" >&2
fi

chmod +x "${BIN_DIR}/ce" "${BIN_DIR}/ce-mcp" "${BIN_DIR}/prune-mcp" "${BIN_DIR}/prune-sync" 2>/dev/null || true
if [ -f "${BIN_DIR}/cloudflared" ]; then
  chmod +x "${BIN_DIR}/cloudflared"
fi
