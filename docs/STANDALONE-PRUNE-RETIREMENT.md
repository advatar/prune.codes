# Standalone Prune Retirement

`advatar/prune.codes` is the canonical production repository for Prune. It owns
the macOS app, release packaging, product documentation, and the embedded engine
workspace at `prune.codes/prune`.

The former standalone repository, `advatar/prune`, is retired as an active
development target. Do not land new engine, app, release, or product work there.

## What Was Preserved

The standalone repository had no source code that should remain unique. Its
standalone-only files were reviewed with this disposition:

| File | Disposition |
| --- | --- |
| `.gitignore` | Ignore rules are already covered in `prune.codes` and `prune.codes/prune`. |
| `INCEPTION_NEXT.md` | A2UI v0.8/v0.9 adapter requirements are implemented in `prune.codes/prune/A2UIRuntime` and documented in `prune.codes/prune/docs/A2UI-VERSIONING.md`. |
| `INSTALL.md` | Obsolete Context Engine v21 installation notes. Current setup belongs in `prune.codes/prune/README.md` and app release docs. |
| `LOVABLE_PROMPT.md` | Scratch webhook setup prompt. Productized webhook/tunnel behavior belongs in PruneApp and release docs. |
| `LOVABLE_STACK` | Duplicate of the Lovable webhook scratch prompt. |
| `PRUNE_PROMPT.md` | Long product/landing-page prompt. Useful only as narrative background, not as canonical engineering docs. |
| `STATUS.md` | Historical standalone task log. Canonical status now lives in `prune.codes/STATUS.md`. |
| `VALIDATE.md` | Validation checklist preserved below. |

## Preserved Validation Checklist

Use these checks from `prune.codes`:

```sh
cd prune && cargo test
cd prune/A2UIRuntime && swift test
xcodebuild -project PruneApp/PruneApp.xcodeproj -scheme PruneApp -configuration Debug -destination 'platform=macOS' build
./scripts/check-engine-overlap.sh --strict
```

Manual A2UI smoke checks:

- Open PruneApp settings and run the v0.9 fixture.
- Open PruneApp settings and run the v0.8 fixture.
- Load a JSONL fixture and verify the surface updates.
- Start a JSONL stream endpoint and verify live updates.

Optional engine smoke checks:

```sh
cd prune
cargo run -p ce-cli -- pack --db <db> --hnsw_dir <dir> --task "<task>"
scripts/codex/ce-mcp.sh
```

## Retirement Rule

If a sibling checkout exists at `../prune`, it should either be absent or contain
only a retirement pointer. `scripts/check-engine-overlap.sh` treats a retired
pointer checkout as skipped. If `../prune` is ever revived as a real engine repo,
it must be mechanically synchronized or converted to a submodule before active
development resumes.
