# Status

## Current Task
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

## Progress
- Added root MIT-style `LICENSE.md`.
- Audited `RELEASE.md`: Launch v1 is not feature complete. DMG scripts/workflow exist, and parts of the app/service surface exist, but release readiness still depends on closing product gaps in setup, service lifecycle, Lovable MCP tools, diagnostics, analytics, and end-to-end verification.
- Re-ran `cargo test` with network access; dependency resolution completed, but the workspace still fails in `ce-lang-swift` because `tree_sitter_swift::LANGUAGE.into()` does not compile against the current `tree-sitter`/grammar crate types.
- Audited existing macOS distribution assets: `PruneApp/scripts/build-dmg.sh` already creates a local DMG, but signing/notarization and release publishing are still missing.
- Added a downloadable-app release path: reusable DMG build script inputs, `PruneApp/scripts/release-dmg.sh`, `.github/workflows/release-macos-dmg.yml`, `docs/RELEASING-MACOS-DMG.md`, and canonical release commands in `prune/docs/ai/dev_commands.yaml`.
- Verified release-path syntax with `bash -n` for the shell scripts, YAML parsing for the GitHub Actions workflow, and `git diff --check`; end-to-end notarization is not locally exercised because Apple signing secrets are not available in this workspace.
- Confirmed `prune.codes/lovable-template` is an isolated Vite/Vitest sample app; cleanup will leave `PruneApp` Lovable MCP and instructions flows intact.
- Removed `lovable-template/` and the stale Vitest note from `AGENTS.md`; repo-wide grep now shows no remaining `lovable-template` or `vitest` references outside this status log.
- Verification attempted from `prune/docs/ai/dev_commands.yaml`: `cargo test` currently fails in `ce-lang-tsreact` / `ce-lang-swift` tree-sitter language setup, and `cd A2UIRuntime && swift test` currently fails due duplicate `A2UIProtocolVersion` / `JSONValue` definitions in `A2UIRuntime`.
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
