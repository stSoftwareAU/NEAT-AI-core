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

echo "📖 Building documentation..."
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps

echo "🏗️ Building release..."
cargo build --workspace --release

echo "✅ All quality checks passed!"
