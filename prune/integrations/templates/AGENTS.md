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
