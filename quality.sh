#!/bin/bash
# Local gate — mirrors NEAT-AI-Discovery `quality.sh` (virtual workspace: neat-core).
# Toolchain: use rustup `stable` (no rust-toolchain.toml; same idea as NEAT-AI-discovery).
# Run `rustup update stable` periodically so local Clippy matches CI.
set -euo pipefail

# Source cargo environment if available (needed for non-login shells)
if [ -f "$HOME/.cargo/env" ]; then
    # shellcheck disable=SC1091
    source "$HOME/.cargo/env"
fi

export RUSTFLAGS="-D warnings"
echo "🔍 Pre-deployment Quality Check"
echo "================================"

# Check bash script syntax
echo "📝 Checking bash script syntax..."
find . -name "*.sh" -type f -not -path "./target/*" -not -path "./.git/*" -exec bash -n {} \;

echo "Running shellcheck on bash scripts..."
if ! command -v shellcheck &>/dev/null; then
    echo "shellcheck is required — install: https://github.com/koalaman/shellcheck#installing"
    exit 1
fi
SHELLCHECK_FAILED=0
while IFS= read -r script; do
    echo "  shellcheck: $script"
    if ! shellcheck -s bash "$script"; then
        SHELLCHECK_FAILED=1
    fi
done < <(find . -name "*.sh" -type f -not -path "./target/*" -not -path "./.git/*")
if [[ "$SHELLCHECK_FAILED" -ne 0 ]]; then
    echo "shellcheck: FAILED"
    exit 1
fi
echo "shellcheck: all scripts passed"

# Bash helper tests (bats) — covers bump-deps.sh and any future shell helpers.
# Fail loud (Issue #631): `bash -n` and shellcheck above read the shell scripts
# without running them, so this is the only gate over their behaviour — a
# missing binary or a missing/empty suite must fail rather than warn and let the
# run reach "All quality checks passed!".
# The block between the markers below is extracted and executed verbatim by
# tests/scripts/quality_bats_gate.bats — keep it self-contained.
# >>> bats-gate
echo "🧰 Running bash helper tests (bats)..."
if ! command -v bats &>/dev/null; then
    echo "bats is required — install: brew install bats-core (or your package manager, e.g. sudo apt-get install -y bats)"
    exit 1
fi
if [ ! -d "tests/scripts" ]; then
    echo "tests/scripts is required — the bats shell-harness suite is missing"
    exit 1
fi
if [ -z "$(find tests/scripts -maxdepth 1 ! -type d -name '*.bats')" ]; then
    echo "tests/scripts holds no *.bats files — the shell-harness suite is empty"
    exit 1
fi
bats tests/scripts
# <<< bats-gate

# Downstream consumers gate (Issue #644) — mirrors the CI downstream-consumers
# job: every repository in scripts/downstream-consumers.txt must still compile
# against this core. Locally it compiles the sibling checkouts beside this repo,
# which is minutes of cargo work and needs them cloned, so it is opt-in here;
# CI runs it on every pull request regardless.
if [[ "${QUALITY_DOWNSTREAM:-0}" == "1" ]]; then
    echo "🧩 Compiling registered downstream consumers against this core..."
    ./scripts/check-downstream-consumers.sh --workspace ..
else
    echo "ℹ️  Downstream consumers gate skipped locally (QUALITY_DOWNSTREAM=1 to run; CI always runs it)"
fi

# TypeScript basic-validity gate (Issue #307) — mirrors the CI typescript-gate job.
echo "🧾 Checking TypeScript sources (deno check)..."
./scripts/typescript-check.sh </dev/null

# TypeScript style gate (Issue #647) — mirrors the CI typescript-gate job.
# `deno check` above proves the helpers compile; these prove they are linted and
# formatted. `fmt.include` in deno.json gives the formatter JavaScript and
# TypeScript only, so Markdown keeps its single owner in markdownlint-cli2 and
# the committed JSON keeps whatever shape its writer gives it; the top-level
# `exclude` drops the generated wasm-pack output under
# neat-core/wasm_activation/pkg/, which .gitignore does not cover.
# The block between the markers below is extracted and executed verbatim by
# tests/scripts/deno_style_gate.bats — keep it self-contained.
# >>> deno-style-gate
echo "🧹 Linting TypeScript sources (deno lint)..."
deno lint </dev/null
echo "🎨 Checking TypeScript formatting (deno fmt --check)..."
deno fmt --check </dev/null
# <<< deno-style-gate

# Mermaid gate (Issue #379) — repo-owned, always-on; mirrors the CI
# markdown-lint job. Fails loud: a missing deno exits non-zero under `set -e`.
echo "🧜 Validating Mermaid blocks..."
deno test --allow-read --allow-write tests/check_mermaid_test.ts </dev/null
deno run --allow-read scripts/check_mermaid.ts . </dev/null

# Native/WASM pruning parity record (Issue #592) — the comparator and the
# committed golden record, which are what stop the wasm entry surface drifting
# from the native one. The end-to-end check needs a built bundle and runs in
# wasm-bundle.yml; this half runs everywhere.
echo "🌿 Checking the native/WASM pruning parity record..."
deno test --allow-read tests/wasm_prune_parity_test.ts </dev/null

# JSR supply-chain gate (Issue #603) — mirrors the CI typescript-gate step.
# The committed `deno.json` quarantine (24h `minimumDependencyAge`) and the
# frozen `deno.lock` are what keep every Deno gate off a freshly-published JSR
# release; this test fails loud if either is removed or weakened.
echo "🔒 Checking JSR dependency quarantine and lockfile..."
deno test --allow-read --allow-write --allow-run=deno tests/deno_supply_chain_test.ts </dev/null

# Optional: codespell (CI runs this; install: pip install codespell)
if command -v codespell &>/dev/null; then
    echo "📖 Running codespell..."
    codespell
else
    echo "⚠️  codespell not installed — skipping (CI still enforces spelling)"
fi

# Update dependencies to latest versions (including incompatible upgrades)
echo "📦 Upgrading Rust library dependencies..."
if command -v cargo-upgrade &>/dev/null; then
    cargo upgrade --incompatible
    cargo update
else
    echo "⚠️  cargo-edit not installed — skipping dependency upgrade"
    echo "   Install with: cargo install cargo-edit"
fi

# Licence and dependency audit
echo "📜 Running licence and dependency audit..."
if ! command -v cargo-deny &>/dev/null; then
    echo "cargo-deny is required — install: cargo install cargo-deny --locked"
    exit 1
fi
cargo deny check
# The wasm-bench harness sits outside the root virtual workspace (Issue #607),
# so its own dependency graph needs its own pass under this same deny.toml —
# the licence allow-list and unknown-registry = "deny" included.
cargo deny --manifest-path wasm-bench/Cargo.toml check

# Use workspace for faster builds
echo "🏗️ Building (debug) for quick feedback..."
cargo build --workspace

echo "🪄 Auto-formatting code..."
cargo fmt --all

echo "🔧 Running linter..."
# Lint rules are configured in Cargo.toml [lints] / [workspace.lints] — do not add -D/-W flags here (NEAT-AI-Discovery Issue #876 pattern)
cargo clippy --workspace --all-targets --all-features -- -D warnings

echo "✅ Running type checks..."
cargo check --workspace --all-targets --all-features

echo "🧪 Running tests..."
# neat-core is library + integration tests only (no [[bin]]); --bins is harmless if added later
cargo test --workspace --lib --tests --all-features -- --test-threads=2

# Issue #562 — doctests are part of the public API surface (the
# `creature_validate` examples), so a doctest that stops compiling must fail
# the gate rather than pass unrun.
echo "🧪 Running doctests..."
cargo test --workspace --doc --all-features

echo "📖 Building documentation..."
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps

echo "🏗️ Building release..."
cargo build --workspace --release

echo "✅ All quality checks passed!"
