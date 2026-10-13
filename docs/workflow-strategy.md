# Parallel workers and deferred integration validation

## Status and purpose

Accepted strategy for future implementation batches. It supersedes the per-delivery validation and promotion used in PoC 1 and PoC 2. Their historical records and saved scripts remain unchanged.

Keep parallel implementation and linear integration, but validate the assembled delivery rather than rerunning the full checks after every merge. The last workflow spent 5m49s in four serial integrators, with approximately 88% recorded as model-response time. Repeated agent turns dominated command time; model timings include request latency and generation. Final integration also stopped on a one-line test lint because of restrictive file ownership; the correction is recorded in commit `4a724bd`.

## Workers

- Run at most four workers concurrently, each in an isolated Git worktree.
- Define module responsibilities, interfaces, file ownership and task dependencies before starting.
- Start dependency-ready tasks immediately when a worker slot is available. Dependent workers base their work on the integration branch containing their prerequisites.
- Run focused regression tests and appropriate formatting/lint checks before delivery, including test code where supported. Report any blocked or unrun checks; never label them passed.
- Commit completed work and hand off its branch, commit, scope and check results. Do not rebase a branch while its worker is still editing it.

## Integrator

Use one persistent integrator and one integration worktree for the batch. Retain its Cargo build cache. Keep workers' build outputs isolated.

1. Queue committed deliveries in completion order.
2. If the integrator is idle, start draining the queue immediately. Otherwise append to its existing queue; do not start another integrator.
3. Rebase each delivered private worker branch onto the current integration branch, resolving conflicts sequentially, then fast-forward the integration branch to the rebased delivery.
4. Resolve mechanical conflicts while preserving module contracts. For ambiguous semantic conflicts, assign a worker fix rather than blindly choosing ours or theirs.
5. Record the original and rebased commits and the integration outcome. Integrated prerequisites unlock dependent tasks without waiting for the full validation suite.

Do not run build, tests or Clippy after every delivery. Do not create a fresh integrator agent or worktree per commit. Ordinary Git operations should be deterministic; agent reasoning is for conflicts and repair coordination.

The integration branch may temporarily be broken. Keep `main` unchanged during assembly. Never force-update or rewrite `main`, overwrite local changes, or rewrite unrelated published branches.

## Final validation and repair

Begin validation when all admitted tasks are delivered, no workers remain active, the integration queue is empty and no conflicts remain.

Run against the assembled integration HEAD:

```sh
task build
task test
task check
```

Run applicable smoke checks for the increment as well. Record hardware/manual checks separately as passed, failed or untested; automated routing tests do not establish physical input behavior.

If validation fails:

1. Create focused fix tasks with the failing command, diagnostics, affected files and expected result.
2. Dispatch dependency-ready fixes to workers under the same four-worker limit. Authorize the necessary scope; a cross-module issue must not stall solely because of the original ownership boundaries.
3. Workers test and commit their fixes. Feed those commits through the same serial integration queue.
4. Once fixes are assembled and the queue is drained, rerun final validation. Repeat until required checks pass, or report a blocker requiring a decision.

Do not disable failing checks or silently drop failures. Record the validated commit and commands. Fast-forward `main` only to that validated commit. If `main` advanced in the meantime, rebase the integration branch onto it, resolve conflicts and rerun validation before promotion.

## Tradeoff

Combined-code problems are discovered later, so worker checks remain important. In exchange, integration avoids repeated full-project verification and repeated agent/context setup. `main` remains the last validated delivery throughout the batch.
