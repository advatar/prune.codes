Yes — **you can implement that exact “fusion pipeline” on top of Codex today**, but it won’t be “a single switch” inside Codex. It looks like:

- **Codex stays the agent loop + editor + command runner**
- You add a **Rust “context engine”** beside it that does:
  AST chunking → graph building → hybrid retrieval → ranking → token-budget packing → (optional) recursive decomposition
- You plug that engine into Codex via **MCP tools + a skill + AGENTS.md guidance**

Codex already has the right extension points:
- It can call tools (including MCP) while it works in a loop.  [oai_citation:0‡OpenAI Developers](https://developers.openai.com/codex/prompting/)  
- It supports **skills** (load tiny metadata always; load full instructions only when invoked).  [oai_citation:1‡OpenAI Developers](https://developers.openai.com/codex/skills/create-skill/)  
- It supports persistent guidance via **AGENTS.md** layering.  [oai_citation:2‡OpenAI Developers](https://developers.openai.com/codex/guides/agents-md/)  

So the answer is “**yes, with an external context engine**,” and optionally “**yes, even more tightly** if you’re willing to fork Codex CLI and wire your context engine into the request pipeline.”

---

## What Codex already does today (and what it doesn’t)

### Built-in compaction exists, but it’s not “AST/graph smart”
Codex explicitly says that for longer tasks it may “compact” by **summarizing relevant information and discarding less relevant details**, and that repeated compaction lets it keep going.  [oai_citation:3‡OpenAI Developers](https://developers.openai.com/codex/prompting/)

You can tune it via config:
- `model_auto_compact_token_limit` (threshold for auto compaction)  [oai_citation:4‡OpenAI Developers](https://developers.openai.com/codex/config-reference/)  
- `compact_prompt` / `experimental_compact_prompt_file` (override the compaction prompt)  [oai_citation:5‡OpenAI Developers](https://developers.openai.com/codex/config-reference/)  

But in practice, compaction is mostly **history rewriting + summary**, and people have observed it can drop important “assistant state” (e.g., confirmations like “Bug X is fixed”) because it rewrites history aggressively into a bridge of user prompts plus a summary.  [oai_citation:6‡GitHub](https://github.com/openai/codex/discussions/5799)

So: **Codex compaction helps you survive long sessions**, but it’s not the same as “program analysis driven minimal context.”

### Codex does *not* ship (today) with your desired context engine
Out of the box, Codex will read files, run commands, and accumulate context.  [oai_citation:7‡OpenAI Developers](https://developers.openai.com/codex/prompting/)  
It doesn’t natively do the full pipeline you described:
- repository-wide AST chunking
- code graphs / subgraph retrieval
- token-budget knapsack packing
- content-addressed fragment IDs
- RLM-style recursive decomposition as a first-class thing

That’s what you add.

---

## The “exact pipeline” you described, mapped to something you can build now

Here’s a concrete mapping of each component to a Codex-friendly implementation:

### 1) Graph index of the codebase
Build a **repository indexer** in Rust that produces:

- **Fragments**: language-aware chunks (functions, methods, types, modules)
- **Edges**: imports, symbol references, calls, overrides, “defined-in”, “used-by”
- **Metadata**: file path, symbol name, signature, docstring, test coverage, last-modified, etc.

Implementation notes:
- Parse syntax with **Tree-sitter** (fast, multi-language) for AST boundaries.
- For *semantic* edges (references/call graph), use **LSP servers** where possible:
  - Rust: `rust-analyzer`
  - TypeScript: `tsserver`
  - Python: `pyright`/`pylance`
  - Go: `gopls`
  - Java: `jdtls`
- Store graph in SQLite + adjacency tables, or use `petgraph` in-memory with persistence.

### 2) AST chunking (boundary-respecting fragments)
This is the “don’t cut in the middle of a function/class” piece.

Output fragments like:

- `fragment_id`
- `language`
- `file_path`
- `span` (start/end byte or line/col)
- `signature_only` (tiny)
- `body` (full)
- `imports_used`
- `symbols_defined`
- `symbols_referenced`

This is the core primitive that keeps context small *without losing essential structure*.

### 3) Hybrid retrieval to seed candidates
Given a user request (or error signature), produce candidate fragments using:

- **Lexical**: ripgrep/BM25 on identifiers, filenames, error strings
- **Semantic**: embeddings over fragment summaries (not always full bodies)
- **Structural**: “cursor-local neighborhood” (same file, same module, same package)

### 4) “Retrieve candidate subgraph”
Take the seed fragments and expand via graph traversal:

- include direct dependencies (imports, called functions, type definitions)
- include reverse edges if helpful (callers, implementers)
- stop at:
  - radius limit
  - token budget
  - diminishing relevance

### 5) Rank and pack into a token budget
Now you do the “knapsack” step:

- Score each candidate fragment by:
  - retrieval score (lexical + semantic)
  - graph distance
  - symbol overlap with query/error
  - recency (recently edited files)
  - “unit test proximity” (if the query is about failing tests)
- Pack into `budget_tokens` by `value / cost` until full.

The output of this step is a **Context Pack**.

### 6) RLM-style recursive breakdown when query is broad
This is implementable **without modifying the LLM**, which is exactly the RLM claim: treat the long prompt as an *external environment* that the model can inspect/decompose, and recursively call itself on snippets.  [oai_citation:8‡arXiv](https://arxiv.org/html/2512.24601v1)

In Codex terms, you approximate “RLM” by:

- Making the **context engine** expose tools like:
  - `peek(fragment_id, mode=signature|body|summary)`
  - `search_symbols(query)`
  - `expand_subgraph(seed_ids, radius)`
- Letting the agent do **multi-step tool use** to narrow scope before requesting full bodies.

If you want “true recursion” (subcalls that return intermediate summaries), implement a tool like:

- `rlm_subcall(task, fragment_ids)`  
  → server calls the model (or runs `codex exec` out-of-band) on each chunk and returns a condensed result.

That matches the RLM picture (“programmatically examine… and invoke itself recursively over snippets”).  [oai_citation:9‡arXiv](https://arxiv.org/html/2512.24601v1)

### 7) Content-addressable references to avoid repetition
Unison’s key idea is: **each definition is identified by a hash of its syntax tree** (content-addressed), so the hash pins the exact implementation and its dependencies.  [oai_citation:10‡unison-lang.org](https://www.unison-lang.org/docs/the-big-idea/)

You can apply this to any language:

- Normalize AST (rename locals to placeholders, normalize whitespace/comments)
- Hash the normalized AST (e.g., BLAKE3 or SHA-256)
- Use that as `fragment_id`

Then your Context Pack can say:

- “Fragment `F:abc123` (signature + 1–2 line summary)”
- only include full body if requested / needed

And if compaction nukes earlier bodies, the model can always re-fetch `F:abc123` via your tool.

---

## How to plug this into Codex *right now*

### A) No fork: MCP server + skill + AGENTS.md (fastest path)
This is the practical “do it today” route.

**1) Add your Rust MCP server**
Codex supports MCP servers in CLI and IDE extension, configured in `~/.codex/config.toml` or via `codex mcp add`.  [oai_citation:11‡OpenAI Developers](https://developers.openai.com/codex/mcp/)  
You can also restrict which tools are exposed via `enabled_tools` / `disabled_tools`.  [oai_citation:12‡OpenAI Developers](https://developers.openai.com/codex/config-reference/)

**2) Create a skill that forces the workflow**
Skills are Markdown with YAML front matter (`name`, `description` required).  [oai_citation:13‡OpenAI Developers](https://developers.openai.com/codex/skills/create-skill/)  
You write a skill like `$context-pack` whose instructions are essentially:

- “Before answering or editing, call `context.pack(query, budget)`”
- “Use returned pack; only request bodies via `fragment.get` if needed”
- “After failures, call `context.pack` again with the error output”

**3) Add AGENTS.md guardrails**
Codex reads `AGENTS.md` before doing work and builds an instruction chain (global + repo + subdir).  [oai_citation:14‡OpenAI Developers](https://developers.openai.com/codex/guides/agents-md/)  
So you can enforce rules like:
- “Never paste entire files; use the context engine tools”
- “Always include fragment IDs in reasoning and in edits”
- “Prefer signatures over bodies until necessary”

This “no fork” approach gets you 80% of the architecture with minimal disruption.

### B) Fork Codex CLI: make context packing automatic (more “exact”)
If you really mean “exactly this, always-on, without relying on the model to remember the skill,” you can fork Codex and:

- intercept each model call
- compute a Context Pack from:
  - current user request
  - current working directory / diff / failing tests
- inject that pack as developer/system context before sampling

This gives you:
- guaranteed usage
- consistent budgets
- consistent provenance (every snippet has a reason + ID)

It’s more work, but it’s the closest to “hard-wiring the pipeline into Codex.”

---

## The one big interaction you must design around: compaction

If you adopt content-addressed fragments + external retrieval, you can actually make compaction much less scary.

But you still have to choose:

### Option 1: Let Codex auto-compact, rely on external memory
Pros: hands-off  
Cons: compaction can drop assistant state and be hard to notice  [oai_citation:15‡GitHub](https://github.com/openai/codex/discussions/5799)

### Option 2: Raise/disable auto-compact, manage context yourself
Codex exposes `model_auto_compact_token_limit` to control when auto-compaction triggers.  [oai_citation:16‡OpenAI Developers](https://developers.openai.com/codex/config-reference/)  
Pros: you keep full history longer  
Cons: you must be strict about not inflating prompt size

### Option 3: Customize compaction prompt to preserve “state variables”
You can override compaction behavior via `compact_prompt` / `experimental_compact_prompt_file`.  [oai_citation:17‡OpenAI Developers](https://developers.openai.com/codex/config-reference/)  
A good trick is: **force the compactor to always retain a structured “state block”**, e.g.:

- current goal
- completed tasks (with evidence)
- fragment IDs that matter
- constraints / decisions
- “do not repeat” notes

Even if the rest is summarized, you keep the invariants.

---

## What you can build first (MVP that genuinely helps)

If you want maximum benefit quickly:

1) **AST chunker + fragment store** (Tree-sitter + hashes)  
2) **Hybrid retrieval** (rg/BM25 + embeddings)  
3) **Token-budget context packer**  
4) **A single MCP tool: `context.pack()`**  
5) **One skill to force calling it**

Then add:
- graph expansion
- RLM-style recursive decomposition
- content-addressed lazy expansion + caching

---

## TL;DR

- **Yes**, you can implement that fused context-engineering system with Codex today.
- You do it by building a **Rust MCP “context engine”** and teaching Codex to use it via **skills + AGENTS.md**.  [oai_citation:18‡OpenAI Developers](https://developers.openai.com/codex/mcp/)
- Codex’s built-in compaction helps, but it’s basically summary-based and can drop important state.  [oai_citation:19‡OpenAI Developers](https://developers.openai.com/codex/prompting/)
- RLM-style “model manages its own context” is implementable as an inference-time loop with tool calls (and optional subcalls), consistent with the RLM framing.  [oai_citation:20‡arXiv](https://arxiv.org/html/2512.24601v1)
- Unison-style content addressing is a great fit for dedup + stable fragment IDs.  [oai_citation:21‡unison-lang.org](https://www.unison-lang.org/docs/the-big-idea/)

If you want, I can outline a concrete **Rust MCP tool schema** (the exact tool names + JSON inputs/outputs) and a matching **`.codex/skills/context-pack/SKILL.md`** that will reliably steer Codex into this workflow.
