# E118 scientific report

## Result

The frozen decision is **`SWE_BENCH_SPARSE_CAUSAL_NOT_SUPPORTED`**.

E118 tested whether an uncertainty-gated, sparse measurement of the raw
ranker's top four strategy mutations improves closed-loop Prune strategy search
over the same raw predictor and mutation-operation schedule. It used real
SWE-bench Multilingual repository states, Stage-A changed-path retrieval utility,
and zero model calls. This is not an issue-resolution experiment.

| Frozen outcome | Value |
| --- | ---: |
| Selected instances | 57 |
| Independent repositories | 13 |
| Train / probe / evaluation repositories | 6 / 3 / 4 |
| Evaluation instances | 20 |
| Raw gain over common base | -0.0086608775 |
| Causal gain over common base | -0.0428504145 |
| Causal minus raw | -0.0341895370 |
| Causal repository wins | 1 of 4 |
| Probe-cost ratio | 0.11 |

The causal-minus-raw value crosses the preregistered negative boundary of
`<= -0.01`. No threshold, exclusion, candidate, or policy was changed after the
result.

The causal arm gained 0.0120713 connectivity relative to raw but lost 0.05
changed-path recall, used 47.6 more tokens per evaluation instance on average,
and therefore lost 0.0341895 utility. Raw also lost to the base, so E118 does not
support the view that its raw predictor was already sufficient.

## Repository evidence and sensitivity

| Evaluation repository | Causal minus raw utility |
| --- | ---: |
| `astral-sh/ruff` | -0.003999608 |
| `facebook/docusaurus` | -0.128241908 |
| `nushell/nushell` | -0.007960455 |
| `preactjs/preact` | +0.005304680 |

An exact 256-draw nonparametric bootstrap over the four repository means has a
2.5% / median / 97.5% interval of `[-0.0971813, -0.0337243, +0.0019884]`.
Only 5.86% of those cluster-bootstrap means are positive. Every
leave-one-repository-out mean remains negative, ranging from -0.0467340 to
-0.00221846. With only four independent evaluation repositories, however, the
two-sided sign probability is 0.625; the frozen negative classification is not
a claim of conventional statistical significance.

At instance level there were 10 wins and 10 losses. The magnitude is dominated
by `facebook__docusaurus-9897`: raw retrieved the expected path and causal did
not, producing a -0.7004415 utility difference. Deleting it post hoc would make
the remaining 19-instance mean +0.0008764, but that task is valid and remains in
the result. This sensitivity does not change the repository majority: causal
still lost in three of four repositories.

## Failure-mode analysis

The sparse gate fired in six of ten causal generations and changed the raw
top-ranked choice in four. Across 24 finalist measurements, the pooled
raw-prediction/probe-gain Pearson correlation was 0.126; pairwise ordering was
concordant 11 times, discordant 23 times, and tied twice. The median absolute
probe gain was only 0.0003212. These diagnostics are consistent with a noisy or
unrepresentative small-shard sensor, but E118 cannot identify sensor noise,
repository transfer, and gate quality separately.

Repository heterogeneity clearly controls the magnitude. The hypothesis that
mutations simply have no effect is not supported: the 24 bootstrap mutation
gains span 0.0281411 and the final strategies produce a large path-recall change.
Stage-A dynamic range is mixed—measurable on train mutations, often tiny on
sparse probes. The probe cost did not buy value in E118: 44 incremental probe
packs, 11% of the naive probe budget, selected the worse final strategy.

The cheapest decisive prospective diagnostic is
`E118a-prospective-sensor-transfer-spec.md`: at most 480 new packs, zero model
calls, existing train/probe indexes only, and crossed one-task versus
whole-repository probe transfer. It must be frozen before execution and cannot
use or rescue the E118 evaluation repositories.

## Provenance, controls, and cost

- Dataset: `SWE-bench/SWE-bench_Multilingual`, revision
  `846e647b9f33c0b51b739d005d13d85493c9af09`, 300-row test split, parquet
  SHA-256 `92abca7cb527b41a9f66d03a26ce441ff7319e3a49f985998fd56be4bb9b08b2`.
- Selected-manifest SHA-256:
  `ccfbe787573dd51c688a92945d923f8d95c73e4811a78f35f9a8c6b1710075ce`.
- Execution environment: arm64 macOS, Python 3.14.7 with the exact
  `E118-requirements.txt` lock, `rustc 1.98.0`, `cargo 1.98.0`, and release
  `ce` SHA-256
  `c75dd2d140d6a89f1f7d3740816c8f3fa57c06f245c413ee3b0f26df3fb07ac7`.
- Every selected instance has an exact clean `repo + base_commit` checkout and
  a final validated HNSW plus logical SQLite binding. Gold content never enters
  packing, ranking, mutation, probing, or selection.
- Raw and causal arms receive the same 10-by-12 mutation-operation schedule.
  Evaluation repositories remain unopened until both trajectories terminate.
- Cost: 829 unique pack evaluations, 830 logical accesses, 57 indexes, 44
  incremental probe packs, 24 measured bootstrap candidates, 432 predicted
  calibration/trajectory candidates, 3 no-op mutations, 28 duplicate sibling
  configs, 0 invalid candidates, 0 failed Git or `ce` commands, 325.33 seconds
  cumulative unique-pack wall time, and 7,982.06 seconds cumulative index wall
  time. The successful resume took 243.75 seconds and reused 829 pack records.
- Two pre-result mechanical interruptions are preserved. Neither wrote a
  result, exposed a decision, or changed a scientific element.

The immutable result SHA-256 is
`5fb25a49aa4fda4cd0c5ea145ffb2a7480e10eeb1f2e63d5d046fb8b0f1ad1c9`.
The independent verifier recomputes the manifest, splits, index and pack
bindings, schedules, model, trajectories, utilities, costs, and decision. Eight
representative evidence corruptions are rejected.

## Claim boundary

E118 falsifies the preregistered promotion claim for this fixed dataset,
mutation genotype, ridge predictor, sparse-probe family, gate, seed, budget, and
Stage-A utility: sparse causal probing selected a materially worse strategy than
raw search on the held-out evaluation set. It does not prove sparse causal
feedback can never help Prune under another prospectively frozen sensor or
corpus.

E118 provides no evidence about actual SWE-bench issue resolution, patch
correctness, coding-agent capability, end-to-end cost per resolved issue,
recursive self-improvement, or multi-generation compounding. Because the
promotion gate failed, E119 was not authorized and was not run.
