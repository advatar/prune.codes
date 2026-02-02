# DOCS-CR — Update Prune Docs (prune.codes/docs) for Templates, Inception, LSP, Memory, Codex/OpenCode

## Purpose
Rewrite the main documentation page at **/docs** to reflect the current Prune feature set, with emphasis on **Codex** and **OpenCode** integrations.

This is a content-first documentation update: new structure, new sections, updated command examples, and “how it works” explanations.

---

## Desired outcome
A single docs page (with anchors + table of contents) that answers:

1. What is Prune and when should I use it?
2. How do I install Prune and run it on my repo?
3. How do I bootstrap a new project with templates during inception?
4. How do I connect Prune to Codex and OpenCode via MCP?
5. How do AST slicing, symbol graphs, and context pruning work?
6. How do I enable optional LSP semantic resolution?
7. How do I enable persistent memory (Prune Memory or external provider)?
8. How do I measure token reduction and evaluate strategies?

---

## IA / Outline (single page)

### 1) Hero + Quickstart
- 3-step flow:
  1. Install
  2. Bootstrap repo (choose template)
  3. Connect agent (Codex/OpenCode)

### 2) Core concepts
- Fragments
- Packs
- Strategies (AST slicing, import graph, symbol graph, API summaries)
- Why smaller models benefit

### 3) Install
- CLI install (cargo)
- PruneApp install (macOS app)
- “Doctor” checks

### 4) Inception (PruneApp)
- A2UI guided interview
- Template selection per workspace:
  - Web (React/Vite/Tailwind + Supabase Edge Functions)
  - Mobile (Xcode multi-platform SwiftUI)
  - Rust (library)
  - Rust (CLI)
- Outputs written to `.prune/`:
  - project profile
  - preferences
  - bootstrap plan

### 5) Bootstrap & index
- `ce bootstrap`
- `ce index`
- `ce pack`

### 6) Agent integrations (focus)
#### Codex
- Add MCP server to `.codex/config.toml`
- Add `AGENTS.md` with Prune usage contract
- “Before edits” / “after changes” lifecycle

#### OpenCode
- Add MCP server to `opencode.json`
- Add rule/instructions files
- Optional plugin to automate recall/remember hooks

### 7) Optional: LSP semantic resolver
- What it fixes: go-to-definition precision for TS/Swift/Rust
- How to enable:
  - `.prune/lsp.json`
  - `ce lsp doctor`
  - `ce pack --lsp auto`

### 8) Optional: Persistent memory
- When to use memory
- Built-in Prune Memory:
  - `memory.recall`, `memory.remember`
- External memory provider (advanced)

### 9) Measuring token reduction
- Report “raw selected fragments tokens” vs “final pack tokens”
- Show examples of CLI output and metrics file

### 10) Troubleshooting
- MCP server not appearing
- index too slow
- pack misses a definition (enable LSP)
- pack too large (strategy knobs)

---

## Exact replacement copy for /docs

> Paste the following content into the /docs page (render as markdown with code blocks and anchor links).

---

# Prune Documentation

Prune turns a codebase into **minimal, high-signal context packs** so coding agents stop guessing and start making correct changes with smaller context windows.

Prune is not an LLM. It’s the **Context Engine** that sits next to your agent (Codex/OpenCode) and decides **what code matters right now**.

---

## Quickstart

### 1) Install
```bash
cargo install --path .
# or: cargo install prune-ce (when published)
```

### 2) Bootstrap your workspace
If you’re starting a new project, Prune can scaffold a template and analyze it immediately:

```bash
ce bootstrap --template web
ce index
```

If you already have a repo:

```bash
ce index
```

### 3) Connect an agent and run a pack
Start the MCP server:

```bash
ce mcp serve
```

Then connect either Codex or OpenCode (see below), and ask the agent to use the `pack` tool before making edits.

---

## How Prune works

### Fragments
Prune splits your repo into small “fragments”:
- function signatures
- API summaries
- type definitions
- minimal slices of implementation
- error-targeted spans (line ranges)

### Graphs
Prune builds graphs that help it include only what’s needed:
- file import graph
- symbol/reference edges
- fragment containment (file → fragment)

### Packs
A pack is the final context payload sent to the agent:
- small enough to fit strict budgets
- coherent (connected by the graph)
- formatted for maximum model comprehension

---

## Install and verify

### CLI
Install via Cargo:

```bash
cargo install --path .
```

Run doctor checks:

```bash
ce doctor
```

---

## Inception with PruneApp

PruneApp provides an inception flow that:
1) asks a short set of product + engineering preference questions (A2UI)
2) lets you choose a workspace template (web / mobile / rust)
3) boots the template and runs the first analysis automatically

Templates supported:
- **Web**: React/Vite/Tailwind + Supabase Edge Functions
- **Mobile**: Xcode multi-platform SwiftUI (Swift 6)
- **Rust**: library
- **Rust CLI**: binary + commands layout

Artifacts written to `.prune/`:
- project profile + preferences
- template selection
- bootstrap plan + index metadata

---

## Bootstrap and index

### New project (template-first)
```bash
ce bootstrap --template web
ce index
```

### Existing project
```bash
ce index
```

### Generate a pack
```bash
ce pack --query "Implement a login form and connect to Supabase"
```

---

## Integrations

Prune is agent-agnostic. Agents are “clients”. The core integration is always:

1) Agent starts
2) Agent uses Prune tools (via MCP) to get the minimal pack
3) Agent edits code
4) Repeat as repo changes

---

### Codex integration

Codex supports MCP servers configured in `~/.codex/config.toml` or a project-scoped `.codex/config.toml`.

**Project-scoped `.codex/config.toml`:**
```toml
[mcp_servers.prune]
command = "ce"
args = ["mcp", "serve"]
cwd = "."
startup_timeout_sec = 20
tool_timeout_sec = 60
enabled = true
```

Add `AGENTS.md` to teach Codex the workflow:
- before making changes: `repo.ensure_fresh` then `pack`
- after changes: run tests, then pack again if needed
- when stuck on “unbound names”: request a new pack with stronger definition support

---

### OpenCode integration

OpenCode supports MCP servers in `opencode.json` under the `mcp` key.

**`.opencode/opencode.json`:**
```jsonc
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "prune": {
      "type": "local",
      "command": ["ce", "mcp", "serve"],
      "enabled": true,
      "timeout": 20000
    }
  }
}
```

Optional: add an OpenCode plugin to automate “recall memory at session start” and “remember decisions at session end”.

---

## Optional: LSP semantic resolver

When syntax-only resolution is not enough (TSX JSX tags, Swift extensions, Rust traits/macros), Prune can call Language Servers on demand.

Enable:
1) create `.prune/lsp.json`
2) run:
```bash
ce lsp doctor
```
3) pack with:
```bash
ce pack --lsp auto --query "Fix the failing test"
```

This yields tighter packs by resolving the *correct* definition targets.

---

## Optional: Persistent memory

Prune can store “what we learned” across tasks:
- architecture decisions
- repo conventions
- repeated bug patterns and their fixes
- preferred libraries and tradeoffs

### Remember a decision
```bash
ce memory remember "Prefer Supabase Edge Functions for server logic; avoid adding a custom backend." --project myapp --tags inception,architecture
```

### Recall before starting work
```bash
ce memory recall "auth flow" --project myapp --k 12 --token-budget 800
```

---

## Measuring token reduction

Prune reports:
- tokens in candidate retrieval set
- tokens after AST slicing + graph pruning
- final pack tokens

Use these metrics to tune strategies and benchmark combinations.

---

## Troubleshooting

### Prune tools do not show up in the agent
- verify MCP server config
- run `ce mcp doctor`
- run the agent again after config changes

### Pack misses a definition
- enable LSP mode (`--lsp auto`)
- increase definition support depth

### Pack is too large
- reduce pack token budget
- prefer summaries/signatures over bodies
- limit neighborhood expansion

---

## End of replacement copy

---

## Lovable.dev implementation instructions
- Update the /docs route to use this new content.
- Add a sticky table-of-contents that anchors to the sections above.
- Provide “copy” buttons on code blocks.
- Ensure headings have stable anchor IDs.
- Keep links clickable and consistent.

