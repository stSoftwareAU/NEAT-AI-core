## Summary

The "What is measured" table in `neat-core/benches/README.md` listed eight of
the nine benchmark groups defined in `neat-core/benches/hot_paths.rs` —
`squash_x4` (`hot_paths.rs:515`) was missing. Added one row for it, next to the
related `squash` row, describing what the code actually does: the lane-parallel
`squash_x4` approximation (Issue #180) against a four-call `apply_squash`
baseline, over `Tanh`/`Logistic`/`Gelu`/`Mish`, with `scalar_x4` and `simd_x4`
naming the two sides. The size column takes the fixed 4-lane fixture
`[0.42, -1.3, 2.7, -0.05]` straight from the setup at `hot_paths.rs:517`.

Every `c.benchmark_group("…")` name in `hot_paths.rs` now has a table row.
Closes #508.

## Evidence

Docs-only change to a README table — no code, tests, or CI behaviour depends on
it, so there is no UI to screenshot and no benchmark to run.

Group names in `hot_paths.rs` versus README rows after the change:

| `hot_paths.rs` group | Line | README row |
| --- | --- | --- |
| `forward_pass` | 41 | ✅ |
| `batched_scoring` | 63 | ✅ |
| `backprop` | 144 | ✅ |
| `reverse_topological_order` | 176 | ✅ |
| `scoring` | 213 | ✅ |
| `topology_ops` | 344 | ✅ |
| `weighted_sum_simd` | 396 | ✅ |
| `squash` | 487 | ✅ |
| `squash_x4` | 515 | ✅ (added) |

`./quality.sh` passes cleanly.

## Test Plan

No tests added. AGENTS.md ("Testing: 'what' not 'how'") rules out source-grep
tests, which is the only shape a README-versus-source consistency check could
take; the issue's own Failure Detection section records the same conclusion
(docs-only, drift resurfaces via a future documentation-audit scan). Existing
`cargo test --workspace` suite re-run green via `./quality.sh`.
