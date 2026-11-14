# Sparse precision protocol v3 — frozen before final validation

Part of issue #758. This is a reporting correction to
[protocol v2](protocol-v2.md), whose factors, controls, accounting, thresholds,
compute limits and adoption rule remain in force. The fresh validation network
seed is 22002 and record seed 23003, as recorded in
[manifest-v3.json](manifest-v3.json). Calibration seed 1001 is unchanged.

The v2 Linux run completed every planned cell and remains valid historical
single-host evidence. A subsequent regression test found an uncovered reporting
boundary: a process whose total budget prevents *every* measurement still called
its host covered. V3 reports a platform as covered only when at least one variant
has timing samples. If none do, every required platform stays in the missing
list. Per-cell completion and variant counts must still be read before claiming
full coverage of a measured platform. Metal remains explicitly unrun.

No kernel, representation, numerical threshold or selection rule changed in this
revision. The new seeds honour v2's freeze rule; no tuning follows either run.
Do not pool v1, v2 and v3 samples. Commit this protocol and the reporting fix
before measuring; the results must identify that commit. V3 keeps results schema
version 2 and uses its own filenames and protocol identifier.
