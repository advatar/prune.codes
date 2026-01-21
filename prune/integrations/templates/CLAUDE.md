# Prune Agent Instructions (CLAUDE.md)

These instructions apply to agents that read **CLAUDE.md** (Claude Code) and compatible clients.

## Required workflow
1) Call `repo.ensure_fresh` before generating any context pack.
2) Call `context.pack` with the full task/failure text.
3) Use deferred items and request `fragment.get(view="slice")` before asking for full bodies.
4) After pushing a commit, call `repo.sync(expected_sha=...)`.

## Context discipline
- Do not paste whole files unless explicitly required.
- Prefer signatures, then minimal slices, then full bodies.
