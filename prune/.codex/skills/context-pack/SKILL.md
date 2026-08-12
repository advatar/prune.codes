---
name: context-pack
description: Use the Context Engine MCP tools to retrieve minimal, relevant code context before editing.
---

# Workflow

## Always do this first
1) Call `context.pack` with:
- task: the user request or error log
- budget_tokens: 2500–4000
- max_bodies: 1–3
- format: "text"

2) Use the returned pack as the only code context unless more is needed.

The pack reports its automatic strategy selection and any `missing_links`. Treat missing links as explicit evidence gaps and fetch the listed deferred fragments when required.

Relevant repository decisions and golden paths are included automatically. Use `memory.add` when a durable architectural decision or verified golden path should guide future work.

## If more detail is needed
Prefer:
- `fragment.get(view="slice")` for a deferred fragment

Only escalate to:
- `fragment.get(view="body")`
when the slice is insufficient.

## After any failed build/test
Call `context.pack` again with the new error output.
