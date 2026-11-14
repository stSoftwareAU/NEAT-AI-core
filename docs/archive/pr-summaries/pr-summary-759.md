## Summary

Added generic O(V+E) reachability diagnostics and an opt-in paired connection-mutation experiment. Corrected cost and failure accounting, preserved the original inconclusive result, and completed a predeclared 160-pair comparison. The overall decision is **do not adopt**; no production mutation or pruning default changes. Closes #759.

## Spec

### Intent and Rationale

- Fixed-point cleanup already removes forward-only dead structure. Reachability adds diagnostics without introducing another pruning implementation.
- The original experiment used bridge selection on about 4% of proposals and omitted shared preparation costs. Extended generated workloads exercise sustained bridge opportunities and separate scoring from preparation/search.
- Mixed and conditional families favoured bridge, while sparse and legacy families remained inconclusive. This does not establish a representative product benefit or reproduce the cited paper.

### Essential Design Decisions

- The public API is diagnostic only. Bridge policy stays private to the example; parents are cloned, topology validation precedes cleanup's authoritative validation, and acceptance requires finite strict calibration improvement.
- `docs/research/reachability-mutation-protocol-v2.md` fixes four families, 40 paired seeds each, 150 offspring evaluations, 600 proposals, untouched validation records and 12,000 bootstrap resamples with 98.75% intervals. Research execution is separate from CI.
- Initial teacher/data/cleanup preparation is charged once per pair and allocated half to each arm. Calibration and validation scoring include compilation. DeadEnd/Isolated structure is reported separately from BiasDriven paths.
- New filenames and schema version 2 separate corrected results from the retained historical JSON. Manifest execution requires a matching committed source tree, embedded source, executable and protocol; incomplete coverage cannot produce a success verdict.

### Undiscoverable Facts

- The paper was accessible on 14 November 2026. Its identity and pruning/regrowth method were verified against arXiv v1; the research record links the sources and explains the method differences.
- The manifest was declared before execution at `0d7f98e0eda1bcd3f7374c413fd3a02fc124a272`. The subsequent empty-offspring-mean serialisation fix did not affect these trials: the smallest arm had 98 calibration evaluations, including its initial parent. Policies, seeds and generators were unchanged.
- Timings came from a shared, parallel host and are indicative only. Build and declaration costs are recorded separately; the harness does not time engineering labour.
- Thirteen earlier completion attempts ended `infrastructure-error` in the worker's pre-push gate, which read the pinned `dtolnay/rust-toolchain@29eef336…` SHA in `.github/workflows/ci.yml` as a toolchain name (`cargo +29eef336… --version` exited 101). That is a worker defect outside this repository; this PR does not change the workflows and no gate was bypassed.

## Evidence

Backend/CLI only; no UI files changed.

```mermaid
flowchart LR
    S[Committed source and predeclaration] --> M[Matching manifest]
    M --> P[Shared teacher, records and cleaned parent]
    P --> U[Unbiased proposals]
    P --> B[Bridge proposals]
    U --> V[Topology validation and cleanup]
    B --> V
    V --> C[Calibration acceptance on clones]
    C --> F[Initial and final validation scores]
    F --> R[Paired distributions, uncertainty and coverage]
```

- `docs/research/reachability-mutation-v2-manifest.json`, `docs/research/reachability-mutation-v2-results.json` and `docs/research/reachability-mutation-v2-execution.json` record provenance, commands, all 160 paired trials, uncertainty, costs and zero failures.
- Mean validation-MSE differences (bridge minus unbiased), with 98.75% intervals: legacy -0.00821 [-0.02988, 0.00609]; sparse -0.00217 [-0.02866, 0.03058]; mixed -0.11111 [-0.18454, -0.04901]; conditional -0.06943 [-0.13023, -0.02436].
- Bridge-selected proposals: legacy 224/5,939; sparse 2,571/6,000; mixed 5,017/6,000; conditional 2,093/6,000. All extended arms reached their evaluation budget. Seven legacy arms stopped at an empty pool; none hit the proposal cap.
- Both arms had zero invalid offspring and zero final dead structure. Those metrics cannot demonstrate an improvement in this already-cleaned, addition-only experiment. BiasDriven paths remain separately counted.
- Indicative costs: shared preparation 0.04485 s; arm search 7.38602 s; executable before output 7.45319 s; release build 31.55468 s. Per-evaluation-hour and allocated preparation/search-hour rates are recorded per arm, with no timing threshold or production-throughput claim.
- `docs/research/reachability-guided-mutation.md` contains the cleanup audit, historical limitations, method verification, reproduction command and observed results. The private filter earns retention as the reproducer, not as a supported production policy.
- #759: Experiment: Measure reachability-guided pruning and connection mutation. Latest `origin/Develop` was `b426a98`; no merged reachability PR was found.

## Test Plan

- `neat-core/tests/reachability_diagnostics.rs`: 10 passing tests cover hand-derived classes/counts, cycles, duplicate/unsorted edges, unused inputs, constant outputs, typed refusals with legal controls, bounded widths and cleanup integration.
- `neat-core/examples/reachability_mutation_experiment/tests.rs`: 14 passing tests cover exact bridge subsets, malformed buffers, unknown empty summaries/rates, separate dead/BiasDriven counts, invalid scoring data, strict acceptance, immutable parents, validation isolation, generated aggregate/IF semantics, bridge/fallback/empty selection, completion limits, deterministic cost denominators and statistical decisions.
- `neat-core/examples/reachability_mutation_experiment/driver.rs`: 3 passing tests cover manifest/source mismatch, incomplete coverage/failure refusal and actual scoring-error causes alongside successful paired results.
- Four measurement tests failed at checkpoint `58d04e8` before the fixes: empty statistics reported zero, an unmeasured evaluation rate used wall time, dead structure was not separately reported, and empty scoring data returned NaN. Develop contained no harness, so these are checkpoint regressions, not a claimed reproduction of a released defect. The original diagnostic implementation had no observed failing-first run.
- `proposal_limit_is_incomplete_but_an_empty_pool_is_a_valid_terminal_state` additionally failed at `0d7f98e` when an offspring mean with no observations was zero; it now asserts null. No pre-existing behaviour assertions were removed. The two original bridge tests moved into the example's test module with their outcome assertions preserved.
- `cargo test -p neat-core --example reachability_mutation_experiment`: passed, 17 tests. `cargo test -p neat-core --test reachability_diagnostics --test prune_cleanup --test if_graft`: passed, 108 tests.
- `cargo test --workspace -q`: passed on the final code, including example tests and doctests. `./quality.sh < /dev/null`: passed on head `3f016ae` (the last code commit; later commits touch only this summary), printing `All quality checks passed!`. Targeted clippy and Markdown lint also passed.
- `scripts/check-downstream-consumers.sh`: passed for all six registered consumers against this core. The subsequent change only affects private example serialisation and tests.
- Artifact checks confirmed the manifest matches the results, all 160 pairs are present, proposal counts equal acceptance plus rejection counts, and calibration-call counts equal evaluations.
- Callers checked: `driver::run_prepared` executes both arms; `run_arm` owns proposal, acceptance and timed scoring paths. Initial calibration, initial validation, offspring calibration and final validation use the cost recorder. No second experiment-accounting implementation was found.
- Docs sweep covered README, active research docs, AGENTS and the example's module documentation for reachability, bridge selection, old flags, cost labels and paper status. Updated the research record, added the v2 protocol and corrected the example command. No production API was removed or narrowed.

## Security self-check

- Scoring refuses empty, mismatched and non-finite data; candidates use the existing topology and cleanup validators. IF construction uses the canonical graft helper.
- CLI subprocesses use argument arrays; manifest matching refuses edited declarations. Output uses new files rather than overwriting existing files.
- Only explicit source, test and research paths were staged; no secrets or hidden files were staged. No experiment dependency or privileged endpoint was added.

## Merge verification

- [x] Focused and workspace tests, local quality gate and downstream compatibility.
- [x] Predeclared comparison and machine-readable evidence.
- [ ] Remote CI and code review: pending.

## Not checked

Private/product corpora, guided DST replication and wasm builds were not run. No representative optimisation benefit is established. Remote CI has not yet run on this branch.
