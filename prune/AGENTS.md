# Prune Agent Instructions (AGENTS.md)

These instructions apply to agents that read **AGENTS.md** (e.g., Codex CLI, OpenCode).

## Always do this
- Before making edits, get a **Context Pack** from Prune via the MCP tools.
- Prefer the smallest sufficient view: **signatures → slices → full bodies**.
- If the repo might have changed upstream, ensure Prune is synced/indexed before requesting context.

## Prune MCP usage policy
1) Call `repo.ensure_fresh`.
2) Call `context.pack` with the task or failure log.
3) Only fetch additional detail using `fragment.get(view="slice")` first; escalate to `view="body"` only if needed.

## After changes
- Run tests/build.
- If errors remain, repeat: ensure_fresh → pack → minimal change.


## Cortex Memory (external MCP)
- Before planning: call `cortex_recall` with a short query and a small limit (top 5).
- Before edits: call `repo.ensure_fresh`, then `context.pack` from Prune.
- After changes: run tests/build; on failures, re-pack with the error first.
- After finishing: call `cortex_remember` with key decisions, constraints, and commands.
- For long tasks: call `cortex_save` at milestones.
- Keep recalls concise; do not dump the full memory DB or large code blobs.
