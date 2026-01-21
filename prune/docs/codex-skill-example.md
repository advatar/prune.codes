# Codex skill example: always use Context Engine

Create `.codex/skills/context-pack/SKILL.md` inside your project repo:

```md
---
name: context-pack
description: Always fetch a minimal context pack from the Context Engine before editing.
---

# Workflow

1. At the start of any task, call `context.pack` with:
   - the task request
   - any failing test output / error logs (if available)
   - a reasonable budget (e.g. 8k–16k chars)
   - optionally, a `strategy_id` (stored in the Context Engine DB) to select a tuned retrieval/packing behavior
   - optionally, a `session_id` to avoid repeating fragments you have already seen in the same task thread
2. Use the returned pack as your working context.
3. If you need more detail on a deferred fragment, call `fragment.get` for that fragment id (start with `view: "slice"`, then escalate to `view: "body"` if needed).
4. After you fix something and rerun tests, repeat `context.pack` with the new error output if still failing.
```

This repo ships an MCP server (`ce-mcp`) that exposes:
- `context.pack`
- `context.search`
- `fragment.get`
 - `strategy.list`
 - `strategy.get`

