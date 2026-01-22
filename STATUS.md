# Status

## Current Task
- Unify A2UIRuntime types to resolve duplicate definitions and restore Xcode builds.
- Ensure PruneApp DMG bundles all required binaries for offline install.
- Add an Install.command to the DMG so users can install the app + bundled dependencies in one step.
- Document PruneApp prerequisites in the in-app Help view (Git, optional GitHub token, network).
- Fix tray install status by auto-installing bundled dependencies on first launch.

## Progress
- Added `--skip-grammar-checks` for Surreal indexing and validated Surreal index/pack with persistent store.
- Upgraded tree-sitter to 0.26 and switched TS/TSX/Swift adapters to the supported LanguageFn API (no grammar mismatch).
- Made Surreal pack/search/eval resolve repo-root paths and error when the store path is missing.
- Stabilized fragment IDs across files (path+span+content hash) to prevent collisions in embedded Surreal indexes.
- Cleaned build warnings in ce-cli/ce-store/ce-lang-* and verified Surreal smoke tests.
- DMG bundling now enforces required binaries and fails fast if any dependency is missing.
- Triaged Xcode build failure: `cargo` missing in build phase PATH; adding explicit cargo discovery and error messaging.
- Fixed `ce-cli` integration template include paths and rebuilt `cargo build -p ce-cli` (warnings remain).
- Updated Bundle Prune Binaries script to resolve the prune repo root and ran `xcodebuild` (warnings remain).
- Identified A2UIRuntime duplicate type definitions causing Swift compile failures.
- Unified A2UIRuntime model + helpers, fixed ContentView rendering, and restored Debug Xcode build with bundled binaries (cloudflared still required for Release DMG).
- Installed cloudflared via Homebrew and verified the binary is available for bundling.
- Added DMG Install.command to copy PruneApp into /Applications and launch it after install.
- Added prerequisites section to the Help view (Git/CLT, macOS version, network, GitHub token).
- Added runtime Git availability check with Help banner + CLT installer button.
- Switched menu bar settings navigation to SettingsLink.
- Auto-install bundled dependencies on first launch and updated quickstart guidance.
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
- Implemented embedded SurrealDB backend (store abstraction, Surreal store, CLI/MCP wiring, tests, docs).
- Ensured the dashboard window orders front when opened from the menu bar.

## Next Steps
- Validate Xcode build after updating cargo discovery in the Bundle Prune Binaries script.
- Integrate ce-lang-tsreact and TS/TSX indexing + ApiSummary refs in CLI.
- Port tsconfig alias resolution + JSX tag edges into `ce-store`.
- Update README + validation steps for TS/TSX support.

## Notes
- Local repo has uncommitted changes in `Cargo.toml`, `Cargo.lock`, `crates/prune-mcp/`, and `crates/prune-sync/` (left untouched).
