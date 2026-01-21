Below is a **ready-to-paste GitHub Epic issue** plus **5 sub-issue bodies** (each with Definition of Done + Test Plan). The “linked” part works like this: you create the sub-issues first, get their issue numbers (e.g. `#42`), then paste those numbers into the Epic checklist.

I’m giving you everything in copy/paste format.

---

## Epic Issue (paste into a new GitHub issue)

```md
# Epic: vNext Pruning & Quality Improvements (Signals → Constraints → Subgraph → UI Skeletonization → Recipe Memory)

## Why this Epic exists
Prune already performs strong structure-aware retrieval and budgeted packing. The next step is to:
- prune more aggressively (smaller packs)
- while improving correctness (fewer missing defs / fewer hallucinations)
- and measure progress in a way that supports DGM-style strategy evolution.

This epic implements the next 5 highest-leverage capabilities for generic “fix a failure” workflows, across languages.

## Outcomes
- Better signal anchoring from failure logs (spans, traces, tests, module hints)
- Packs that are both small *and* complete enough to reason (“no unbound names”)
- Packs selected as connected explanations rather than top-K fragments
- Higher density compaction for UI-heavy code (TSX / SwiftUI)
- Learned repair recipes that reduce repeated work across recurring failures

## Non-goals (for this epic)
- No new UI/desktop app work here (observability logs are fine; UI later)
- No full language-server-grade type checking
- No multi-branch indexing requirements

---

## Key metrics we will track (must be logged for every run)
These metrics are required so strategies can evolve with a fitness function:
- `pack_tokens_total` (pack + on-demand fetches)
- `baseline_tokens_total` (Baseline A) and `saved_pct`
- `hit_rate_paths` (did pack include touched files in the eventual fix?)
- `unbound_symbol_count` (symbols referenced without defs or deferred candidates)
- `avg_iterations_per_fix` (if/when an end-to-end harness exists)
- `redundancy_pct` (repeated fragments across steps in a session)

---

## Baseline definition (Baseline A) for token savings
Baseline A (generic failure baseline):
- If task contains `path:line`:
  - include full body of file containing that location (or the containing fragment, pick one and stay consistent)
  - plus full bodies of top 2 locally-imported modules
- Else:
  - include full bodies of top 3 lexical hits

Compute baseline tokens using the same tokenizer used by Prune pack accounting.

---

## Sub-issues (create these issues and paste the issue numbers below)
- [ ] #___ CR-01: Structured Failure Signal Extraction (signals bundle + strategy genes + pack annotations)
- [ ] #___ CR-02: Constraint-Based Packing (“No Unbound Names” support closure + unresolved reporting)
- [ ] #___ CR-03: Minimum-Cost Connected Subgraph Packing (Steiner-ish/beam selector + multi-view costs)
- [ ] #___ CR-05: Domain-Aware Skeletonization (JSX + SwiftUI compaction modes + config genes)
- [ ] #___ CR-04: Recipe Memory (store successful fix patterns + retrieve on similar failures)

> Note: CR-05 before CR-04 is intentional. CR-04 benefits from having strong logging and stable pack structure.

---

## Integration points (must be consistent across all sub-issues)
- Signals must be serialized in pack JSON (for debugging + evaluation).
- Support closure must operate on the exact text that will be emitted (view-aware).
- Subgraph selection must respect hard budgets (tokens) and per-file caps.
- Skeletonization must preserve readability and provenance (“what was collapsed?”).
- Recipe memory must be clearly labeled and never override repo truth.

---

## Rollout plan
Phase 1 (behind flags):
- Implement each capability behind strategy toggles.
- Default strategy remains unchanged.

Phase 2 (A/B evaluation):
- Evaluate on internal “fix failure” task set.
- Promote new default strategy if it improves:
  - hit-rate at equal tokens, or
  - equal hit-rate at lower tokens.

Phase 3 (enable by default):
- Enable improvements in balanced failure strategy profile.

---

## Definition of Done (Epic)
- All sub-issues merged
- All metrics logged for every run
- At least one “failure_default_next” strategy profile exists that:
  - uses structured signals
  - enforces no-unbound-names
  - uses connected-subgraph selection
  - uses UI skeletonization where appropriate
- Token savings vs Baseline A reported in logs and included in evaluation output
```

---

## Sub-issue 1: CR-01 Structured Failure Signal Extraction

```md
# CR-01: Structured Failure Signal Extraction

## Summary
Implement a unified Signal Extractor that converts raw task input (error logs, stack traces, diffs, failing test output) into a structured `SignalBundle`, and integrate signals into retrieval + ranking + pack annotations.

## Why
Failure outputs usually contain the best possible anchors (file:line, traces, related locations). Parsing these reliably lets us:
- seed retrieval from ground truth
- prune aggressively
- reduce dependence on broad semantic retrieval
- improve pack correctness

## Scope
### Signal types (minimum)
- `SpanHint { path, line, col?, message?, confidence }`
- `SymbolHint { name, kind?, confidence }`
- `ModuleHint { specifier, importer_path?, confidence }`
- `TestHint { name, suite?, confidence }`
- `ErrorHint { code?, category?, first_line?, confidence }`
- `DiffHint { changed_paths[], hunk_spans[] }`

### Inputs supported (generic)
- File:line(:col) patterns across common toolchains
- Stack trace formats (JS + Swift at minimum; others opportunistic)
- “Cannot resolve module” / import resolution breadcrumbs
- Unified diffs (paths + hunks)
- Test failure names where present

## Deliverables
- `SignalBundle` + serializer in pack JSON (`signals_used[]`)
- Signal-aware candidate seeding:
  - direct lookup by path+line → covering fragment
  - path boosting for files mentioned
- Strategy genes:
  - `signals_enabled`
  - `signal_span_boost`
  - `signal_max_spans`, `signal_max_paths`, etc.
- Logging:
  - signals extracted count by category
  - which signals were actually used

## Acceptance Criteria
- If task contains a file:line, pack includes a slice/fragment covering that region ≥ 90% on a curated set.
- When signals exist, average pack tokens decrease while hit-rate stays same or improves.

## Definition of Done
- [ ] SignalBundle extraction implemented + unit tests
- [ ] Pack JSON includes signals extracted and used
- [ ] Signals influence retrieval (candidate pool) and ranking (boosts)
- [ ] Metrics: signal-hit-rate and pack token changes are logged

## Test Plan
1) Unit tests:
   - parsing file:line:col variants
   - parsing 2–3 stack trace formats
   - parsing unified diff hunks and changed paths
2) Integration tests:
   - given a synthetic repo + an error log with `path:line`, ensure the first pack includes that fragment
3) Regression tests:
   - confirm signals don’t explode context when logs are long (caps enforced)
```

---

## Sub-issue 2: CR-02 Constraint-Based Packing (“No Unbound Names”)

```md
# CR-02: Constraint-Based Packing (“No Unbound Names”)

## Summary
Add a packing constraint: when included context references identifiers, Prune should include definitions (prefer signatures) or explicitly list unresolved symbols with best candidates in deferred.

## Why
A major agent failure mode is “referencing unseen definitions,” causing hallucinations. Minimal supporting defs (often signatures) are cheap and improve correctness.

## Scope
- For included pack items (slice/skeleton/body), compute referenced identifiers:
  - use refs table when available
  - fall back to lightweight token/identifier extraction from emitted text
- Enforce a support closure:
  - for top-N “important” identifiers, include:
    - definition fragment (signature view), OR
    - deferred candidates + explicit unresolved note

## Deliverables
- `support_closure` packing phase:
  - inputs: selected items + identifier list + budget
  - outputs: added support items + unresolved list
- Strategy genes:
  - `support_enabled`
  - `support_max_defs`
  - `support_signature_only`
  - `support_min_confidence`
  - `unbound_penalty_weight`
- Pack annotations:
  - `covers_symbols[]`
  - `unresolved_symbols[]` with deferred suggestions

## Acceptance Criteria
- Unbound referenced symbols (no def shown and no deferred candidates) drop to near zero in evaluation set.
- Token overhead limited (target: +10–15% max) due to signature-first support.

## Definition of Done
- [ ] Support closure implemented and budget-aware
- [ ] Pack JSON/text includes unresolved symbol reporting
- [ ] Strategy toggles and caps implemented
- [ ] Logging: unbound_symbol_count per pack + support defs count

## Test Plan
1) Unit tests:
   - identifier extraction from slices/skeletons
   - definition lookup + candidate selection
2) Integration tests:
   - create a repo where a failure slice references a type defined elsewhere
   - ensure pack includes type signature or lists it as unresolved with candidates
3) Budget tests:
   - verify support closure respects budget and degrades gracefully (more deferred, fewer included)
```

---

## Sub-issue 3: CR-03 Minimum-Cost Connected Subgraph Packing (Steiner-ish / Beam)

```md
# CR-03: Minimum-Cost Connected Subgraph Packing (Steiner-ish / Beam)

## Summary
Replace “bag of top-K fragments” selection with a connected explanation: select a minimal-cost subgraph connecting signals, key defs, and necessary neighbors while respecting strict budgets.

## Why
Disconnected top-K often misses connective tissue and wastes tokens. Graph-guided selection tends to reduce context while improving completeness and relevance.

## Scope
- Inputs:
  - seed nodes from signals + top retrieval candidates
  - expanded neighborhood via edges (bounded)
- Selection:
  - approximate Steiner/prize-collecting approach using greedy/beam search
  - node cost = token cost of chosen view (sig/slice/skeleton/body)
  - node benefit = relevance + coverage (signals/support)
  - edge weighting = explanation value
- Output:
  - selected set of nodes + chosen views + connectivity annotations

## Deliverables
- `SubgraphSelector` module:
  - `select_subgraph(seeds, neighborhood, budget) -> Selection`
- Multi-view cost model:
  - precompute estimated token costs for each candidate view
- Strategy genes:
  - `subgraph_enabled`
  - `beam_width`
  - `max_hops`
  - `connectivity_penalty`
  - per-edge-type weights and hop decay overrides
- Pack provenance:
  - “included because it connects X → Y via edge E”

## Acceptance Criteria
- On evaluation set, equal-or-better hit-rate at equal-or-lower token budgets compared to current top-K.
- Packs show fewer disjoint sections (connectivity metric improves).

## Definition of Done
- [ ] Selector implemented and behind a strategy toggle
- [ ] Budget enforcement is strict (never overflow)
- [ ] Pack output includes connectivity/provenance notes
- [ ] Metrics: connectivity score + pack token deltas logged

## Test Plan
1) Unit tests:
   - selector chooses cheaper connected set over expensive disjoint set
   - respects per-file caps and budget
2) Integration tests:
   - synthetic repo with known edges; seed at a call site; ensure defs pulled via minimal path
3) Regression tests:
   - ensure selector doesn’t “explode” when graph is dense (caps enforced)
```

---

## Sub-issue 4: CR-05 Domain-Aware Skeletonization for JSX and SwiftUI

```md
# CR-05: Domain-Aware Skeletonization for JSX (TSX) and SwiftUI

## Summary
Implement domain-aware skeletonization that collapses UI-heavy structure while preserving the logic needed to debug failures:
- TSX: collapse large JSX subtrees
- SwiftUI: collapse view builder structure
Keep hooks/state/actions/API calls visible.

## Why
UI trees are huge and often irrelevant. Most debugging needs logic and data flow, not full markup. This reduces tokens and increases signal-to-noise.

## Scope
### TSX skeletonization
- Collapse JSX elements beyond a threshold:
  - keep component/tag name + key prop names (bounded)
  - collapse children to indented placeholder
- Preserve:
  - hooks calls (useX)
  - event handlers and closures
  - data transforms and API calls
- Provide clear placeholders indicating collapsed regions

### SwiftUI skeletonization
- Collapse builder hierarchies (VStack/HStack/etc.)
- Preserve:
  - state bindings
  - actions/closures
  - navigation and side effects
  - API calls
- Apply especially to `body` builders

## Deliverables
- New snippet mode(s):
  - `tsx_skeleton`
  - `swiftui_skeleton`
- Strategy genes:
  - thresholds for collapse
  - preserve-list toggles
  - max props/arms/children shown
- Provenance:
  - collapse markers are indentation-aware and readable

## Acceptance Criteria
- On UI-heavy files, average token usage decreases meaningfully without reducing hit-rate on failure-relevant code.
- Skeleton output remains readable and clearly indicates collapsed regions.

## Definition of Done
- [ ] TSX skeletonizer implemented and integrated into pack view selection
- [ ] SwiftUI skeletonizer improvements integrated
- [ ] Strategy toggles and thresholds exposed
- [ ] Logging: skeleton savings and usage rate

## Test Plan
1) Unit tests:
   - JSX collapse respects thresholds and preserves event handlers
   - SwiftUI collapse preserves actions and state
2) Integration tests:
   - create UI-heavy fragments and ensure skeleton output stays under budget
3) Readability checks:
   - snapshot tests for skeleton output with indentation-aware placeholders
```

---

## Sub-issue 5: CR-04 Recipe Memory (Repair Patterns)

```md
# CR-04: Recipe Memory (Repair Patterns)

## Summary
Store minimal “repair recipes” from successful fixes and retrieve them for similar future failures to reduce iterations and improve correctness.

## Why
Many failures repeat in shape. A small curated memory often beats re-deriving context every time, and it’s token-efficient.

## Scope
### Store on success
When a fix is recorded as successful (manual trigger or harness trigger):
- failure fingerprint
- minimal pack summary (not the full pack)
- diff/patch metadata (not necessarily full content)
- tags (optional)
- outcome: success + tokens + iterations

### Retrieve on new failure
Given new failure text:
- retrieve top similar recipes
- include a bounded “recipe excerpt” section in the pack:
  - checklist of likely causes
  - 1–2 minimal pointers (“check these files/symbols”)
- label clearly as “prior fix pattern” (never authoritative)

## Deliverables
- New persistent store:
  - `recipes` table + JSONL export
- Fingerprinting function:
  - stable normalization of failure text into a signature
- Retrieval gating:
  - only include recipes when similarity > threshold
  - strict token cap for recipes section
- Strategy genes:
  - `recipes_enabled`, `recipes_max_tokens`, `recipes_min_similarity`

## Acceptance Criteria
- On repeated/familiar failure categories, session tokens and/or iterations decrease measurably.
- Recipe section never dominates pack budget and never overrides repo truth.

## Definition of Done
- [ ] Recipe store schema + CRUD
- [ ] Fingerprint + similarity retrieval implemented
- [ ] Pack integrates recipe section behind a toggle with token cap
- [ ] Logging: recipe hit-rate and impact on tokens

## Test Plan
1) Unit tests:
   - fingerprint stability under minor log variations
   - retrieval returns expected recipes on similar input
2) Integration tests:
   - store a recipe → trigger retrieval → ensure pack includes recipe excerpt under cap
3) Safety tests:
   - ensure recipes are clearly labeled and do not silently modify retrieval/packing decisions
```

---
