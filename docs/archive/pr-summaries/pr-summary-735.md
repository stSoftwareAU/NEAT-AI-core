## Summary

Quiets `cargo test` on the green path. Both the lib/integration suite and the
doctest suite now run with `-q` in `quality.sh` and in `ci.yml`'s "Run tests" /
"Run doctests" steps, so a passing run no longer prints one
`test <name> ... ok` line per test. Failure detail is unchanged. Closes #735.

## Evidence

- `tests/scripts/quiet_cargo_test.bats` runs the **live** commands (taken from
  `quality.sh` and from `ci.yml` through `extract_step`) against a throwaway
  crate:
  - before the fix: the two green-path tests failed, because output held
    `test tests::probe_case ... ok` and `test src/lib.rs - add_one (line 3) ... ok`;
  - after the fix: all 5 pass.
- A failing unit test and a failing doctest still print the test name, the
  assertion message and `panicked at` under `-q`. This was asserted, not
  assumed.
- `./quality.sh < /dev/null` passes, and its log has zero `... ok` per-test lines.

## Test Plan

- Added `tests/scripts/quiet_cargo_test.bats`:
  - `quality.sh` and `ci.yml` run the same cargo test commands (keeps local and CI output identical);
  - a green test suite / green doctest suite prints no per-test ok line;
  - a failing test / failing doctest still prints its name, message and panic site.
