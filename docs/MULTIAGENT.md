Yes — you can support **Codex CLI**, **OpenCode**, and **Claude Code** from the **same Prune repository** without forking any of them, by treating Prune as an **agent-agnostic “context & repo intelligence” service** and then shipping **thin “adapters”** (config + instruction files + optional hooks/plugins) for each client.

Below is a concrete plan that’s practical, incremental, and keeps *everything authored/maintained in Prune*.

---

## The core idea: Prune stays the same, agents become “clients”

You don’t want to “be” the agent — you want to be the **best possible context provider** and **repo state manager** (index/graph/pack/sync), and let different agents call you.

That maps cleanly to two integration layers:

1. **MCP Tool Layer (universal):**  
   Prune exposes a stable set of MCP tools (e.g. `repo.ensure_fresh`, `repo.sync`, `ce.index`, `ce.pack`, `metrics.token_savings`, etc.).  
   All three agents can connect to MCP servers.  
   - Codex: MCP configured in `~/.codex/config.toml`.  [oai_citation:0‡OpenAI Developers](https://developers.openai.com/codex/mcp/)  
   - OpenCode: MCP servers configured in `opencode.json` under `mcp`, supports local+remote.  [oai_citation:1‡OpenCode](https://opencode.ai/docs/mcp-servers/)  
   - Claude Code: MCP servers configured in `.mcp.json` (project) or user config, supports env var expansion in config.  [oai_citation:2‡Claude Code](https://code.claude.com/docs/en/mcp)  

2. **Behavior Layer (per-agent):**  
   You *teach* the agent how/when to use Prune via:
   - **AGENTS.md** (Codex + OpenCode)
   - **CLAUDE.md** (Claude Code; OpenCode can fall back to it)
   - **SKILL.md skills** (Claude Code + OpenCode) for progressive disclosure + “auto-trigger” behavior

Codex explicitly reads `AGENTS.md` before work and layers it by directory.  [oai_citation:3‡OpenAI Developers](https://developers.openai.com/codex/guides/agents-md/)  
OpenCode uses `AGENTS.md` as its rules file, and supports Claude Code conventions as fallbacks (including `CLAUDE.md` + `.claude/skills`).  [oai_citation:4‡OpenCode](https://opencode.ai/docs/rules/?utm_source=chatgpt.com)  
Claude Code supports skills in `.claude/skills/` (project) or `~/.claude/skills/` (personal).  [oai_citation:5‡Claude Code](https://code.claude.com/docs/en/skills)  

---

## What “supporting all three agents” means in practice

### Deliverable A: A single “Prune Integration Pack” inside the Prune repo
Add an `integrations/` folder (in Prune repo) that contains:

- **Instruction sources of truth**
  - `integrations/instructions/prune.AGENTS.md.template`
  - `integrations/instructions/prune.CLAUDE.md.template`
  - (optional) `integrations/instructions/prune.common.md` (shared sections)
- **Skills**
  - `integrations/skills/prune-context/SKILL.md`  
    (placed into `.claude/skills/prune-context/SKILL.md` in target repos)
  - Support files for progressive disclosure (examples, checklists, “pack contract”, etc.)
- **Agent-specific config templates**
  - `integrations/codex/config.toml.snippet`
  - `integrations/opencode/opencode.json.snippet`
  - `integrations/claude/.mcp.json.template`
- **Optional automation**
  - `integrations/opencode/plugins/prune_auto_context.ts` (OpenCode plugin)
  - `integrations/claude/hooks/prune_inject_context.json` (Claude hooks template)

This is how you keep everything in **one** Prune repo, yet target multiple agents.

---

## The minimum viable integration for each agent

### 1) Codex CLI integration
**What Codex needs:**
- An MCP server entry in `~/.codex/config.toml` using `[mcp_servers.<name>]` tables.  [oai_citation:6‡OpenAI Developers](https://developers.openai.com/codex/mcp/)  
- An `AGENTS.md` in the target codebase (plus optional global `~/.codex/AGENTS.md`). Codex layers these and has discovery/size limits.  [oai_citation:7‡OpenAI Developers](https://developers.openai.com/codex/guides/agents-md/)  

**Plan:**
- Prune adds a command:  
  `prune integrate codex --repo <path>`  
  which will:
  1) Print (or patch) the TOML snippet for MCP config  
  2) Generate `AGENTS.md` into the target repo root (if missing) using template(s)  
  3) Optionally generate nested `AGENTS.md` files for large monorepos (only where it helps)

**Codex MCP sample (snippet):**  [oai_citation:8‡OpenAI Developers](https://developers.openai.com/codex/mcp/)
```toml
# ~/.codex/config.toml

[mcp_servers.prune]
command = "prune"
args = ["mcp", "serve", "--stdio"]
# cwd = "/path/to/repo"  # optional if you want to force a repo
enabled = true
```

**Behavior instruction placement:**
- Put Prune usage rules in the repo’s `AGENTS.md` (and optionally nested overrides) because Codex merges them in precedence order.  [oai_citation:9‡OpenAI Developers](https://developers.openai.com/codex/guides/agents-md/)  

---

### 2) OpenCode integration
**What OpenCode needs:**
- An `opencode.json` with an `mcp` object; supports **local** (command array) and **remote** MCP servers.  [oai_citation:10‡OpenCode](https://opencode.ai/docs/mcp-servers/)  
- A project `AGENTS.md` rules file (or fallback `CLAUDE.md`).  [oai_citation:11‡OpenCode](https://opencode.ai/docs/rules/?utm_source=chatgpt.com)  
- Optional: plugins in `.opencode/plugins/` to enforce or enhance workflows.  [oai_citation:12‡OpenCode](https://opencode.ai/docs/plugins/)  

**Plan:**
- Prune adds:
  `prune integrate opencode --repo <path> [--write-opencode-json]`
  which will:
  1) Create/patch `<repo>/opencode.json` to add MCP server `prune`  
  2) Ensure `<repo>/AGENTS.md` exists (or create `<repo>/CLAUDE.md` fallback too)
  3) Drop `.claude/skills/prune-context/` into the repo (so OpenCode can discover it via Claude compatibility as needed)  [oai_citation:13‡OpenCode](https://opencode.ai/docs/rules/?utm_source=chatgpt.com)  

**OpenCode MCP local server example:**  [oai_citation:14‡OpenCode](https://opencode.ai/docs/mcp-servers/)
```jsonc
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "prune": {
      "type": "local",
      "command": ["prune", "mcp", "serve", "--stdio"],
      "enabled": true
    }
  }
}
```

**Optional (recommended) OpenCode plugin path:**
OpenCode loads plugins from `.opencode/plugins/` (project) or `~/.config/opencode/plugins/` (global).  [oai_citation:15‡OpenCode](https://opencode.ai/docs/plugins/)  

You can ship a plugin that:
- listens to message/session events (e.g., session created / diff updates)  [oai_citation:16‡OpenCode](https://opencode.ai/docs/plugins/)  
- and/or wraps tool usage (via `tool.execute.before/after`)  [oai_citation:17‡OpenCode](https://opencode.ai/docs/plugins/)  
- and auto-runs `prune pack` at strategic times.

Even if you **don’t** auto-inject context at first, a plugin can still:
- enforce “always call Prune pack before edits”
- collect metrics (token reduction, pack hit rate, etc.)
- store run logs for your DGM-style evolution loop.

---

### 3) Claude Code integration
**What Claude Code needs:**
- `.mcp.json` with `mcpServers` definitions to connect to MCP.  [oai_citation:18‡Claude Code](https://code.claude.com/docs/en/mcp?utm_source=chatgpt.com)  
- Skills in `.claude/skills/` (project) or `~/.claude/skills/` (personal).  [oai_citation:19‡Claude Code](https://code.claude.com/docs/en/skills)  
- A `CLAUDE.md` for persistent project instructions.  [oai_citation:20‡Claude Code](https://code.claude.com/docs/en/skills)  
- Optional: hooks for deterministic enforcement/automation (PreToolUse, PostToolUse, etc.).  [oai_citation:21‡Claude Code](https://code.claude.com/docs/en/hooks?utm_source=chatgpt.com)  

**Plan:**
- Prune adds:
  `prune integrate claude --repo <path>`
  which will:
  1) Create `<repo>/.mcp.json` defining a `prune` MCP server
  2) Create `<repo>/CLAUDE.md` (or generate from same templates as AGENTS.md)
  3) Install `.claude/skills/prune-context/SKILL.md` (project-level)

**Claude MCP config example:**  [oai_citation:22‡Claude Code](https://code.claude.com/docs/en/mcp?utm_source=chatgpt.com)
```json
{
  "mcpServers": {
    "prune": {
      "command": "prune",
      "args": ["mcp", "serve", "--stdio"]
    }
  }
}
```

**Skill location is first-class in Claude Code** (project `.claude/skills/`).  [oai_citation:23‡Claude Code](https://code.claude.com/docs/en/skills)  
This is the best place to encode “when to call Prune, what to do with the pack, how to keep context small”.

---

## Unifying AGENTS.md vs CLAUDE.md vs SKILL.md so you don’t duplicate work

You want **one** source of truth.

### Recommended pattern
- Treat **AGENTS.md** as the canonical “always loaded rules” doc (because Codex relies on it heavily).  [oai_citation:24‡OpenAI Developers](https://developers.openai.com/codex/guides/agents-md/)  
- Generate **CLAUDE.md** as either:
  - a near-duplicate (safe, deterministic), or
  - a short shim pointing to AGENTS.md (lighter but depends on Claude’s file-reading habits)

OpenCode explicitly supports Claude file conventions as fallbacks, so shipping both makes you resilient.  [oai_citation:25‡OpenCode](https://opencode.ai/docs/rules/?utm_source=chatgpt.com)  

### Skills are your “high leverage” layer
Skills are designed for progressive disclosure and automation-style behavior (triggered by the model based on description). Claude Code describes this model-invoked workflow and where skills live.  [oai_citation:26‡Claude Code](https://code.claude.com/docs/en/skills)  

Because OpenCode supports `.claude/skills` as Claude-compatible, you can often ship **one set of skills** that works in both Claude Code and OpenCode.  [oai_citation:27‡OpenCode](https://opencode.ai/docs/rules/?utm_source=chatgpt.com)  

---

## The “agent selection” UX you asked for: CLI + PruneApp

### In the Prune CLI
Add two core commands:

1) `prune integrate <codex|opencode|claude> --repo <path> [--global]`
- Writes configs + instruction files + skills into the right place
- Prints what it changed
- Supports `--dry-run`

2) `prune run --agent <codex|opencode|claude> [--repo <path>]`
- Convenience wrapper that launches the agent after ensuring integration is present
- Optional: start/stop your MCP server in the same “session”

### In the PruneApp (SwiftUI)
Add an “Agents” screen with:
- Agent picker: Codex / OpenCode / Claude Code
- Status checks:
  - “MCP server reachable”
  - “Instruction files found”
  - “Skill installed”
- Buttons:
  - Install integration
  - Open agent (launch CLI)
  - Copy “manual install snippet” to clipboard

No forks required.

---

## The optional “next level”: deterministic enforcement, not just prompting

If you want Prune usage to be **reliable**, you eventually want *enforcement*, not only instructions.

### OpenCode: plugins
OpenCode plugins can be loaded from `.opencode/plugins/` and hook tool execution (before/after), subscribe to session/message events, etc.  [oai_citation:28‡OpenCode](https://opencode.ai/docs/plugins/)  

That means you can implement:
- **Auto-pack before risky actions**: on `tool.execute.before` when `edit/write/bash` is requested, ensure a fresh Prune pack exists (or block and instruct).  [oai_citation:29‡OpenCode](https://opencode.ai/docs/plugins/)  
- **Auto-metrics**: compute token reduction per message/session
- **Auto-eval logging**: save traces for your DGM evolution loop

### Claude Code: hooks + skills
Claude Code supports hooks (pre/post tool lifecycle) for deterministic behavior.  [oai_citation:30‡Claude Code](https://code.claude.com/docs/en/hooks?utm_source=chatgpt.com)  
So you can:
- Prevent edits unless a “current pack” marker exists
- Auto-run `prune pack` on session start or before edit tools
- Automatically log “pack coverage” metrics

This can become the backbone of “evaluate combinations” without relying on the model’s discipline.

---

## Concrete build order for Prune to reach “multi-agent support” cleanly

### Step 1 — Make Prune MCP transport & toolset “client-proof”
- Ensure `prune mcp serve --stdio` works with:
  - multiple concurrent tool calls
  - robust working-directory behavior
- Provide a tiny tool surface:
  - avoid adding tons of tools (OpenCode warns MCP tools add context overhead)  [oai_citation:31‡OpenCode](https://opencode.ai/docs/mcp-servers/)  

### Step 2 — Add `prune integrate …` (template-based)
- Templates stored in Prune repo
- A generator that can:
  - patch JSON (`opencode.json`, `.mcp.json`)
  - patch TOML (`~/.codex/config.toml`)
  - create `AGENTS.md` / `CLAUDE.md` / `.claude/skills/*`

### Step 3 — Add `prune doctor`
- Detect:
  - agent installed?
  - MCP config present?
  - repo instruction present?
  - skills installed?
- Print “what to do next” in one screen

### Step 4 — Optional enforcement
- OpenCode plugin shipped with Prune (in-repo)
- Claude hooks templates shipped with Prune (in-repo)

### Step 5 — Evaluation compatibility
- Ensure the same Prune packing workflow is callable from all three agents so your SWE-bench runner can compare agent behavior apples-to-apples.

---

## Quick reference links (clickable)
```text
https://github.com/openai/codex
https://developers.openai.com/codex/mcp/
https://developers.openai.com/codex/guides/agents-md/

https://github.com/anomalyco/opencode
https://opencode.ai/docs/mcp-servers/
https://opencode.ai/docs/rules/
https://opencode.ai/docs/plugins/

https://code.claude.com/docs/en/mcp/
https://code.claude.com/docs/en/skills
https://code.claude.com/docs/en/hooks
```

---

If you want the next step as a **single, concrete change request**: I’d implement `prune integrate` + `prune doctor` first (because it unlocks all three agents immediately), then add the **OpenCode plugin** and **Claude hooks** as opt-in “enforcement modules”.
