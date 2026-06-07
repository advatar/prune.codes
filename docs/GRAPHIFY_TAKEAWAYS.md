# Graphify Takeaways for Prune

The sibling `graphify/` checkout is useful as a product and UX reference, not as
a replacement for Prune's engine.

## What Graphify Does Well

- Presents a repo graph as a compact artifact: `graph.html`, `graph.json`, and
  `GRAPH_REPORT.md`.
- Makes graph output explainable with hub nodes, strong relationships, and
  suggested questions.
- Keeps extractor stages easy to reason about: detect, extract, build graph,
  cluster, analyze, report, export.
- Labels relationship confidence so users can tell direct evidence from weaker
  inference.

## What Prune Already Has

Prune already owns the deeper context-engine substrate:

- SQLite-backed fragments, symbols, refs, and resolved edges.
- Hybrid lexical/vector retrieval.
- Graph expansion for context packing.
- Connected subgraph selection.
- Signals-first slicing from build logs, diffs, tests, and paths.
- Recipe memory and strategy evolution.

Duplicating Graphify's full Python graph pipeline would split responsibility and
make Prune harder to validate.

## Adopted Pattern

Prune adopts the explainability layer through:

```sh
ce graph-report --db .ce/index.sqlite --out .ce/GRAPH_REPORT.md
```

The command reads the existing Prune index and writes a Markdown report with:

- indexed fragment and edge counts,
- edge type distribution,
- top connected fragments,
- strongest cross-file relationships,
- suggested follow-up questions.

This gives agents and humans a Graphify-style orientation artifact while keeping
the graph source of truth inside Prune's existing index.

## Future Candidates

- Add confidence labels to stored edge rows once edge producers can report
  `extracted`, `inferred`, or `ambiguous` explicitly.
- Add a JSON graph export that reuses the same report query layer.
- Surface graph-report generation from PruneApp diagnostics after indexing.
