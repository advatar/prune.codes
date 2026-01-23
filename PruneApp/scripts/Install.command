#!/bin/bash
set -euo pipefail

APP_NAME="PruneApp.app"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SOURCE_APP="${SCRIPT_DIR}/${APP_NAME}"
DEST_DIR="/Applications"
DEST_APP="${DEST_DIR}/${APP_NAME}"

if [ ! -d "${SOURCE_APP}" ]; then
  echo "error: ${APP_NAME} not found next to installer."
  echo "expected: ${SOURCE_APP}"
  exit 1
fi

BIN_DIR="${SOURCE_APP}/Contents/Resources/bin"
REQUIRED_BINS=(ce ce-mcp prune-mcp prune-sync cloudflared)
missing=()
for name in "${REQUIRED_BINS[@]}"; do
  if [ ! -x "${BIN_DIR}/${name}" ]; then
    missing+=("${name}")
  fi
done

CLOUDFLARED_SOURCE="${PRUNE_CLOUDFLARED_PATH:-}"
if [ -z "${CLOUDFLARED_SOURCE}" ] && command -v cloudflared >/dev/null 2>&1; then
  CLOUDFLARED_SOURCE="$(command -v cloudflared)"
elif [ -z "${CLOUDFLARED_SOURCE}" ] && [ -x "/opt/homebrew/bin/cloudflared" ]; then
  CLOUDFLARED_SOURCE="/opt/homebrew/bin/cloudflared"
elif [ -z "${CLOUDFLARED_SOURCE}" ] && [ -x "/opt/homebrew/opt/cloudflared/bin/cloudflared" ]; then
  CLOUDFLARED_SOURCE="/opt/homebrew/opt/cloudflared/bin/cloudflared"
elif [ -z "${CLOUDFLARED_SOURCE}" ] && [ -x "/usr/local/bin/cloudflared" ]; then
  CLOUDFLARED_SOURCE="/usr/local/bin/cloudflared"
fi

if [ "${#missing[@]}" -gt 0 ]; then
  non_cloudflared=()
  for name in "${missing[@]}"; do
    if [ "${name}" != "cloudflared" ]; then
      non_cloudflared+=("${name}")
    fi
  done

  if [ "${#non_cloudflared[@]}" -gt 0 ]; then
    echo "error: missing bundled binaries in ${APP_NAME}: ${non_cloudflared[*]}"
    exit 1
  fi

  if [ -z "${CLOUDFLARED_SOURCE}" ]; then
    echo "error: missing bundled binary cloudflared and no system install found."
    echo "hint: install cloudflared (brew install cloudflared) or set PRUNE_CLOUDFLARED_PATH."
    exit 1
  fi
fi

copy_app() {
  /usr/bin/ditto "${SOURCE_APP}" "${DEST_APP}"
}

if [ -w "${DEST_DIR}" ]; then
  if [ -d "${DEST_APP}" ]; then
    echo "Removing existing ${DEST_APP}..."
    rm -rf "${DEST_APP}"
  fi
  copy_app
else
  echo "Requesting administrator permission to copy into ${DEST_DIR}..."
  /usr/bin/osascript <<EOF
set src to "${SOURCE_APP}"
set dest to "${DEST_APP}"
do shell script "rm -rf " & quoted form of dest & " && ditto " & quoted form of src & " " & quoted form of dest with administrator privileges
EOF
fi

DEST_BIN_DIR="${DEST_APP}/Contents/Resources/bin"
if [ ! -x "${DEST_BIN_DIR}/cloudflared" ] && [ -n "${CLOUDFLARED_SOURCE}" ] && [ -x "${CLOUDFLARED_SOURCE}" ]; then
  echo "Installing cloudflared from ${CLOUDFLARED_SOURCE}..."
  mkdir -p "${DEST_BIN_DIR}"
  cp "${CLOUDFLARED_SOURCE}" "${DEST_BIN_DIR}/cloudflared"
  chmod +x "${DEST_BIN_DIR}/cloudflared" 2>/dev/null || true
fi

echo "Installed to ${DEST_APP}"
echo "Launching PruneApp..."
open -a "${DEST_APP}"
