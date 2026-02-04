# Status

## Current Task
- Keep the CLI fully standalone while PruneApp manages bundled binaries (PruneApp augments, not replaces, manual CLI workflows).
- Validate Xcode build after cargo discovery changes in the Bundle Prune Binaries script.
- Implement CR-003 LSP on-demand semantic resolver (ce-lsp crate, CLI wiring, pack hooks).
- Integrate TS/TSX indexing (ce-lang-tsreact) plus tsconfig alias resolution + JSX tag edges in ce-store.
- Update README + validation steps for TS/TSX support.
- Fix ce-mcp Surreal pack strategy borrow-after-move error.
- Reduce health-check timeouts so menu bar actions don’t feel blocked.

## Progress
- Implemented CR-006: Context7 integration, optional docs provider, new ce-docs crate, docs CLI/MCP tools, and pack injection.
- Built Prune CLI locally (`prune/target/release/ce`) via `cargo build --release -p ce-cli`.
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
- Set PRUNE_CLOUDFLARED_PATH in PruneApp build settings to the Homebrew cloudflared path.
- Ensured the dashboard window is brought to the front with retries after opening from the menu bar.
- Converted the Setup view to an A2UI-driven surface with binding-backed controls and actions.
- Added a folder picker action to the Setup A2UI surface for choosing a repo.
- Added editable question/answer overrides in the Inception interview (manual Q/A + followup edits).
- Made cloudflared bundling fall back to PATH/Homebrew when no explicit path is set.
- Tightened dashboard window activation so it is raised to the front on menu bar open.
- Converted all PruneApp UI to A2UI-driven surfaces (menu bar, settings nav, and all tabs).
- Extended the A2UI renderer to support SecureField/read-only inputs and multiline sizing.
- Added LLM-driven A2UI render validation and a no-repo A2UI sheet for inception flows.
- Let Install.command and the runtime installer locate system cloudflared when the bundle is missing it.
- Routed A2UI interactions through userAction → LLM responses with action requests + data-model baselines.
- Fixed A2UI starter catalog SwiftUI builder errors and gated Surreal-only Rust code to clear ce-cli/ce-mcp warnings.
- Enabled Surreal features for bundled ce binaries and expanded cloudflared discovery in bundle/install/runtime.
- Polished the menu bar popover layout and added action fallbacks when the LLM omits action requests.
- Updated `prune/README.md` with current language packs, store backends, strategy presets, and slicing modes.
- Refreshed `docs/REACT.md` to reflect current TS/TSX/JS support, edges, and CLI flows.
- Implemented CR-004 Prune Memory: new `ce-memory` crate, `.prune/memory.json`, CLI `ce memory` commands, MCP `memory.*` tools, and Codex/OpenCode integration preset.
- Fixed Surreal pack strategy ownership in ce-cli so bundle-binaries builds succeed.
- Implemented CR-005 Cortex external provider mode: vendor install/update/doctor, Codex/OpenCode integration for two MCP servers, OpenCode wrapper, AGENTS guidance, docs updates, and integration tests.
- Ran `cargo test -p ce-cli`.
- Converted Cortex vendor checkout to a git submodule at `prune/.prune/vendors/cortex`.
- Fixed ce-mcp Surreal pack strategy move to avoid borrow-after-move errors.
- Fixed menu bar actions to dispatch immediately (bypass LLM latency) while still allowing A2UI updates.
- Shortened health-check timeouts to avoid menu bar stalls when localhost endpoints are down.

## Next Steps
- Run Xcode build validation after bundle script changes and recheck DMG bundling.
- Extend TS/TSX indexing to include ApiSummary refs in CLI.
- Verify TS/TSX alias/JSX edges end-to-end in `ce-store` with a sample repo.

## Notes
- Keep Cortex vendor updates explicit (`ce vendor update cortex`) to avoid unexpected network calls during integration.
