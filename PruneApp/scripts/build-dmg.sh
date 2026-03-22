#!/bin/bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PROJECT="${ROOT}/PruneApp/PruneApp.xcodeproj"
SCHEME="${SCHEME:-PruneApp}"
CONFIGURATION="${CONFIGURATION:-Release}"
BUILD_DIR="${BUILD_DIR:-${ROOT}/build}"
DERIVED_DATA="${DERIVED_DATA:-${BUILD_DIR}/DerivedData}"
DMG_DIR="${DMG_DIR:-${BUILD_DIR}/dmg}"
STAGING="${DMG_DIR}/staging"
OUTPUT_DIR="${OUTPUT_DIR_OVERRIDE:-${DMG_DIR}/out}"

APP_PATH="${APP_PATH_OVERRIDE:-}"
if [ -z "${APP_PATH}" ]; then
  xcodebuild -project "${PROJECT}" -scheme "${SCHEME}" -configuration "${CONFIGURATION}" -derivedDataPath "${DERIVED_DATA}" build
  APP_PATH="${DERIVED_DATA}/Build/Products/${CONFIGURATION}/PruneApp.app"
fi
if [ ! -d "${APP_PATH}" ]; then
  echo "error: app not found at ${APP_PATH}" >&2
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
ln -s /Applications "${STAGING}/Applications"

hdiutil create -volname "${VOL_NAME}" -srcfolder "${STAGING}" -ov -format UDZO "${DMG_PATH}"

echo "DMG created: ${DMG_PATH}"
