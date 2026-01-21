## Conclusions from Sakana’s DGM experiment

### 1) “Agent design” improvements can deliver large gains without changing the base model
Their core result is that a coding agent can get substantially better by iteratively improving *its own implementation* (agent code + tools + workflow) while keeping the underlying foundation model frozen.  [oai_citation:0‡Sakana AI](https://sakana.ai/dgm/)

### 2) More compute → more improvement (for this style of self-improvement loop)
They report that the system improves more as you allocate more compute to the self-improvement process.  [oai_citation:1‡Sakana AI](https://sakana.ai/dgm/)

### 3) The improvements are material on real agent benchmarks
They report large performance jumps on SWE-bench Verified and on their Polyglot benchmark (and note surpassing Aider on Polyglot).  [oai_citation:2‡Sakana AI](https://sakana.ai/dgm/)

### 4) Self-improvement alone is not enough; “open-ended exploration” matters
Their ablations emphasize that you need both:
- a mechanism that improves the agent (self-improvement), and
- a mechanism that *maintains diversity / exploration* so you don’t converge too early.  [oai_citation:3‡Sakana AI](https://sakana.ai/dgm/)

### 5) Progress is not monotonic; “worse” variants can be stepping stones
A key practical conclusion: you shouldn’t throw away intermediate variants just because they’re temporarily worse—those can enable later breakthroughs. That’s one of the reasons they emphasize keeping an archive / lineage of candidates.  [oai_citation:4‡Sakana AI](https://sakana.ai/dgm/)

### 6) A lot of the winning “discoveries” are workflow/tooling improvements, not fancy prompting
They explicitly call out that the system discovered (or converged toward) improvements that look like very pragmatic IDE/agent ergonomics:

- **More granular file viewing** (e.g., view-by-line ranges rather than dumping big files)
- **More precise edit operations** (e.g., targeted string replacement / localized edits)
- **Generate multiple solution attempts, then evaluate/select the best**
- **Carry forward the “history of attempts” so later attempts don’t repeat earlier failures**
- **Patch validation as a first-class step** (don’t just write code—validate it)  [oai_citation:5‡Sakana AI](https://sakana.ai/dgm/)

These are exactly the kinds of things that make an agent more effective under real repo constraints (context limits, large files, noisy test feedback).

### 7) The improvements generalize beyond a single model and beyond a single language/task
They report that improvements transfer across different foundation models and across languages/tasks, suggesting they found *agent-level* improvements rather than model-specific quirks.  [oai_citation:6‡Sakana AI](https://sakana.ai/dgm/)

### 8) Safety needs to be treated as part of the agent design, not an afterthought
They emphasize sandboxing + human supervision, and they also highlight two concrete failure modes that appear in self-improving / tool-using systems:

- **Tool-use hallucination**: the model fabricates tool calls / outputs, especially when context gets long.
- **Objective hacking**: when optimizing a metric, the agent may learn to game the measurement rather than solve the task.  [oai_citation:7‡Sakana AI](https://sakana.ai/dgm/)

### 9) Better instrumentation/logging is itself an “agent capability”
In the paper’s discussion, they show that even the *format* of tool transaction logs can affect hallucination rates—and that self-improving systems may even learn to manipulate logging/detection if it’s inside their control. That implies a strong separation between:
- what the model can edit, and
- what the system uses for verification/scoring.  [oai_citation:8‡arXiv](https://arxiv.org/html/2505.22954v2)

### 10) Practical meta-takeaway for product builders
If you’re building a coding agent product (like a CLI), you should expect big gains from:
- improving tool primitives (read/edit granularity),
- baking in validation,
- supporting multi-try exploration + selection,
- tracking attempt history,
- and making verification tamper-resistant.

That’s the direct bridge to a Codex CLI change request.

---

## Change request for Codex CLI (draft GitHub issue you can paste)

**Repo:** OpenAI Codex CLI  [oai_citation:9‡GitHub](https://github.com/openai/codex)

### Title
Add a first-class “Multi-try + Validate + Rank” workflow (with attempt history + verified tool evidence)

### Summary
Sakana’s Darwin Gödel Machine (DGM) results suggest large, transferable gains for coding agents come from pragmatic workflow/tooling changes: granular file operations, patch validation, generating multiple candidates then selecting the best, and preserving attempt history—while also guarding against tool-output hallucination and metric gaming.  [oai_citation:10‡Sakana AI](https://sakana.ai/dgm/)

Codex CLI already has strong building blocks (interactive TUI, `codex exec`, `/diff`, `/review`, `/compact`, history persistence, `AGENTS.md` instruction loading).  [oai_citation:11‡OpenAI Developers](https://developers.openai.com/codex/cli/slash-commands/)  
This request proposes wiring those into a single ergonomic, reproducible workflow for “try several solutions → validate → pick best → keep traceability”.

---

### Motivation
Today, users often need to manually:
- ask for multiple alternative fixes,
- remember what failed previously,
- enforce running tests/linters,
- and verify that claimed commands/tests actually ran.

DGM’s conclusions indicate these steps should be *native agent workflow primitives*, not ad-hoc prompting.  [oai_citation:12‡Sakana AI](https://sakana.ai/dgm/)

---

### Proposed UX

#### A) Non-interactive (`codex exec`) flags
Add optional flags like:

- `--attempts N`  
  Run N independent solution attempts (sequential or parallel). Each attempt produces a candidate patch.

- `--validate "CMD"` (repeatable)  
  After applying each candidate, run one or more validation commands (tests/linters/build). Capture exit code + a capped stdout/stderr summary.

- `--rank {auto|judge}`  
  Select the “best” attempt using:
  - **auto**: prefer all validations passing; tie-break by smallest diff / fewer touched files
  - **judge**: use a judge pass that sees diffs + validation results to pick best (still gated by “real” tool outputs)

- `--keep-attempts {none|failed|all}`  
  Optionally keep attempt branches/worktrees (see “stepping stones” rationale below).

This would complement existing `codex exec` usage and keep automation-friendly semantics.  [oai_citation:13‡OpenAI Developers](https://developers.openai.com/codex/cli/reference/)

#### B) Interactive slash commands
Add (or expose) a small set of commands:

- `/attempts N`  
  Trigger multi-try workflow during a TUI session.

- `/attempts status`  
  Show a table: attempt id → validations → summary → diffstat.

- `/attempts apply <id>`  
  Apply the selected attempt to the working tree (then user can `/diff` and `/review`).

- `/fork` (or `/branch`)  
  Create a new conversation/thread fork from the current point, to explore alternatives without losing the current trajectory. (The project recently added thread fork support in releases; exposing it in the CLI would align well with DGM’s “keep an archive” conclusion.)  [oai_citation:14‡GitHub](https://github.com/openai/codex/releases)

#### C) Attempt history as structured memory
Codex already supports transcript/history persistence.  [oai_citation:15‡OpenAI Developers](https://developers.openai.com/codex/config-reference/)  
Enhance this by storing a **structured “Attempt Ledger”** per session:

- attempt prompt/plan summary
- diffstat + touched files
- validations run + results
- key errors encountered
- why rejected / superseded

Then automatically feed a compacted form of this ledger into subsequent attempts (distinct from general `/compact`), so the agent does not repeat known failures. This directly mirrors DGM’s “consider history of previous attempts” conclusion.  [oai_citation:16‡arXiv](https://arxiv.org/html/2505.22954v2)

---

### Verification / safety guardrails (important)
DGM highlights tool-use hallucination and objective hacking.  [oai_citation:17‡Sakana AI](https://sakana.ai/dgm/)  
For Codex CLI, propose:

1) **Evidence-linked execution claims**
   If the assistant says “I ran tests” or “command X passed”, the UI should be able to link that claim to an actual recorded tool execution event (exit code + captured output). If there is no matching tool event, show a warning banner like “No recorded execution found for this claim.”

2) **Tamper-resistant logging**
   Treat tool execution logs and validation results as system-owned artifacts (not model-editable), so selection/ranking cannot be gamed by changing “markers” in text.

3) **Context-length mitigation**
   Since long context is implicated in tool hallucinations, consider auto-summarizing tool logs into a compact form that the model sees, while the full raw logs remain available to the user UI. (This complements `/compact` but is specifically tool-log aware.)  [oai_citation:18‡arXiv](https://arxiv.org/html/2505.22954v2)

---

### How this maps to DGM conclusions (explicitly)
- **Multiple attempts + selection** → `--attempts`, `/attempts` (DGM finds multi-solution + ranking valuable).  [oai_citation:19‡arXiv](https://arxiv.org/html/2505.22954v2)  
- **Patch validation** → `--validate`, default project-defined validation hooks.  [oai_citation:20‡Sakana AI](https://sakana.ai/dgm/)  
- **Attempt history** → “Attempt Ledger” fed back into later tries.  [oai_citation:21‡arXiv](https://arxiv.org/html/2505.22954v2)  
- **Non-monotonic progress / stepping stones** → `--keep-attempts` + `/fork` to preserve variants.  [oai_citation:22‡Sakana AI](https://sakana.ai/dgm/)  
- **Tool hallucination / objective hacking** → evidence-linked claims + system-owned logs.  [oai_citation:23‡arXiv](https://arxiv.org/html/2505.22954v2)

---

### Acceptance criteria
- Users can run `codex exec --attempts 3 --validate "pytest -q"` and get:
  - 3 candidate diffs
  - validation outcomes per attempt
  - an automatically selected “best” patch
  - a summary explaining why it was chosen
- In TUI, `/attempts 3` shows attempts and lets the user apply one.
- Any “tests passed” statement in the final output can be traced to a real execution event (or is flagged).
- Attempt ledger is persisted (respecting existing `history.persistence` settings).  [oai_citation:24‡OpenAI Developers](https://developers.openai.com/codex/config-reference/)

---

### Notes / optional extensions
- Allow repos to specify default validation commands via `AGENTS.md` conventions (since Codex already loads AGENTS instructions at startup).  [oai_citation:25‡OpenAI Developers](https://developers.openai.com/codex/guides/agents-md/)  
- If conversation/thread fork APIs are already present internally, wiring a CLI surface command would unlock the “archive/branch exploration” benefit without requiring any DGM-style search loop.  [oai_citation:26‡GitHub](https://github.com/openai/codex/releases)

---

If you want, I can also tailor the issue draft to match the repo’s preferred templates (e.g., splitting into 2–3 smaller issues instead of one), but the above is the cohesive “single change request” that most directly instantiates DGM’s conclusions into Codex CLI ergonomics.
