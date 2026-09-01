# E118a prospective sensor-transfer diagnostic specification

Status: specified after the frozen negative E118 result; not preregistered, not
executed, and not a scientific result. Freeze this specification on a new
experiment branch before any new pack measurement.

## Question

Does E118's sparse one-shard probe select mutations that transfer to a different
development repository, and can a full-repository probe separate task-shard
noise from repository heterogeneity?

This is the cheapest decisive diagnostic of the leading E118 failure modes. It
does not retest the original claim and cannot promote to E119.

## Data boundary

- Use only E118 train and probe repositories and their exact recorded
  `repo + base_commit` indexes.
- Never open, score, fit on, or select with an E118 evaluation repository.
- Continue using only the problem statement for packing and gold changed paths
  for scoring.
- Generate a new candidate-panel seed and freeze every parent, mutation, and
  candidate digest before measuring any new panel.

## Frozen comparison to create

1. Use 12 independent static panels of 12 matched mutation opportunities.
2. Cycle prospectively among the E118 common base, raw-final, and causal-final
   parents; these strategies were fixed before E118 evaluation access.
3. Apply the frozen raw ridge predictor and retain its top four candidates in
   each panel.
4. For the eight E118 probe tasks, measure the parent and four finalists once.
5. Compute selection three ways without changing candidates: raw top-one,
   E118-style one-task shard, and whole-source-repository probe.
6. For every ordered pair of distinct probe repositories, score the selected
   candidate on the target repository. The repository is the inferential unit;
   ordered pairs are repeated descriptive contrasts and must be clustered by
   source and target repository, never counted as independent pack-level data.

The upper bound is 480 new pack evaluations (`12 panels * 5 configs * 8 probe
tasks`), with zero model calls and reuse of eight exact-state indexes.

## Decision table

- One-task selection fails but full-repository selection transfers: the E118
  sensor is too noisy at its frozen shard size.
- Both selections improve their source repository but fail on the target: the
  probe family is repository-specific and unrepresentative.
- Neither selection improves even its source: the measured causal response has
  insufficient useful dynamic range for this genotype.
- Both transfer but E118 still failed: the margin gate or sequential trajectory
  interaction is the leading failure mode; freeze a separate gate experiment.

Predeclare paired repository-cluster estimates, all exclusions, exact pack
budget, and a no-tuning rule. A positive diagnostic only justifies designing a
new confirmation on repositories absent from all 13 E118 repositories. It does
not rescue or amend the E118 decision.
