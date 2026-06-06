# Status

## Current Task
- Clarify and productionize the `prune` vs `prune.codes` split: identify canonical ownership, quantify overlap/drift, and plan consolidation without losing app, release, language-pack, or standalone-engine work.
- Add a root MIT-style `LICENSE.md`.
- Audit `RELEASE.md` Launch v1 scope against the current implementation and keep status honest about incomplete release work.
- Make PruneApp downloadable via a signed/notarized DMG release pipeline and GitHub Releases automation.
- Consolidate the repo by removing the checked-in `lovable-template` sample app and stale repo-level frontend guidance while preserving product Lovable integrations.
- Sync context-engine-v24 REACT/TSX language pack into `prune` (TS/TSX parser, import graph alias resolution, JSX edges, CLI/DB updates, README).
- Add Codex MCP autostart helper to avoid `context_engine` startup failures in new terminals.
- Add coverage runner + baseline report and expand tests (ce-store + ce-mcp JSON-RPC + MCP smoke).
- Extend PruneApp A2UI diagnostics to support v0.8 fixtures, JSONL load, and live stream ingestion.
- Update `.gitignore` to ignore PruneApp Xcode build artifacts (DerivedData and related outputs).
- Fix `.gitignore` for the `prune` Rust workspace (Cargo artifacts and rustfmt backups).
- Resolve still-relevant `REVIEW.md` consolidation fallout in the nested `prune` copy so canonical checks target a buildable package surface.

## Progress
- Audited local repo layout: `prune/` is the standalone engine repo (`advatar/prune.git`); `prune.codes/` is the product/distribution repo (`advatar/prune.codes.git`) containing PruneApp, release docs/workflows, and a nested `prune/` engine copy.
- Created tracking issue for consolidation and production readiness: https://github.com/advatar/prune.codes/issues/21.
- Compared tracked engine files: 60 shared paths between `prune/` and `prune.codes/prune/`, 49 byte-identical shared files, 11 drifted shared files, 8 files only in standalone `prune/`, and 35 engine files only in the nested `prune.codes/prune/` copy.
- Confirmed the nested engine copy is ahead functionally: it includes Swift and TS/React language crates, bootstrap/integration/doctor/MCP CLI flows, templates, and integration assets that are absent from standalone `prune/`.
- Confirmed the product repo owns production distribution assets: menu-bar PruneApp, GitHub webhook/Keychain/LaunchAgent/tunnel service management, diagnostics UI, DMG scripts, notarization workflow, and release docs.
- Verification for the consolidation audit: `git diff --check`, `cd prune && cargo test`, and `cd prune/A2UIRuntime && swift test` pass; `xcodebuild -project PruneApp/PruneApp.xcodeproj -scheme PruneApp -configuration Debug -destination 'platform=macOS' build` fails in `PruneApp/PruneApp/ContentView.swift` because A2UI diagnostics code is out of sync with the current `A2UIRuntime` API and there is an invalid `.sheet` modifier placement.
- Implementation direction accepted: use `prune.codes` as the production repo, treat the parent `CONSOLIDATION_PLAN.md` as a completed/narrow Lovable-template cleanup note, and start by restoring PruneApp build cleanliness before adding canonical repo-contract and drift-guard docs/checks.
- Restored PruneApp build cleanliness by moving the inception interview sheet to a valid view modifier position and adapting the local inception A2UI renderer to the current `A2UIRuntime` model (`NormalizedSurfaceInfo`, `NormalizedDataUpdate`, immutable `NormalizedComponent`, and type-erased recursive rendering).
- Added root production repo contract docs in `README.md`, root canonical dev commands in `docs/ai/dev_commands.yaml`, and `scripts/check-engine-overlap.sh` to audit drift against a sibling standalone `../prune` checkout.
- Verification after implementation: `git diff --check`, `bash -n scripts/check-engine-overlap.sh`, `./scripts/check-engine-overlap.sh`, `cd prune && cargo test`, `cd prune/A2UIRuntime && swift test`, `xcodebuild -project PruneApp/PruneApp.xcodeproj -scheme PruneApp -configuration Debug -destination 'platform=macOS' build`, and `./PruneApp/scripts/build-dmg.sh` all pass. The local unsigned DMG is at `build/dmg/out/PruneApp-1.0.dmg`; build output still warns that bundled `cloudflared` was not found at the expected vendor path.
- Added root MIT-style `LICENSE.md`.
- Audited `RELEASE.md`: Launch v1 is not feature complete. DMG scripts/workflow exist, and parts of the app/service surface exist, but release readiness still depends on closing product gaps in setup, service lifecycle, Lovable MCP tools, diagnostics, analytics, and end-to-end verification.
- Re-ran `cargo test` with network access; dependency resolution completed, but the workspace still fails in `ce-lang-swift` because `tree_sitter_swift::LANGUAGE.into()` does not compile against the current `tree-sitter`/grammar crate types.
- Audited existing macOS distribution assets: `PruneApp/scripts/build-dmg.sh` already creates a local DMG, but signing/notarization and release publishing are still missing.
- Added a downloadable-app release path: reusable DMG build script inputs, `PruneApp/scripts/release-dmg.sh`, `.github/workflows/release-macos-dmg.yml`, `docs/RELEASING-MACOS-DMG.md`, and canonical release commands in `prune/docs/ai/dev_commands.yaml`.
- Verified release-path syntax with `bash -n` for the shell scripts, YAML parsing for the GitHub Actions workflow, and `git diff --check`; end-to-end notarization is not locally exercised because Apple signing secrets are not available in this workspace.
- Confirmed `prune.codes/lovable-template` is an isolated Vite/Vitest sample app; cleanup will leave `PruneApp` Lovable MCP and instructions flows intact.
- Removed `lovable-template/` and the stale Vitest note from `AGENTS.md`; repo-wide grep now shows no remaining `lovable-template` or `vitest` references outside this status log.
- Earlier verification from `prune/docs/ai/dev_commands.yaml` exposed failures in `ce-lang-tsreact` / `ce-lang-swift` tree-sitter language setup and duplicate A2UIRuntime type definitions.
- REVIEW follow-up: removed the stale nested A2UI aggregate runtime file, aligned all Rust language adapters on `tree-sitter` 0.24 APIs, refreshed `Cargo.lock`, fixed the stale `ce-cli` onboarding fragment lookup, and verified `cargo test` plus `cd A2UIRuntime && swift test` now pass in `prune.codes/prune`.
- Triaged Xcode build failure: `cargo` missing in build phase PATH; adding explicit cargo discovery and error messaging.
- Fixed `ce-cli` integration template include paths and rebuilt `cargo build -p ce-cli` (warnings remain).
- Updated Bundle Prune Binaries script to resolve the prune repo root and ran `xcodebuild` (warnings remain).
- GitHub issues created: CR-01..CR-05 + Epic (advatar/prune).
- Added signal extraction, support-closure metrics, connected-subgraph selection, TSX/SwiftUI skeletonization, and recipe memory plumbing across `ce-core`, `ce-store`, `ce-cli`, and `ce-mcp`.
- Added recipe persistence schema + DB methods and wired pack rendering to include recipe excerpts and metrics.
- Implemented `ce recipe` CLI (add/list/export).
- `cargo test` passed (warnings in ce-lang-rust, ce-store, ce-cli).
- Reviewed `../context-engine-v22` against `SWIFT.md`; Swift/SwiftUI pack features already present.
- Updating MCP startup troubleshooting to point to the repo helper script.
- Added ce-store smoke tests, ce-mcp JSON-RPC tests, and MCP pack integration smoke test.
- Added llvm-cov coverage script and baseline report.
- Wired EPIC vNext metrics logging in CLI/MCP output and eval (hit_rate_paths, avg_iterations_per_fix, redundancy_pct).
- Added CLI eval redundancy tracking across tasks so `redundancy_pct` reports actual reuse.
- Applied `avoid_seen` scoring in CLI eval/strategy runs so redundancy is reduced, not just reported.
- Fixed PruneApp tray "Open Dashboard" action to open the settings window reliably.
- Built and installed PruneApp (app copied to /Applications; default config + bundled binaries installed).
- Drafted `INCEPTION_NEXT.md` template; awaiting version details.
- Updated `INCEPTION_NEXT.md` with A2UI v0.8/v0.9 requirements from `v0.9.md`.
- Implemented A2UIRuntime Swift package with adapters, normalized store, fixtures, and tests.
- Promoted A2UI v0.9 as the default protocol and updated versioning docs/adapter selection.
- Fixed PruneApp startup crash and added an A2UI fixture diagnostics panel for inception testing.

## Next Steps
- Validate Xcode build after updating cargo discovery in the Bundle Prune Binaries script.
- Integrate ce-lang-tsreact and TS/TSX indexing + ApiSummary refs in CLI.
- Port tsconfig alias resolution + JSX tag edges into `ce-store`.
- Update README + validation steps for TS/TSX support.

## Notes
- Local repo has uncommitted changes in `Cargo.toml`, `Cargo.lock`, `crates/prune-mcp/`, and `crates/prune-sync/` (left untouched).
