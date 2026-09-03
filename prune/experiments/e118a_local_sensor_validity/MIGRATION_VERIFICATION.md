# E118a migration verification

Status: **MIGRATION_VERIFICATION — VERIFIED**

This verification reads the frozen E118a artifacts migrated from
`advatar/parity` and does not collect interventions, evaluate candidate
outcomes, or replace the original scientific result. The independently
reproduced verdict is:

`FAIL_NO_PROSPECTIVE_INCREMENTAL_LOCAL_VALIDITY`

## Evidence and population checks

- All 7 frozen protocol/evidence files match their Parity originals by
  SHA-256 and byte comparison.
- The candidate plan contains 12 parent panels and 144 planned candidates.
- The frozen mutation log contains 144 generated rows: 121 analyzed, 20
  duplicate configurations excluded, and 3 no-op mutations excluded.
- The analyzed population spans 6 repositories and 12 eligible parents.
- The target repositories are exactly the six E118 training repositories:
  `babel/babel`, `prometheus/prometheus`, `sharkdp/bat`, `tokio-rs/axum`,
  `tokio-rs/tokio`, and `vuejs/core`.
- The causal score uses exactly the three frozen E118 probe repositories:
  `axios/axios`, `burntsushi/ripgrep`, and `uutils/coreutils`.
- The E118 evaluation repositories remain excluded:
  `astral-sh/ruff`, `facebook/docusaurus`, `nushell/nushell`, and
  `preactjs/preact`.
- Permutations are restricted to each `(repository, parent_id)` sibling group.

## Independently reproduced statistics

| Statistic | Value |
| --- | ---: |
| Pooled within-parent raw/gain rank correlation | 0.08510711710997547 |
| Pooled within-parent causal/gain rank correlation | 0.09740982292363289 |
| Partial causal/gain rank correlation given raw | 0.10793451225738734 |
| Causal-signal one-sided permutation p-value | 0.16128387161283872 |
| Incremental-signal one-sided permutation p-value | 0.138986101389861 |
| Mean raw top-1 regret | 0.0034664952666666814 |
| Mean causal top-1 regret | 0.023318223683796307 |
| Mean random top-1 regret | 0.019499789264260704 |
| Mean raw top-4 regret | 0.0003410907476851949 |
| Mean causal top-4 regret | 0.020372683718055565 |
| Mean random top-4 regret | 0.011991952441985131 |

The machine-readable record is `migration_verification.json`. Its
`verification_type` is `MIGRATION_VERIFICATION`; it is not a new E118a result.

## Verification commands

The migrated and existing E118 Python suites passed 27 tests (18 migrated
E118a tests and 9 existing E118 protocol tests):

```text
PYTHONDONTWRITEBYTECODE=1 prune/.venv-e118/bin/python -m unittest \
  prune.tests.test_e118_protocol \
  prune.tests.test_e118a_local_sensor_validity -v
```

The full Rust workspace suite passed 11 tests:

```text
cd prune && cargo test --workspace
```

The suite emitted pre-existing compiler warnings. A first Rust attempt found a
stale ignored FastEmbed download-cache lock. After moving only that disposable
cache aside and confirming that no Cargo or FastEmbed process was active, the
previously affected test and the full workspace suite passed.

No E118 or E118a scientific outcome was regenerated. E118 is unchanged, E118b
remains unauthorized, and E118c has not started.
