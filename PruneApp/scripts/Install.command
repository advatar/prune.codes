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
if [ "${#missing[@]}" -gt 0 ]; then
  echo "error: missing bundled binaries in ${APP_NAME}: ${missing[*]}"
  exit 1
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

echo "Installed to ${DEST_APP}"
echo "Launching PruneApp..."
open -a "${DEST_APP}"
