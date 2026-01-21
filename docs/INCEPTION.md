Here’s an **Epic + sub-issues pack** you can paste into GitHub Issues to implement **Inception Mode** exactly as specified:

- Preference capture via **A2UI v0.8** (JSONL streaming UI)
- Driven by a **local Apple Foundation Model** session
- First client is your **native SwiftUI macOS app**
- Answers persist into repo and are automatically used by **Prune inception strategy profiles**

## References for implementers
- A2UI Protocol v0.8 spec (message types, JSONL stream, `userAction` event schema).  [oai_citation:0‡A2UI](https://a2ui.org/specification/v0.8-a2ui/)  
- A2UI renderer development guide (buffering rules + rendering flow).  [oai_citation:1‡A2UI](https://a2ui.org/guides/renderer-development/)  
- A2UI GitHub repo status (v0.8 public preview; expect changes).  [oai_citation:2‡GitHub](https://github.com/google/A2UI?utm_source=chatgpt.com)  
- A2UI v0.9 evolution guide (future migration: `beginRendering` → `createSurface`, schema refactors).  [oai_citation:3‡A2UI](https://a2ui.org/specification/v0.9-evolution-guide/)  
- Apple Foundation Models docs: `LanguageModelSession` and `@Generable` (for guided generation).  [oai_citation:4‡Apple Developer](https://developer.apple.com/documentation/foundationmodels/languagemodelsession?utm_source=chatgpt.com)  

---

# Epic Issue: Inception Mode (A2UI Interview + Local Foundation Model + SwiftUI Client + Prune Integration)

```md
# Epic: Inception Mode — A2UI Preference Interview driven by Local Apple Foundation Model (SwiftUI client)

## Goal
Implement “Inception Mode” for Prune:
- Capture project preferences at feature inception time (architecture, testing, dependency policy, workflow norms).
- Use A2UI v0.8 streaming protocol to render a native wizard UI in our SwiftUI macOS app.
- Drive the wizard using a local Apple Foundation Model session (no cloud LLM required).
- Persist answers into the repo as a small `.prune/` artifact and automatically inject them into “inception” context packs.

## Why
The earliest mistakes are architectural and convention mistakes, not syntax errors. This feature reduces rework by:
- aligning first-pass design with repo conventions
- encoding preferences explicitly
- turning “design choices” into reusable context

## Non-goals (for this epic)
- No remote/web A2UI client (SwiftUI app is first)
- No full multi-agent orchestration (single local agent loop is enough)
- No deep static analysis/language-server; we rely on Prune’s existing indexing and summaries

## Target Protocol / Version
- Implement A2UI v0.8 (stable) end-to-end in the client (stream parsing, surface store, data model store, bound values, userAction).
- Add version/capability fields in our own runtime so we can migrate to A2UI v0.9 later without rewriting the renderer.

## User flows
1) User opens Prune macOS app → “Inception Wizard”
2) Select repo (local path)
3) Start interview:
   - Local model generates A2UI surface(s)
   - User answers
4) Finish:
   - App writes `.prune/prune.preferences.json`
   - Optionally writes `.prune/prune.decisions.md`
5) Prune inception strategy automatically includes these files in context packs and uses them to shape planning and implementation.

## Success criteria
- A2UI wizard renders reliably, captures answers, and persists them.
- A “ce pack --intent inception …” includes preferences and produces a coherent plan-first pack.
- Fully offline operation (local model + local repo) supported.

## Sub-issues (create these issues and paste numbers here)
- [ ] #___ A2UI Runtime: JSONL parser + message dispatcher + surfaces + binding + userAction
- [ ] #___ SwiftUI A2UI Renderer + Interview Catalog
- [ ] #___ Local Foundation Model Agent Loop: guided generation of A2UI + state machine
- [ ] #___ Preferences Schema + Persistence: `.prune/` artifacts + resume/edit
- [ ] #___ Prune Inception Strategy: router + onboarding pack + golden paths + interface-first
- [ ] #___ Docs + QA: user manual + end-to-end demo + tests

## Definition of Done (Epic)
- All sub-issues merged
- End-to-end demo recorded (or scripted) showing:
  - wizard → writes preferences → inception pack reflects those preferences
- A “default inception profile” exists and is stable under budget constraints
```

---

# Sub-issue 1: A2UI Runtime (Swift) — JSONL Stream, Surfaces, Data Model, Bindings, userAction

```md
# A2UI Runtime (Swift): JSONL parser + dispatcher + surfaces + bindings + userAction

## Summary
Implement a minimal but correct A2UI v0.8 runtime in Swift, suitable for a native SwiftUI client.

## Requirements
### 1) JSONL streaming parser
- Parse server-to-client stream line-by-line (each line is one JSON object).
- Dispatch by message type:
  - surfaceUpdate
  - dataModelUpdate
  - beginRendering
  - deleteSurface
- Ignore unknown keys safely (forward compatibility).

### 2) Surface manager
For each surfaceId:
- component buffer (Map<componentId, ComponentInstance>)
- data model store (JSON-like object)
- render readiness state (rootId, catalogId)
- last-updated timestamps (for diagnostics)

### 3) Component model (adjacency list)
- Store components by ID.
- Support common child references:
  - child
  - contentChild
  - children.explicitList
  - children.template (optional in v1; implement if needed for interview lists)
- Component “wrapper object” must contain exactly one component type key (v0.8) — validate and emit client-side error if invalid.

### 4) Data model update (typed adjacency list)
- Support v0.8 typed entries:
  - valueString, valueNumber, valueBoolean, valueMap, valueList, valueNull
- Apply updates at a `path` (JSON pointer-like).
- If path omitted: replace root model.

### 5) BoundValue resolution
- Resolve literal vs path bindings:
  - if literal only → use literal
  - if path only → look up in data model
  - if both → initialize model at path from literal, then bind to path (shorthand initialization)

### 6) Progressive rendering rule
- Buffer surfaceUpdate + dataModelUpdate without rendering until beginRendering for a surface is received.
- After beginRendering:
  - allow incremental updates to re-render affected subtree.

### 7) Client-to-server event envelope: userAction
- Provide helper to construct userAction payload:
  - name, surfaceId, sourceComponentId, timestamp, context
- Resolve action.context bound values before sending.

### 8) Error envelope
- Provide helper to send error payload to agent loop (render-time binding failures, unknown component types, invalid schema).

## Deliverables
- `A2UIRuntime` Swift module (or Swift Package) with:
  - Message decoding
  - Surface store + APIs
  - Binding resolver
  - Event builder

## Acceptance Criteria
- Can load and render a known-good A2UI v0.8 interview surface from a fixture JSONL.
- userAction generation includes resolved context values.
- Runtime can handle multiple surfaces without cross-contamination.

## Test Plan
- Unit tests:
  - JSONL parsing with partial lines
  - component wrapper validation
  - dataModelUpdate path application
  - bound value resolution
- Golden tests:
  - feed JSONL fixture → surface graph built → root resolvable
- Negative tests:
  - invalid component wrapper → emits error event
```

---

# Sub-issue 2: SwiftUI A2UI Renderer + Interview Catalog

```md
# SwiftUI A2UI Renderer + Interview Catalog (A2UI → native widgets)

## Summary
Render A2UI v0.8 surfaces using SwiftUI widgets via a small “Interview Catalog”.

## Requirements
### 1) Widget registry
Map component type strings to SwiftUI views. Minimum set:
- Layout: Column, Row, Card, Divider, Spacer
- Text: Text, Markdown
- Inputs: TextField, TextArea, Toggle, Select, MultiSelect
- UX: Progress (stepper), Badge, Callout
- Actions: Button

### 2) Data binding support
- Inputs must bind to data model paths.
- Editing an input updates the local data model store immediately.
- For “Submit/Next/Finish” buttons, action.context is resolved from data model at click time.

### 3) Rendering
- Build UI tree by resolving component IDs from root.
- Handle unknown components gracefully:
  - render a “Unknown component” placeholder
  - emit error event

### 4) Accessibility & UX
- Support headings/variants for Text
- Keyboard navigation for form fields
- Validation display (minimal v1: inline text below field)

### 5) Surface lifecycle
- Render the surface identified by beginRendering.root as the main view.
- Support surface deletion.

## Deliverables
- SwiftUI renderer view: `A2UISurfaceView(surfaceId:)`
- Interview catalog definition and mapping

## Acceptance Criteria
- “Interview Wizard” renders end-to-end:
  - multiple pages
  - next/back
  - finish
- Inputs update the underlying data model and are reflected in action.context.

## Test Plan
- Snapshot tests for key screens (optional)
- Interaction tests:
  - enter text → press Next → userAction contains updated answers
```

---

# Sub-issue 3: Local Foundation Model Agent Loop (Guided Generation → A2UI)

```md
# Local Foundation Model Agent Loop: A2UI interview driver (guided generation)

## Summary
Implement a local agent loop using Apple Foundation Models that:
- generates A2UI v0.8 surfaces for an interview wizard
- consumes userAction events
- updates the UI/data model over time
- produces a final normalized preferences object

## Requirements
### 1) Agent session
- Use a local LanguageModelSession with strong system instructions:
  - Output must be A2UI v0.8 server-to-client messages (JSONL or typed chunks)
  - Maintain a stable surfaceId for the interview (e.g., "prune_inception")
  - Emit initial batch: surfaceUpdate(s) + dataModelUpdate(s) + beginRendering (exactly once per surface)
  - For updates: use dataModelUpdate and/or surfaceUpdate only

### 2) Guided generation (strongly recommended)
- Use Generable types to constrain output to valid A2UI envelope shapes (v0.8).
- Validate before applying; on validation failure:
  - request correction from model with a concise error message
  - do not crash UI

### 3) Interview state machine
- Steps:
  1) Scope / thin-slice definition
  2) UX and constraints
  3) Architecture boundaries and patterns
  4) Types and contracts preferences
  5) Testing and quality gates
  6) Dependencies policy
  7) Workflow preferences
  8) Summary + confirm
- Support adaptive follow-ups:
  - If user selects “strict types”, ask about API shape preference
  - If user enables “new deps allowed”, ask for bar/rules

### 4) Output normalization
- Maintain canonical question IDs (stable schema).
- On finish:
  - output normalized JSON object (answers)
  - output short “decisions” bullets (optional)

## Deliverables
- `InceptionInterviewAgent` Swift module:
  - `start(repoContext:) -> stream<A2UIMessage>`
  - `handle(userAction:) -> stream<A2UIMessage>`
  - `finalize() -> PreferencesResult`

## Acceptance Criteria
- The wizard works fully offline and completes.
- Produced preferences match canonical schema and include only expected keys.
- Agent can resume from existing data model (if answers already exist).

## Test Plan
- Use a deterministic “mock model” that replays prerecorded A2UI outputs to test UI flow.
- Manual QA with real local model:
  - complete interview
  - verify saved file content
  - resume and edit
```

---

# Sub-issue 4: Preferences Schema + Persistence (.prune/ artifacts + resume/edit)

```md
# Preferences Schema + Persistence: `.prune/` artifacts + resume/edit

## Summary
Define a stable preference schema and persist it inside the repo so Prune can consume it for inception packs.

## Requirements
### 1) File locations
- `.prune/prune.preferences.json` (required)
- `.prune/prune.decisions.md` (optional)

### 2) JSON schema (v1)
- Must include:
  - version
  - updated_at
  - answers (structured)
  - optional freeform notes
- Use canonical IDs for answers (stable keys).

### 3) Write & update semantics
- On first completion: create `.prune/` directory if missing; write both files.
- On subsequent runs:
  - load existing preferences into the interview as defaults
  - allow edits
  - rewrite preferences file atomically

### 4) Diagnostics
- Provide “Export interview transcript” (optional)
- Provide “Reset preferences” (danger button)

## Deliverables
- Preferences schema doc in user manual
- Persistence helpers in app:
  - load
  - save
  - atomic write
- Migration stub for future versions

## Acceptance Criteria
- Preferences survive app restarts.
- Editing preferences produces deterministic diffs (stable formatting recommended).
- Missing file is handled gracefully (starts fresh).

## Test Plan
- Unit tests:
  - load/save round trip
  - atomic write behavior
- Integration:
  - run wizard twice; confirm defaults are prefilled from file
```

---

# Sub-issue 5: Prune “Inception” Strategy Profile (router + onboarding pack + golden paths + interface-first)

```md
# Prune Inception Strategy Profile: preferences injection + onboarding pack + design-first context

## Summary
Add a new “inception” intent/profile in Prune that:
- always includes `.prune` preferences artifacts (tiny, high signal)
- produces onboarding/architecture packs optimized for new feature design and first implementation
- stays within strict budgets and uses progressive disclosure

## Requirements
### 1) Intent router
- Add `--intent inception` (and/or a strategy profile name).
- Optional: heuristic classifier from task text (feature vs bugfix), but CLI flag is authoritative.

### 2) Always-include injection
In inception mode:
- Always include `.prune/prune.preferences.json` if present
- Include `.prune/prune.decisions.md` if present
- Include as a dedicated top section: “Project Preferences”

### 3) Onboarding layer
Under a small token budget:
- entrypoints (per language pack heuristics)
- directory ApiSummaries (top-level or selected hot dirs)
- 1–2 “golden path” examples:
  - representative feature implementation
  - representative API/data access
  - representative UI component/hook (if TSX) or View (if SwiftUI)

### 4) Interface-first packing
Prefer:
- types/interfaces/protocols
- exported signatures and public APIs
Only include bodies for:
- golden paths
- direct call sites required by the task

### 5) Output format
Context pack should include:
- Preferences (always)
- Repo map (summaries)
- Conventions/golden paths
- Proposed plan scaffold (optional: if you generate plans in pack)
- Deferred fetch list

## Deliverables
- New strategy profile config file(s): `inception_balanced`, `inception_summary_first`
- Wiring in CLI and pack output

## Acceptance Criteria
- Inception pack remains small (budget-respecting) but is sufficient to write an initial implementation aligned with repo conventions.
- Preferences file is present at the top and influences decisions (observable in plan or chosen patterns).

## Test Plan
- Run on at least:
  - TSX repo
  - SwiftUI repo
  - Rust repo
- Verify that preferences and golden paths are included and budgets are met.
```

---

# Sub-issue 6: Docs + QA (User manual, Inception workflow, Lovable instruction block placement)

```md
# Docs + QA: Inception workflow + user manual updates + end-to-end demo

## Summary
Document and validate the inception workflow end-to-end.

## Requirements
### 1) User manual section: Inception Wizard
- When to use it
- What it writes (`.prune/…`)
- How to edit/reset
- How it influences inception packs

### 2) User manual section: Lovable operating procedure (placement)
- Include the “repo.ensure_fresh before pack” and “repo.sync after push” instruction block in the manual (NOT on the landing page).

### 3) QA checklist
- Offline flow
- Corrupt JSON recovery
- Large repos (performance guardrails)
- Re-render correctness (no flicker, no stale context)

### 4) Demo
- Provide a scripted demo run (markdown steps) and a sample repo or fixture JSONL.

## Acceptance Criteria
- A new user can follow docs to:
  - run interview
  - write preferences
  - generate inception pack
- Demo artifacts are included in repo.

## Test Plan
- Manual checklist run
- Smoke tests on 2 machines/OS versions
```

---
