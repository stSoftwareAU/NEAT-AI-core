# NEAT-AI-core

**Native shared Rust** for [NEAT-AI](https://github.com/stSoftwareAU/NEAT-AI): the **`neat-core`** crate (tests included) lives here as a Cargo workspace member.

## Test-driven development

Development in this repository follows **TDD**: do not merge behaviour changes unless **`cargo test --workspace`** already covers them (extend tests first when fixing bugs or adding APIs). Run **`./quality.sh`** before every commit/PR.

## WebAssembly

**`wasm_activation`** and **`pkg/`** remain in the **NEAT-AI** repo on `Develop` — not in this repository.

## Layout

| Path | Role |
|------|------|
| `neat-core/` | Shared computation library; **140+** unit tests in `src/**/*.rs` plus integration tests in `neat-core/tests/` (>350 total). |
| `Cargo.toml` | Virtual workspace root; `[workspace.package]` holds semver for release automation. |
| `deny.toml` | `cargo deny` (licences, advisories, bans). |
| `quality.sh` | Local gate (fmt, clippy, tests, doc, deny, bats). |
| `.github/workflows/ci.yml` | CI gate. The `rust-gates` job runs the lint (`cargo clippy -D warnings`) and compile/syntax (`cargo check --all-targets`) gates on **every push to `Develop` and every pull request**; the `quality` job is the full PR pipeline. |
| `bump-deps.sh` | Cargo dep refresh + audit + native/WASM build (Vibe Coder hook). |
| `.github/dependabot.yml` | Advisory-triggered security-update fast lane — raises a fix PR the moment a RustSec/OSV advisory lands, independent of the weekly bump. |
| `tests/scripts/` | `bats` suites for shell helpers (e.g. `bump-deps.sh`). |
| `LICENSE`, `.gitleaks.toml` | Inherited from NEAT-AI `Develop`. |

## Build

```bash
export RUSTFLAGS="-D warnings"
cargo test --workspace
# or full gate:
./quality.sh
```

## Related Repositories

The NEAT-AI project is split across seven public repositories. Each focuses on one concern and composes with the others as shown below.

| Repository | Role |
|------------|------|
| [NEAT-AI](https://github.com/stSoftwareAU/NEAT-AI) | Primary Deno/TypeScript neural-network engine (evolution, training, WASM activation). |
| [NEAT-AI-core](https://github.com/stSoftwareAU/NEAT-AI-core) | Shared native Rust library (`neat-core`) with numerics, topology helpers, and the chunked `.bin` training stream. |
| [NEAT-AI-Discovery](https://github.com/stSoftwareAU/NEAT-AI-Discovery) | Rust discovery module invoked by NEAT-AI via Deno FFI to search architectures and hyper-parameters. |
| [NEAT-AI-Snapshot](https://github.com/stSoftwareAU/NEAT-AI-Snapshot) | Creature/genome snapshot format and fixtures produced by NEAT-AI and consumed by downstream tools. |
| [NEAT-AI-scorer](https://github.com/stSoftwareAU/NEAT-AI-scorer) | Production forward-only scoring application built on `neat-core` via a path dependency. |
| [NEAT-AI-Explore](https://github.com/stSoftwareAU/NEAT-AI-Explore) | Visualiser for creatures that reads NEAT-AI-Snapshot data. |
| [NEAT-AI-Examples](https://github.com/stSoftwareAU/NEAT-AI-Examples) | Worked examples and tutorials that depend on NEAT-AI. |

### Dependency graph

```mermaid
graph TD
    Core[NEAT-AI-core<br/>Rust shared lib]
    Main[NEAT-AI<br/>Deno/TypeScript engine]
    Discovery[NEAT-AI-Discovery<br/>Rust, via Deno FFI]
    Snapshot[NEAT-AI-Snapshot<br/>creature data]
    Scorer[NEAT-AI-scorer<br/>Rust scorer app]
    Explore[NEAT-AI-Explore<br/>visualiser]
    Examples[NEAT-AI-Examples<br/>tutorials]

    Main -->|Deno FFI| Discovery
    Main -->|produces| Snapshot
    Scorer -->|path dependency| Core
    Explore -->|reads| Snapshot
    Examples -->|depends on| Main
```

## Propagation to downstream repositories

Once an enhancement merges to `Develop` here, it flows automatically to the
next pull request raised in either consumer repository — no manual SHA bump
is required. The two consumer paths differ in mechanism but share the same
Vibe Coder hook (`bump-deps.sh` runs before `quality.sh` on every PR).

### NEAT-AI (Deno + WASM consumer)

- On every push to `Develop`, [`.github/workflows/wasm-bundle.yml`](.github/workflows/wasm-bundle.yml)
  builds `wasm_activation-pkg.tar.gz` and publishes a per-commit GitHub
  Release tagged `wasm-bundle-<SHA>`.
- NEAT-AI's `bump-deps.sh` invokes `./build.sh`, which downloads the matching
  bundle and updates `deno.json`'s `neatCore.rev` field in lock-step.
- A fresh PR in NEAT-AI is therefore sufficient to pick up the latest
  `Develop` of NEAT-AI-core.
- The published bundle (and its CycloneDX SBOM) carries a Sigstore-backed
  build-provenance attestation, so a consumer can cryptographically confirm
  the tarball was produced by this workflow from the pinned commit before
  trusting it. Verify in the bump flow with:

  ```bash
  gh attestation verify wasm_activation-pkg.tar.gz \
    --repo stSoftwareAU/NEAT-AI-core
  ```

  Pinning by SHA and the post-publish content re-verification prove the
  bundle's *shape*; the attestation proves its *origin*, closing the
  published-artefact-substitution attack class.

### NEAT-AI-scorer (Rust + path dependency)

- NEAT-AI-scorer's CI uses `actions/checkout` to clone
  `stSoftwareAU/NEAT-AI-core@Develop` into the workspace on every PR.
- `rust_scorer/Cargo.toml`'s `path = "../../NEAT-AI-core/neat-core"` resolves
  against that fresh clone, so the next PR build always compiles against the
  current tip of `Develop`.
- No SHA pin or release artefact is involved on this path.

### End-to-end flow

```mermaid
sequenceDiagram
    actor Dev as Maintainer
    participant Core as NEAT-AI-core/Develop
    participant CI as wasm-bundle.yml
    participant Rel as GitHub Release<br/>wasm-bundle-{SHA}
    participant Main as NEAT-AI PR
    participant Scorer as NEAT-AI-scorer PR

    Dev->>Core: merge enhancement
    Core->>CI: push to Develop
    CI->>CI: attest build provenance (Sigstore, keyless)
    CI->>Rel: build & publish wasm_activation-pkg.tar.gz
    Dev->>Main: open PR
    Main->>Rel: bump-deps.sh -> build.sh download
    Main->>Rel: gh attestation verify (origin proof)
    Note over Main: deno.json neatCore.rev advances
    Dev->>Scorer: open PR
    Scorer->>Core: actions/checkout @Develop
    Note over Scorer: path dep resolves fresh clone
```

### Wiring reference

| Consumer | Trigger | Script / workflow |
|----------|---------|-------------------|
| NEAT-AI-core | push to `Develop` | [`.github/workflows/wasm-bundle.yml`](.github/workflows/wasm-bundle.yml) |
| NEAT-AI | PR opened (Vibe Coder hook) | `NEAT-AI/bump-deps.sh` → `NEAT-AI/build.sh` |
| NEAT-AI-scorer | PR opened (CI) | `NEAT-AI-scorer/.github/workflows/ci.yml` (`actions/checkout` of `NEAT-AI-core@Develop`) |

### Race window

`wasm-bundle.yml` typically takes ~30–60 seconds to build and publish the
release after a merge to `Develop`. PRs raised in NEAT-AI inside that
window may transiently fail the bundle download in `build.sh` because the
release tag for the latest `Develop` SHA does not yet exist. Re-run the
PR's checks once the bundle workflow has completed, or wait a minute
before opening the PR.

## Dependency updates: two channels

Dependency refresh runs on two complementary channels so the urgent
"patch this advisory now" path is decoupled from the routine weekly bump:

- **Routine bump** — [`.github/workflows/upgrade-dependencies.yml`](.github/workflows/upgrade-dependencies.yml)
  runs `bump-deps.sh` every Monday (`cron "0 6 * * 1"`), applying the
  `VIBE_BUMP_QUARANTINE_HOURS` release-age quarantine, `cargo audit`, and
  dual native/WASM builds before raising a general upgrade PR.
- **Security fast lane** — [`.github/dependabot.yml`](.github/dependabot.yml)
  enables Dependabot's Cargo **security-updates** channel. When a
  RustSec/OSV advisory lands against a crate already in `Cargo.lock`,
  Dependabot raises a fix PR immediately — independent of the weekly window.

Advisory *detection* still lives in [`security.yml`](.github/workflows/security.yml)
and the `ci.yml` `security` job (`cargo audit` / `rustsec/audit-check`); the
new channel is what *raises* the remediation PR rather than waiting for Monday.

When an actively-exploited advisory's fix is newer than the
`VIBE_BUMP_QUARANTINE_HOURS` window, an approver can take the documented
**emergency quarantine override** — dispatch *Upgrade Cargo Dependencies* with
`emergency_bypass: true` (or run `./bump-deps.sh --quarantine-hours 0`) and
confirm `cargo audit` is clean before merge. See
[`SECURITY.md`](SECURITY.md#emergency-quarantine-override).

```mermaid
flowchart TD
    Adv[RustSec/OSV advisory disclosed] --> Detect[cargo audit detects<br/>security.yml / ci.yml]
    Detect -->|fails PR / scheduled job| Alert[Maintainer alerted]
    Adv --> Dependabot[dependabot.yml<br/>security-updates channel]
    Dependabot -->|immediate| FixPR[Advisory fix PR]
    Cron[Weekly cron Mon 06:00] --> Bump[upgrade-dependencies.yml<br/>bump-deps.sh]
    Bump -->|general refresh| GenPR[Weekly upgrade PR]
    FixPR --> Develop[Develop]
    GenPR --> Develop
```

## License

Apache-2.0 — see `LICENSE`.
