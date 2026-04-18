# NEAT-AI-core

**Native shared Rust** for [NEAT-AI](https://github.com/stSoftwareAU/NEAT-AI): the **`neat-core`** crate (tests included) lives here as a Cargo workspace member.

## Test-driven development

Development in this repository follows **TDD**: do not merge behaviour changes unless **`cargo test --workspace`** already covers them (extend tests first when fixing bugs or adding APIs). Run **`./quality.sh`** before every commit/PR.

## WebAssembly

**`wasm_activation`** and **`pkg/`** remain in the **NEAT-AI** repo on `Develop` — not in this repository.

## Layout

| Path | Role |
|------|------|
| `neat-core/` | Shared computation library; **233+** unit tests in `src/**/*.rs`. |
| `Cargo.toml` | Virtual workspace root; `[workspace.package]` holds semver for release automation. |
| `deny.toml` | `cargo deny` (licences, advisories, bans). |
| `quality.sh` | Local gate (fmt, clippy, tests, doc, deny). |
| `LICENSE`, `.gitleaks.toml` | Inherited from NEAT-AI `Develop`. |

## Build

```bash
export RUSTFLAGS="-D warnings"
cargo test --workspace
# or full gate:
./quality.sh
```

## License

Apache-2.0 — see `LICENSE`.
