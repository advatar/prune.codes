# E118a local sensor validity — repository migration

Status: **frozen negative evidence migrated; no scientific outcome rerun**

This directory makes `advatar/prune.codes` the canonical repository home for
the completed E118a local sibling-ranking experiment. E118a was accidentally
implemented and preserved on `advatar/parity`, even though its mutation,
predictor, Stage-A, dataset, exact-state, and pack machinery came from Prune.

`frozen_parity_tree/` mirrors the original Parity paths. Every file in that
tree is copied byte-for-byte from
`advatar/parity:research/e118a-local-sensor-validity` at evidence commit
`593e78e470cd5a03ffea6d82e7ef2fb9faf3b61f`. Historical absolute paths and
formatting are intentionally preserved.

The result remains `FAIL_NO_PROSPECTIVE_INCREMENTAL_LOCAL_VALIDITY`. E118b is
not authorized. This migration changes repository ownership only; it does not
create a new experiment or amend either E118 or E118a.

## Distinct historical document

The existing file
`prune/experiments/E118a-prospective-sensor-transfer-spec.md`, added in Prune
commit `b10d0ded147621beea7708a85a1b6e2350e32d3f`, is an earlier, unexecuted
proposal for a different cross-probe-transfer diagnostic. It is not the
144-candidate local sibling-ranking experiment archived here. Neither document
supersedes or silently renames the other.

Machine-readable provenance and byte hashes are in `migration_manifest.json`.

