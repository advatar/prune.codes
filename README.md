# Prune Codes

This is the canonical production repository for Prune.

It contains the macOS app, release packaging, product documentation, and the
embedded Prune engine workspace used by the app.

## Repository Contract

- `PruneApp/` is the macOS menu-bar application and Xcode project.
- `prune/` is the embedded Rust/Swift engine workspace shipped with the app.
- `PruneApp/scripts/` contains local and release DMG build scripts.
- `.github/workflows/` contains release automation.
- `docs/` contains product and launch documentation.
- `scripts/` contains repository-level maintenance checks.

The sibling checkout sometimes present at `../prune` is the retired standalone
engine repository (`advatar/prune.git`). It is not the production entry point.
New product, release, macOS app, and bundled-engine work should land here first.

Do not edit both engine copies independently. If `../prune` is a retired pointer
checkout, `scripts/check-engine-overlap.sh` skips it. If `../prune` is ever
revived as an engine repository, convert it to a submodule or mechanically
synchronize it before active development resumes. See
`docs/STANDALONE-PRUNE-RETIREMENT.md` for the retirement disposition.

## Core Checks

From this repository root:

```sh
cd prune && cargo test
cd prune/A2UIRuntime && swift test
xcodebuild -project PruneApp/PruneApp.xcodeproj -scheme PruneApp -configuration Debug -destination 'platform=macOS' build
./scripts/check-engine-overlap.sh
```

Build a local unsigned DMG:

```sh
./PruneApp/scripts/build-dmg.sh
```

Build a signed/notarized release DMG when Apple release credentials are
available:

```sh
./PruneApp/scripts/release-dmg.sh
```
