#!/bin/bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PROJECT="${ROOT}/PruneApp/PruneApp.xcodeproj"
SCHEME="${SCHEME:-PruneApp}"
CONFIGURATION="${CONFIGURATION:-Release}"
BUILD_DIR="${BUILD_DIR:-${ROOT}/build/release}"
DERIVED_DATA="${DERIVED_DATA:-${BUILD_DIR}/DerivedData}"
ARCHIVE_PATH="${ARCHIVE_PATH:-${BUILD_DIR}/${SCHEME}.xcarchive}"
DMG_DIR="${DMG_DIR:-${BUILD_DIR}/dmg}"
STAGING="${DMG_DIR}/staging"
OUTPUT_DIR="${OUTPUT_DIR:-${DMG_DIR}/out}"
APPLE_DEVELOPER_IDENTITY="${APPLE_DEVELOPER_IDENTITY:-}"
APPLE_ENTITLEMENTS_PATH="${APPLE_ENTITLEMENTS_PATH:-}"
APPLE_NOTARY_KEY_PATH="${APPLE_NOTARY_KEY_PATH:-}"
APPLE_NOTARY_KEY_ID="${APPLE_NOTARY_KEY_ID:-}"
APPLE_NOTARY_ISSUER_ID="${APPLE_NOTARY_ISSUER_ID:-}"
SKIP_NOTARIZATION="${SKIP_NOTARIZATION:-0}"

if [ -z "${APPLE_DEVELOPER_IDENTITY}" ]; then
  echo "error: APPLE_DEVELOPER_IDENTITY must be set to a Developer ID Application identity." >&2
  exit 1
fi

codesign_file() {
  local target="$1"
  local -a args
  args=(--force --timestamp --options runtime --sign "${APPLE_DEVELOPER_IDENTITY}")
  if [ -n "${APPLE_ENTITLEMENTS_PATH}" ]; then
    args+=(--entitlements "${APPLE_ENTITLEMENTS_PATH}")
  fi
  codesign "${args[@]}" "${target}"
}

mkdir -p "${BUILD_DIR}" "${OUTPUT_DIR}"
rm -rf "${ARCHIVE_PATH}"

xcodebuild \
  -project "${PROJECT}" \
  -scheme "${SCHEME}" \
  -configuration "${CONFIGURATION}" \
  -derivedDataPath "${DERIVED_DATA}" \
  -archivePath "${ARCHIVE_PATH}" \
  CODE_SIGNING_ALLOWED=NO \
  archive

APP_PATH="${ARCHIVE_PATH}/Products/Applications/PruneApp.app"
if [ ! -d "${APP_PATH}" ]; then
  echo "error: app not found at ${APP_PATH}" >&2
  exit 1
fi

if [ -d "${APP_PATH}/Contents/Resources/bin" ]; then
  while IFS= read -r -d '' helper_path; do
    codesign_file "${helper_path}"
  done < <(find "${APP_PATH}/Contents/Resources/bin" -type f -perm -111 -print0)
fi

codesign_file "${APP_PATH}"
codesign --verify --deep --strict --verbose=2 "${APP_PATH}"

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
codesign --force --timestamp --sign "${APPLE_DEVELOPER_IDENTITY}" "${DMG_PATH}"

if [ "${SKIP_NOTARIZATION}" != "1" ]; then
  if [ -z "${APPLE_NOTARY_KEY_PATH}" ] || [ -z "${APPLE_NOTARY_KEY_ID}" ] || [ -z "${APPLE_NOTARY_ISSUER_ID}" ]; then
    echo "error: notarization requires APPLE_NOTARY_KEY_PATH, APPLE_NOTARY_KEY_ID, and APPLE_NOTARY_ISSUER_ID." >&2
    exit 1
  fi

  xcrun notarytool submit "${DMG_PATH}" \
    --key "${APPLE_NOTARY_KEY_PATH}" \
    --key-id "${APPLE_NOTARY_KEY_ID}" \
    --issuer "${APPLE_NOTARY_ISSUER_ID}" \
    --wait
  xcrun stapler staple "${DMG_PATH}"
  xcrun stapler validate "${DMG_PATH}"
fi

shasum -a 256 "${DMG_PATH}" > "${DMG_PATH}.sha256"
echo "Release DMG created: ${DMG_PATH}"
