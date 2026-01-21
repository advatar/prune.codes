---
name: prune-context
description: Use Prune to generate minimal, high-signal context packs before editing code.
---

# Prune context workflow

## Before editing
1) Call `repo.ensure_fresh`.
2) Call `context.pack` with the task or failure log.

## Progressive disclosure
- Start with signatures.
- Use `fragment.get(view="slice")` for deferred items.
- Only ...
