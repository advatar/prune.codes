#!/bin/bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PROJECT="${ROOT}/PruneApp/PruneApp.xcodeproj"
SCHEME="PruneApp"
CONFIGURATION="${CONFIGURATION:-Release}"
BUILD_DIR="${ROOT}/build"
DERIVED_DATA="${BUILD_DIR}/DerivedData"
DMG_DIR="${BUILD_DIR}/dmg"
STAGING="${DMG_DIR}/staging"
OUTPUT_DIR="${DMG_DIR}/out"

xcodebuild -project "${PROJECT}" -scheme "${SCHEME}" -configuration "${CONFIGURATION}" -derivedDataPath "${DERIVED_DATA}" build

APP_PATH="${DERIVED_DATA}/Build/Products/${CONFIGURATION}/PruneApp.app"
if [ ! -d "${APP_PATH}" ]; then
  echo "error: app not found at ${APP_PATH}" >&2
  exit 1
fi

BIN_DIR="${APP_PATH}/Contents/Resources/bin"
REQUIRED_BINS=(ce ce-mcp prune-mcp prune-sync cloudflared)
missing=()
for name in "${REQUIRED_BINS[@]}"; do
  if [ ! -x "${BIN_DIR}/${name}" ]; then
    missing+=("${name}")
  fi
done
if [ "${#missing[@]}" -gt 0 ]; then
  echo "error: missing bundled binaries in app bundle: ${missing[*]}" >&2
  echo "hint: ensure PruneApp/scripts/bundle-binaries.sh can locate the required binaries." >&2
  exit 1
fi

VERSION="$(defaults read "${APP_PATH}/Contents/Info.plist" CFBundleShortVersionString 2>/dev/null || true)"
if [ -z "${VERSION}" ]; then
  VERSION="dev"
fi

VOL_NAME="${DMG_VOLUME_NAME:-Prune}"
DMG_NAME="${DMG_NAME_OVERRIDE:-PruneApp-${VERSION}.dmg}"
DMG_PATH="${OUTPUT_DIR}/${DMG_NAME}"

rm -rf "${STAGING}"
mkdir -p "${STAGING}" "${OUTPUT_DIR}"
cp -R "${APP_PATH}" "${STAGING}/"
INSTALL_SCRIPT="${ROOT}/PruneApp/scripts/Install.command"
if [ ! -f "${INSTALL_SCRIPT}" ]; then
  echo "error: missing Install.command at ${INSTALL_SCRIPT}" >&2
  exit 1
fi
cp "${INSTALL_SCRIPT}" "${STAGING}/Install.command"
chmod +x "${STAGING}/Install.command" 2>/dev/null || true
ln -s /Applications "${STAGING}/Applications"

hdiutil create -volname "${VOL_NAME}" -srcfolder "${STAGING}" -ov -format UDZO "${DMG_PATH}"

echo "DMG created: ${DMG_PATH}"
