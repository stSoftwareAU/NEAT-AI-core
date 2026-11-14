# Replay the retired MulClip experiment

The v2 result is negative. Its executable harness, example and integration test
were removed from routine Cargo targets after measurement. `harness-v2.patch`
is an immutable research artifact, containing those sources and both manifests
from measured revision `5a259b1413d80e0ad06811310aa3d91e7f4fdf43`.
It applies to core revision `b426a98ceb8707204231f8e7ebdcd8d1665f9612`.
The common benchmark fixture and public scoring implementation come from that
base, rather than a copied implementation in the artifact.

Patch SHA-256: `f819c595abac774d6db8ff7945ea5683b7327279465562076bdd1a4a11d54b14`.

The replay was exercised in a separate worktree: application succeeded and the
archived numerical, inference, manifest and assessment suite passed 25 tests.
This archive is not a supported consumer activation or a live benchmark target.
To reproduce it without changing the main checkout:

```bash
TASK_ROOT="$(pwd)"
REPLAY_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/mulclip-v2.XXXXXX")"
# Fetch the exact base explicitly; a shallow clone need not contain it.
git fetch origin b426a98ceb8707204231f8e7ebdcd8d1665f9612
git worktree add --detach "$REPLAY_ROOT/worktree" b426a98ceb8707204231f8e7ebdcd8d1665f9612
cd "$REPLAY_ROOT/worktree"
git apply --check "$TASK_ROOT/docs/research/mulclip/harness-v2.patch"
git apply "$TASK_ROOT/docs/research/mulclip/harness-v2.patch"
cargo test -p neat-core --test mulclip_experiment < /dev/null

# Use gtimeout on macOS when coreutils provides that name.
timeout 600 cargo run --release -p neat-core --example mulclip_experiment -- \
  --manifest docs/research/mulclip/manifest-v2.json \
  --out "$REPLAY_ROOT/results.json" \
  --code-rev "b426a98+harness-v2.patch@f819c595abac774d6db8ff7945ea5683b7327279465562076bdd1a4a11d54b14" \
  --rustc "$(rustc -vV)" < /dev/null

# Request an explicit assembly path; Cargo's intermediate names vary.
cargo rustc --release -p neat-core --example mulclip_experiment -- \
  --emit="asm=$REPLAY_ROOT/mulclip.s" < /dev/null
```

Observed toolchains were Rust 1.99.1 on ARM Linux and 1.100.0 on hosted
x86 Linux/Apple Silicon. Portable release settings were optimisation level 3,
fat LTO and one codegen unit. Preserve the new replay's toolchain and source
provenance; do not relabel a replay as one of the committed measurements.
The source and data are frozen, while hosted timing noise is not reproducible
bit for bit.

## Replay v1

The v1 sources are archived the same way in `harness-v1.patch`: the v1
harness, example, integration test and `manifest.json`, as they stood at
historical revision `9e506f9484d7a01c6bac5b00bce25d79bca2d32d` (an
unreferenced commit that may stop being fetchable, so the patch is the durable
copy). It applies to the same base,
`b426a98ceb8707204231f8e7ebdcd8d1665f9612`.

Patch SHA-256: `160fe6a7eb86ed2dd6c2a05bc7c5a40d73a82f4e801ccd3977ca1ed6204b0b50`.

The patch carries only those four files; v1's hosted CI lane and
`neat-core/benches/README.md` note are not archived. The replay was exercised
in a separate worktree: the patch applied, the four files matched the
historical blobs byte for byte, and the archived suite passed 20 tests.

```bash
TASK_ROOT="$(pwd)"
REPLAY_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/mulclip-v1.XXXXXX")"
git fetch origin b426a98ceb8707204231f8e7ebdcd8d1665f9612
git worktree add --detach "$REPLAY_ROOT/worktree" b426a98ceb8707204231f8e7ebdcd8d1665f9612
cd "$REPLAY_ROOT/worktree"
git apply --check "$TASK_ROOT/docs/research/mulclip/harness-v1.patch"
git apply "$TASK_ROOT/docs/research/mulclip/harness-v1.patch"
cargo test -p neat-core --test mulclip_experiment < /dev/null
```

`harness-v2.patch` intentionally reproduces the corrected v2 method, not v1.
