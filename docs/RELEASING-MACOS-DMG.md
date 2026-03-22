# Releasing PruneApp as a Downloadable DMG

This repo now supports two macOS packaging paths:

- `./PruneApp/scripts/build-dmg.sh`
  Builds a local DMG for testing. This is useful for manual install checks, but it does not sign or notarize the app.
- `./PruneApp/scripts/release-dmg.sh`
  Builds a release archive, signs the app and bundled helper binaries, creates a DMG, signs the DMG, notarizes it, staples the ticket, and writes a `.sha256` checksum file.

## Fast path

For real users, the intended path is:

1. Push a version tag such as `v1.0.0`
2. GitHub Actions runs `.github/workflows/release-macos-dmg.yml`
3. The workflow builds and notarizes `PruneApp-<version>.dmg`
4. The DMG and checksum are published to the GitHub Release for that tag

## Required GitHub Actions secrets

The release workflow expects these repository secrets:

- `APPLE_DEVELOPER_IDENTITY`
  Example: `Developer ID Application: Example, Inc. (TEAMID)`
- `MACOS_CERTIFICATE_P12_BASE64`
  Base64-encoded Developer ID Application `.p12`
- `MACOS_CERTIFICATE_PASSWORD`
  Password for the `.p12`
- `MACOS_KEYCHAIN_PASSWORD`
  Password used for the temporary CI keychain
- `APPLE_NOTARY_KEY_ID`
  App Store Connect API key id for notarization
- `APPLE_NOTARY_ISSUER_ID`
  App Store Connect issuer id for notarization
- `APPLE_NOTARY_API_PRIVATE_KEY_BASE64`
  Base64-encoded contents of `AuthKey_<KEYID>.p8`

## Local commands

Build a local unsigned DMG:

```sh
./PruneApp/scripts/build-dmg.sh
```

Build a signed DMG but skip notarization:

```sh
APPLE_DEVELOPER_IDENTITY="Developer ID Application: Example, Inc. (TEAMID)" \
SKIP_NOTARIZATION=1 \
./PruneApp/scripts/release-dmg.sh
```

Build, sign, and notarize locally:

```sh
APPLE_DEVELOPER_IDENTITY="Developer ID Application: Example, Inc. (TEAMID)" \
APPLE_NOTARY_KEY_PATH="$HOME/.private_keys/AuthKey_ABC1234567.p8" \
APPLE_NOTARY_KEY_ID="ABC1234567" \
APPLE_NOTARY_ISSUER_ID="00000000-0000-0000-0000-000000000000" \
./PruneApp/scripts/release-dmg.sh
```

## What the release script signs

`PruneApp` bundles helper executables under `PruneApp.app/Contents/Resources/bin/` during the Xcode build. The release script explicitly signs those helpers before signing the app bundle so Gatekeeper and notarization treat the bundle as a coherent release artifact.

## Output locations

By default, the release script writes artifacts under `build/release/dmg/out/`:

- `PruneApp-<version>.dmg`
- `PruneApp-<version>.dmg.sha256`

## Tagging a release

Once the required secrets are present:

```sh
git tag v1.0.0
git push origin v1.0.0
```

That tag triggers the release workflow and publishes the downloadable DMG to GitHub Releases.
