# Security Scan — Issue #73

Static, evidence-backed MythOS-style security audit of `stSoftwareAU/NEAT-AI-core`.
Read-only inspection — no files in the repo were modified. The JSON block below is
the machine-readable artefact consumed by `security_issue_filer`; the Markdown
sections below it are for human reviewers.

```json
{
  "schema": "mythos-security-scan/v1",
  "repo": "stSoftwareAU/NEAT-AI-core",
  "scanStartedAt": "2026-05-18T00:00:00Z",
  "scanFinishedAt": "2026-05-18T00:30:00Z",
  "coverage": {
    "chunksPlanned": 6,
    "chunksAudited": 6,
    "filesRead": 18
  },
  "findings": [
    {
      "id": "a1c4ne01",
      "class": "supply-chain:quarantine-misconfigured",
      "severity": "High",
      "severityRationale": "AV:N/AC:H/PR:N/UI:N/S:C/C:H/I:H/A:H — the per-PR CI pipeline bypasses the project's own crates.io quarantine. A freshly-published malicious crate can land in the build within minutes of publication via the `version-increment` job, and the same job force-pushes the change back onto the PR branch using ACTIONS_PUSH (a PAT with contents:write).",
      "confidence": "high",
      "confidenceLift": "End-to-end reproducer publishing a malicious crates.io version and observing it being committed by `github-actions[bot]` inside the quarantine window.",
      "easeOfExploit": "hard",
      "easeOfExploitRationale": "Requires the attacker to publish a malicious version of a crate already in the dependency graph (e.g. via a compromised maintainer account), but no further action by a NEAT-AI-core maintainer — the CI bypass auto-installs and auto-commits.",
      "file": ".github/workflows/ci.yml",
      "lines": ["59-101", "104-112"],
      "attackerModel": "Attacker who can publish a malicious version of any direct or transitive crate in the `neat-core` dependency graph (e.g. via a compromised maintainer account or registry takeover).",
      "trigger": "Any pull request to `Develop` triggers the `version-increment` job, which runs `cargo upgrade --incompatible` followed by `cargo update` on lines 99–101 and force-pushes the bumped `Cargo.toml`/`Cargo.lock` to the PR branch with the `ACTIONS_PUSH` PAT on line 111. Neither call honours `bump-deps.sh --quarantine-hours`, so a crate published seconds ago is pulled in.",
      "whyItIsABug": "`ci.yml:99-101` runs `cargo upgrade --incompatible && cargo update` directly, instead of delegating to `./bump-deps.sh`, which is the script the project itself documents as enforcing `VIBE_BUMP_QUARANTINE_HOURS` (default 24h) against `crates.io`. The bypassed quarantine is the documented supply-chain control for this repo (`bump-deps.sh:9-19` and `AGENTS.md`). Because `version-increment` then commits the result with `ACTIONS_PUSH` (lines 104–112), the malicious crate is checked in by `github-actions[bot]` before the `security` job runs `cargo audit` — `cargo audit` only flags known advisories from RustSec, not freshly-published malicious versions.",
      "exploitSketch": "1. Attacker publishes `serde v1.0.999` (or any in-graph crate) containing a build.rs payload at T₀. 2. Within 24h of T₀ a maintainer (or the scheduled `upgrade-dependencies.yml` cron, see finding `b2c4ne02`) opens a PR. 3. `version-increment` runs `cargo upgrade --incompatible && cargo update`, pulls `1.0.999`, commits, and pushes back with `ACTIONS_PUSH`. 4. The subsequent `quality` and `security` jobs invoke `cargo clippy`, `cargo test`, `cargo build --release`, and `cargo audit` against the malicious crate. The build.rs runs on every job runner with whatever scopes those steps have access to.",
      "fixSuggestion": "Replace the `cargo upgrade --incompatible && cargo update` block (`ci.yml:99-101`) with a single `./bump-deps.sh` invocation so the documented 24h quarantine is enforced in CI by the same code path used by the local Vibe Coder hook. Equivalently, drop the implicit bump entirely from the `version-increment` job and rely on the existing scheduled `upgrade-dependencies.yml` (after it has been gated likewise — see finding `b2c4ne02`)."
    },
    {
      "id": "b2c4ne02",
      "class": "supply-chain:quarantine-misconfigured",
      "severity": "High",
      "severityRationale": "AV:N/AC:H/PR:N/UI:N/S:C/C:H/I:H/A:H — the scheduled weekly upgrade workflow bypasses the same quarantine and force-creates a PR that the auto-format / version-increment chain then pushes through.",
      "confidence": "high",
      "confidenceLift": "Sample of a real weekly PR landing a crate version published less than 24h before the cron fired.",
      "easeOfExploit": "hard",
      "easeOfExploitRationale": "Same attacker model as `a1c4ne01` — needs a malicious crates.io version of a graph crate — but the weekly cron makes it a guaranteed window with no human in the loop.",
      "file": ".github/workflows/upgrade-dependencies.yml",
      "lines": ["30-34", "67-79"],
      "attackerModel": "Attacker who can publish a malicious version of any in-graph crate.",
      "trigger": "Every Monday 06:00 UTC the `upgrade` job runs `cargo upgrade --dry-run`, then unconditional `cargo upgrade` and `cargo update` (lines 31–48), then `peter-evans/create-pull-request@v7` (line 69) opens a PR with the bumped lockfile.",
      "whyItIsABug": "The scheduled cron uses `cargo upgrade && cargo update` directly. It does not invoke `bump-deps.sh`, so the `VIBE_BUMP_QUARANTINE_HOURS` window is never consulted. The compensating control in `bump-deps.sh:171-216` filters dry-run output and defers any bump newer than the quarantine; the workflow simply ignores it.",
      "exploitSketch": "1. Attacker publishes a malicious version of any graph crate any time during the week. 2. Cron fires Monday 06:00 UTC. 3. Workflow upgrades to the malicious version and opens a PR. 4. When a maintainer or the `version-increment` job auto-merges the PR, the malicious build.rs runs on every CI runner.",
      "fixSuggestion": "Replace the `cargo upgrade` / `cargo update` calls (lines 31–48) with `./bump-deps.sh --quarantine-hours \"${VIBE_BUMP_QUARANTINE_HOURS:-24}\"`, and only proceed to `create-pull-request` if `bump-deps.sh` exited 0 with at least one applied bump (the script prints `bump-deps: external=...; audit=...; build=...`)."
    },
    {
      "id": "c3d5ne03",
      "class": "supply-chain",
      "severity": "Medium",
      "severityRationale": "AV:N/AC:H/PR:N/UI:N/S:C/C:H/I:H/A:H — third-party GitHub Actions are pinned to mutable tags rather than 40-char commit SHAs, so a tag move (e.g. supply-chain compromise of a tag) silently re-points the workflow.",
      "confidence": "high",
      "confidenceLift": "Demonstrate a force-push to an upstream action's `v2` tag and observe the next CI run consuming the new SHA.",
      "easeOfExploit": "moderate",
      "easeOfExploitRationale": "Requires compromise of (or social-engineering against) a third-party action maintainer to retag, which has been the entry point for prior real-world incidents (`tj-actions/changed-files` 2025).",
      "file": ".github/workflows/ci.yml",
      "lines": ["33-37", "52-57", "125-129", "144-147", "176-180", "196-202", "205-208", "267-272", "289-295"],
      "attackerModel": "Attacker who can move a tag on a third-party action repository (e.g. compromised maintainer account, repository takeover).",
      "trigger": "Any push or PR triggers CI, which pulls the latest commit pointed to by `@v2` / `@v4.2.0` / `@stable` rather than a pinned 40-char SHA.",
      "whyItIsABug": "Per the worker's coding guideline (`Dependency Bumps and Supply Chain` — `Pin GitHub Actions to commit SHAs, not version tags`) every external action should be pinned to a 40-char SHA. The following lines violate this: `ci.yml:33` (`actions/checkout@v4.2.0`), `ci.yml:52` and `ci.yml:144` and `ci.yml:196` and `ci.yml:295` (`dtolnay/rust-toolchain@stable` — pinned to a *branch*, the worst case), `ci.yml:55` and `ci.yml:206` (`taiki-e/install-action@v2`), `ci.yml:125` (`actions/checkout@v4.2.0`), `ci.yml:201` (`Swatinem/rust-cache@v2`), `ci.yml:267` (`codespell-project/actions-codespell@v2`), `ci.yml:289` (`actions/checkout@v4.2.0`). The same pattern is in `security.yml:20,29,32,39,50`, `wasm-bundle.yml:21,24`, `upgrade-dependencies.yml:19,25,69`, `gitleaks.yml:14,17`, and `semgrep.yml:16`. `markdown-lint.yml:17,19,38` is the only workflow that gets this right.",
      "exploitSketch": "1. Attacker compromises `dtolnay/rust-toolchain` maintainer credentials or social-engineers a maintainer. 2. They push a backdoored commit and move the `stable` branch to point at it. 3. Next NEAT-AI-core CI run installs a Rust toolchain that exfiltrates `ACTIONS_PUSH` or injects code into the binary it builds.",
      "fixSuggestion": "Replace every `uses: <repo>@<tag>` line with `uses: <repo>@<40-char SHA> # <tag>` so the tag stays in a comment for human readability but the resolution is immutable. Apply to `ci.yml`, `security.yml`, `wasm-bundle.yml`, `upgrade-dependencies.yml`, `gitleaks.yml`, and `semgrep.yml`. `markdown-lint.yml` is the reference implementation."
    },
    {
      "id": "d4e6ne04",
      "class": "supply-chain",
      "severity": "Medium",
      "severityRationale": "AV:N/AC:H/PR:N/UI:N/S:C/C:H/I:H/A:H — `curl | sh` install of wasm-pack with no checksum / version pin. A compromise of `rustwasm.github.io` (or a MITM on the redirect chain) lets an attacker land arbitrary code in the build runner.",
      "confidence": "high",
      "confidenceLift": "Reproducer serving an alternative installer body from a controlled DNS view of `rustwasm.github.io`.",
      "easeOfExploit": "hard",
      "easeOfExploitRationale": "Requires compromise of the rustwasm GitHub Pages site, or a CDN/MITM attack against the runner egress.",
      "file": ".github/workflows/wasm-bundle.yml",
      "lines": ["28-29"],
      "attackerModel": "Attacker who can serve a malicious response from `rustwasm.github.io/wasm-pack/installer/init.sh` (project compromise, GitHub Pages takeover, runner egress MITM).",
      "trigger": "Every push to `Develop` runs the `publish` job, which executes `curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh` (line 29) with no checksum verification, no GPG signature, no version pin.",
      "whyItIsABug": "Per the worker's coding guideline (`Secure Coding Principles — Keep Dependencies Updated` / `Pin GitHub Actions to commit SHAs`) and historical experience, `curl | sh` is the classic supply-chain attack pattern. The installer body executes with the runner's identity and is then used to compile the artefact that is published as a per-commit GitHub Release — which NEAT-AI consumers download by SHA. A poisoned installer turns into a poisoned `wasm_activation_bg.wasm` for every NEAT-AI bump-deps run that month.",
      "exploitSketch": "1. Attacker replaces the contents of `init.sh` for ~5 minutes. 2. The next push to `Develop` runs the workflow and installs a backdoored wasm-pack. 3. The backdoored wasm-pack emits a backdoored `wasm_activation_bg.wasm`. 4. NEAT-AI's `bump-deps.sh`/`build.sh` downloads the per-commit release and stitches the backdoored bundle into the next NEAT-AI PR — the verifier in `verify-wasm-bundle.sh` only checks file presence and size, not content integrity.",
      "fixSuggestion": "Replace the installer pipe with a pinned release download + sha256 verification — e.g. download `wasm-pack-vX.Y.Z-x86_64-unknown-linux-musl.tar.gz` from the wasm-pack GitHub release page, sha256-check it against a value committed alongside the workflow, then `tar xzf` and `install -m 0755 wasm-pack /usr/local/bin/`. Equivalently, use `cargo install wasm-pack --locked --version X.Y.Z` if the runner already has `cargo`."
    },
    {
      "id": "e5f7ne05",
      "class": "memory-safety",
      "severity": "Medium",
      "severityRationale": "AV:L/AC:H/PR:L/UI:N/S:U/C:L/I:L/A:H — undefined behaviour in the SIMD weighted-sum fast path on caller-supplied inputs. Exploitability depends on whether downstream consumers (NEAT-AI-scorer, rust_scorer) ever pass mismatched slice lengths or out-of-range `from_index` values from untrusted training data.",
      "confidence": "high",
      "confidenceLift": "Run with `RUSTFLAGS=-Z sanitizer=address` and a fuzz harness over `weighted_sum_simd_8records` / `_4records`.",
      "easeOfExploit": "hard",
      "easeOfExploitRationale": "Requires a path from untrusted bytes to mismatched `synapses` / `act*` lengths or to a `synapse.from_index` larger than `act*.len()`. Production callers in this codebase appear to validate upstream, but the public wrapper carries the contract on its caller, so a future caller can trigger it.",
      "file": "neat-core/src/simd_native.rs",
      "lines": ["83-153", "165-241", "244-334"],
      "attackerModel": "Anyone who can call `neat-core::weighted_sum_simd_8records` / `weighted_sum_simd_4records` with attacker-influenced slice lengths or `synapse.from_index` values — e.g. malformed `.bin` training data parsed by `training_bin_stream.rs` and threaded through `network::SynapseData`.",
      "trigger": "Call `weighted_sum_simd_8records(&synapses, &act0, ..., &act7, 0, synapses.len(), 0.0)` where (a) one of `act0..act7` has fewer elements than `synapses.iter().map(|s| s.from_index).max()`, or (b) `end > synapses.len()`.",
      "whyItIsABug": "The public wrappers `weighted_sum_simd_8records` (`simd_native.rs:247-291`) and `weighted_sum_simd_4records` (`simd_native.rs:296-334`) take untrusted slices and forward them to the SIMD `unsafe fn`s on lines 83, 126, 165, and 214. Each `unsafe fn` uses `synapses.get_unchecked(i)` inside `for i in start..end` (e.g. line 99) and `act_k.get_unchecked(from)` (e.g. lines 103–110) without any bounds check. The `# Safety` comment on each `unsafe fn` (e.g. `simd_native.rs:79-80`) only mentions CPU-feature detection (`is_x86_feature_detected!`/`is_aarch64_feature_detected!`) — it omits the actual safety invariants needed by `get_unchecked` (`start..end ⊆ synapses` and `synapse.from_index < min(act_k.len())`). The public wrappers do not validate these invariants before entering the `unsafe` block (`simd_native.rs:269-273`, `280-284`, `314-318`, `325-329`), so a caller satisfying the documented contract can still trigger UB.",
      "exploitSketch": "1. Caller passes `synapses = [{from_index: 100, weight: 1.0}]` and `act0..act7` each with 16 elements. 2. `weighted_sum_simd_8records(.., 0, 1, 0.0)` enters the `is_x86_feature_detected!(\"avx2\")` branch and calls `x86::weighted_sum_simd_8records_avx2` (line 269). 3. Inside the `unsafe fn`, `*act0.get_unchecked(100)` is read past the end of `act0`'s buffer — UB. With ASan: out-of-bounds load detected; without ASan: returns whatever the heap allocator placed there, or segfaults if the page is unmapped.",
      "fixSuggestion": "Either (a) validate `end <= synapses.len()` and `max_from < min(act_k.len())` in the public wrappers before the `unsafe` block (return scalar result on failure), or (b) tighten the `# Safety` comments on each `unsafe fn` to spell out the index invariants AND add `debug_assert!` checks inside the `unsafe fn` so debug builds and fuzz harnesses surface a violation. Approach (a) is the smaller-blast-radius fix because it eliminates the UB class at the public boundary."
    },
    {
      "id": "f6g8ne06",
      "class": "supply-chain",
      "severity": "Low",
      "severityRationale": "AV:N/AC:H/PR:N/UI:N/S:U/C:L/I:L/A:L — `quality.sh` runs `cargo upgrade --incompatible && cargo update` on the developer's local box (line 61–62), so a developer-local invocation bypasses the same quarantine that `bump-deps.sh` enforces. Local-only blast radius, but the dev's `~/.cargo` cache then taints future runs.",
      "confidence": "high",
      "confidenceLift": "Reproducer running `./quality.sh` while a malicious crate version is fresh on crates.io and observing it land in `Cargo.lock` locally.",
      "easeOfExploit": "moderate",
      "easeOfExploitRationale": "Same attacker model as `a1c4ne01` — needs a malicious crates.io version of a graph crate.",
      "file": "quality.sh",
      "lines": ["58-66"],
      "attackerModel": "Attacker who can publish a malicious crate version, in combination with a developer running `./quality.sh` (the documented pre-commit local gate).",
      "trigger": "Developer runs `./quality.sh` (per `AGENTS.md` and the worker rules) — line 61–62 unconditionally runs `cargo upgrade --incompatible && cargo update`, with no quarantine.",
      "whyItIsABug": "`quality.sh:58-66` invokes `cargo upgrade --incompatible && cargo update` directly, instead of `./bump-deps.sh` which is the project's own quarantine-aware wrapper. The local gate is documented in `AGENTS.md` (`Run **./quality.sh** before commit/PR.`) and is therefore the most-executed path in the project.",
      "fixSuggestion": "Replace the `cargo upgrade --incompatible && cargo update` lines with `./bump-deps.sh` (or `./bump-deps.sh --skip-build` if the subsequent `cargo build` / `cargo test` steps are sufficient). The script already prints a one-line summary so the local UX is unchanged."
    }
  ]
}
```

## Executive summary

Scope was the full repository, a Rust shared library (`neat-core`) with no HTTP /
queue / database surface, plus the build and supply-chain tooling under
`.github/workflows/` and `bump-deps.sh` / `quality.sh` / `scripts/*.sh`. Six
findings survive triage. The headline risk is **supply-chain**: the project
ships a documented 24h quarantine for crates.io in `bump-deps.sh`, but the
per-PR CI and the weekly scheduled upgrade both bypass that quarantine by
calling `cargo upgrade` / `cargo update` directly. Two High-severity findings
(`a1c4ne01`, `b2c4ne02`) call this out. A Medium memory-safety finding
(`e5f7ne05`) records that the public SIMD wrappers forward unchecked indices
into `unsafe` `get_unchecked` paths. No injection, no secret leaks, no
auth/IDOR surface (this is a library, not a service).

## Findings

### `a1c4ne01` — `supply-chain:quarantine-misconfigured` — **High**

- **File:** `.github/workflows/ci.yml`, lines 59–101 and 104–112.
- **CVSS rationale:** AV:N/AC:H/PR:N/UI:N/S:C/C:H/I:H/A:H — CI bypasses the
  documented crates.io quarantine; malicious crate lands on the PR branch with
  `ACTIONS_PUSH` (contents:write) before `cargo audit` runs.
- **Confidence:** high. Lift requires a reproducer publishing a malicious
  crates.io version and observing it being committed by `github-actions[bot]`
  inside the quarantine window.
- **Ease of exploit:** hard — attacker must publish a malicious crate version,
  but the CI bypass is automatic after that.
- **Attacker model:** any attacker who can publish a malicious in-graph crate.
- **Trigger:** any PR to `Develop`.
- **Why it is a bug:** `ci.yml:99-101` runs `cargo upgrade --incompatible && cargo update`
  directly; the project's documented control is `./bump-deps.sh` with
  `VIBE_BUMP_QUARANTINE_HOURS=24`. `cargo audit` only flags known RustSec
  advisories, not freshly-published malicious versions.
- **Exploit sketch:** publish `serde v1.0.999` with a `build.rs` payload →
  next PR's `version-increment` job pulls it within seconds and pushes it
  back to the PR branch → `quality`/`security` jobs build and run it on the
  runner.
- **Fix:** replace the `cargo upgrade --incompatible && cargo update` block
  with a single `./bump-deps.sh` invocation.

### `b2c4ne02` — `supply-chain:quarantine-misconfigured` — **High**

- **File:** `.github/workflows/upgrade-dependencies.yml`, lines 30–34 and 67–79.
- **CVSS rationale:** AV:N/AC:H/PR:N/UI:N/S:C/C:H/I:H/A:H — weekly cron with
  no human in the loop.
- **Confidence:** high. Lift requires a sample real weekly PR landing a crate
  version published less than 24h before the cron fired.
- **Ease of exploit:** hard — same as `a1c4ne01`, plus needs the malicious
  version to exist when the cron fires.
- **Attacker model:** same as `a1c4ne01`.
- **Trigger:** Monday 06:00 UTC cron.
- **Why it is a bug:** `upgrade-dependencies.yml:30-34` and `46-48` run
  `cargo upgrade` / `cargo update` directly, then `peter-evans/create-pull-request@v7`
  opens a PR carrying whatever was bumped — quarantine not consulted.
- **Exploit sketch:** publish malicious version during the week → cron fires →
  workflow opens a PR with the malicious bump → merging that PR runs the
  payload on every CI runner.
- **Fix:** delegate to `./bump-deps.sh --quarantine-hours "${VIBE_BUMP_QUARANTINE_HOURS:-24}"`
  before the `Check for changes` / `Create pull request` steps; only proceed
  if `bump-deps.sh` exited 0 with at least one applied bump.

### `c3d5ne03` — `supply-chain` (action tag pinning) — **Medium**

- **File:** `.github/workflows/ci.yml`, lines 33, 52, 55, 125, 144, 176, 196,
  201, 206, 267, 289, 295. Same pattern in `security.yml:20,29,32,39,50`,
  `wasm-bundle.yml:21,24`, `upgrade-dependencies.yml:19,25,69`,
  `gitleaks.yml:14,17`, `semgrep.yml:16`.
- **CVSS rationale:** AV:N/AC:H/PR:N/UI:N/S:C/C:H/I:H/A:H.
- **Confidence:** high. Lift requires demonstrating a forced tag move on an
  upstream action and observing the next CI run pick up the new SHA.
- **Ease of exploit:** moderate — requires compromise / social engineering of
  an action maintainer. This was the exact pattern of the March 2025
  `tj-actions/changed-files` incident.
- **Attacker model:** attacker who can move a tag on a third-party action repo.
- **Trigger:** any push or PR.
- **Why it is a bug:** the worker coding guidelines say *"Pin GitHub Actions
  to commit SHAs, not version tags."* `markdown-lint.yml` already follows that
  pattern (lines 17, 19, 38). The other six workflows do not. `dtolnay/rust-toolchain@stable`
  pinned to a *branch* is the worst case because any push to `stable` becomes
  immediately effective.
- **Exploit sketch:** compromise `dtolnay/rust-toolchain` → push backdoored
  commit → move `stable` → next NEAT-AI-core CI run installs the backdoored
  toolchain on a runner with `ACTIONS_PUSH` available.
- **Fix:** rewrite each `uses: <repo>@<tag>` to `uses: <repo>@<40-char SHA> # <tag>`,
  using `markdown-lint.yml` as the reference. Adopt the existing pattern
  rather than inventing a new one.

### `d4e6ne04` — `supply-chain` (`curl | sh` installer) — **Medium**

- **File:** `.github/workflows/wasm-bundle.yml`, lines 28–29.
- **CVSS rationale:** AV:N/AC:H/PR:N/UI:N/S:C/C:H/I:H/A:H.
- **Confidence:** high. Lift requires a reproducer serving an alternative
  installer body.
- **Ease of exploit:** hard — needs rustwasm Pages compromise or runner egress
  MITM.
- **Attacker model:** attacker who can serve a malicious response from
  `rustwasm.github.io/wasm-pack/installer/init.sh`.
- **Trigger:** every push to `Develop`.
- **Why it is a bug:** `curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh`
  with no checksum, no signature, no version pin. The poisoned tool builds the
  per-commit `wasm_activation-pkg.tar.gz` that downstream NEAT-AI's
  `bump-deps.sh` / `build.sh` consumes; the existing `verify-wasm-bundle.sh`
  only checks file presence and a size threshold, not content integrity.
- **Exploit sketch:** replace `init.sh` body for ~5 minutes → next push to
  `Develop` installs backdoored wasm-pack → backdoored wasm is published to
  the per-commit Release → NEAT-AI consumes it on the next bump-deps run.
- **Fix:** pin to a specific wasm-pack release tarball downloaded from GitHub
  releases and sha256-verify it against a value committed alongside the
  workflow, or use `cargo install wasm-pack --locked --version <X.Y.Z>`.

### `e5f7ne05` — `memory-safety` (SIMD wrapper unsafe forwarding) — **Medium**

- **File:** `neat-core/src/simd_native.rs`, lines 83–153 (`x86::*`), 165–241
  (`aarch64::*`), 244–334 (public wrappers).
- **CVSS rationale:** AV:L/AC:H/PR:L/UI:N/S:U/C:L/I:L/A:H — UB triggered by
  caller-supplied inputs.
- **Confidence:** high. Lift requires `RUSTFLAGS=-Z sanitizer=address` + a
  fuzz harness.
- **Ease of exploit:** hard — needs a path from untrusted bytes to a mismatch.
- **Attacker model:** any caller of `weighted_sum_simd_8records` /
  `weighted_sum_simd_4records` with attacker-influenced slice lengths or
  `synapse.from_index`. Trace candidates: `training_bin_stream.rs` parsing
  `.bin` files → `network::SynapseData`.
- **Trigger:** mismatched lengths or `from_index` outside `act_k.len()`.
- **Why it is a bug:** `synapses.get_unchecked(i)` and `act_k.get_unchecked(from)`
  inside `unsafe fn`s assume bounds invariants that the `# Safety` comments do
  not name, and that the public wrappers (`simd_native.rs:269-273`, `280-284`,
  `314-318`, `325-329`) do not validate before entering `unsafe`.
- **Exploit sketch:** caller passes `synapses = [{from_index: 100, ..}]` plus
  `act0..act7` each of length 16; the AVX2 path reads `*act0.get_unchecked(100)`
  past the buffer.
- **Fix:** either validate in the public wrappers (small blast radius,
  preferred) or tighten the `# Safety` comments AND add `debug_assert!`
  checks inside the `unsafe fn`s.

### `f6g8ne06` — `supply-chain` (local quality.sh bypass) — **Low**

- **File:** `quality.sh`, lines 58–66.
- **CVSS rationale:** AV:N/AC:H/PR:N/UI:N/S:U/C:L/I:L/A:L — local-only blast
  radius but documented as the pre-commit gate.
- **Confidence:** high. Lift requires reproducing a fresh malicious version
  landing in a developer's `Cargo.lock` after `./quality.sh`.
- **Ease of exploit:** moderate — needs a fresh malicious version on
  crates.io and a developer running `./quality.sh`.
- **Attacker model:** attacker publishing a malicious in-graph crate.
- **Trigger:** developer runs `./quality.sh`.
- **Why it is a bug:** `quality.sh:58-66` runs `cargo upgrade --incompatible && cargo update`
  directly, ignoring `bump-deps.sh`. The local gate is documented as the
  pre-commit check.
- **Exploit sketch:** poison a fresh crates.io version → developer runs
  `./quality.sh` to verify their PR → malicious build.rs runs locally.
- **Fix:** replace lines 61–62 with `./bump-deps.sh` (or
  `./bump-deps.sh --skip-build` if the subsequent `cargo build` covers the
  build step).

## Coverage map

| Chunk | Files read | Notes |
|-------|------------|-------|
| 1. CI / version-increment | `.github/workflows/ci.yml` | High-severity quarantine bypass and action-pinning findings. |
| 2. Scheduled upgrade cron | `.github/workflows/upgrade-dependencies.yml` | Second quarantine-bypass path. |
| 3. WASM bundle publish | `.github/workflows/wasm-bundle.yml`, `scripts/build-wasm-bundle.sh`, `scripts/verify-wasm-bundle.sh` | `curl \| sh` installer finding; verifier limited to size + presence. |
| 4. Security / lint workflows | `.github/workflows/security.yml`, `gitleaks.yml`, `semgrep.yml`, `markdown-lint.yml` | All except `markdown-lint.yml` are tag-pinned — folded into `c3d5ne03`. |
| 5. Local quality / bump-deps | `quality.sh`, `bump-deps.sh`, `deny.toml`, `.gitleaks.toml` | `bump-deps.sh` is the documented quarantine control; `quality.sh` bypasses it locally. |
| 6. Library safety surface (`neat-core/`) | `lib.rs`, `wasm_exports.rs`, `simd_native.rs`, `simd.rs` (scanned), `propagate_codec.rs`, `training_bin_stream.rs`, `Cargo.toml`, root `Cargo.toml` | One SIMD memory-safety finding. Propagate codec has proper up-front size validation. `unsafe { std::env::set_var(...) }` calls in `training_bin_stream.rs` are all inside `#[cfg(test)]` and required by `edition = "2024"` — not findings. |

Skipped: `target/`, `.git/`, `Cargo.lock` (audited transitively via `cargo audit`
results expected by `security.yml`). No tests were executed (read-only audit).

## Suggested next scans

- **Fuzz `neat-core` decoders.** `propagate_codec::decode_propagate_buffer` and
  the `training_bin_stream.rs` binary record parser are obvious targets. A
  one-day cargo-fuzz harness over each is the highest-value follow-up.
- **ASan run of `simd_native` against representative `network::SynapseData`
  inputs.** Confirms whether finding `e5f7ne05` is exploitable in practice
  via the production call sites (NEAT-AI-scorer, `rust_scorer`).
- **Replay the supply-chain quarantine test from a real malicious-crate fixture.**
  The bats harness in `tests/scripts/` exercises `bump-deps.sh --check-published`;
  extend it to drive `ci.yml` and `upgrade-dependencies.yml` via `act`.
- **Audit the `ACTIONS_PUSH` PAT scope.** A `contents:write` PAT that can push
  back to the PR branch in `version-increment` is the lever that turns a
  quarantine bypass into a self-applying compromise. If the scope can be
  tightened (e.g. to a specific branch protection class), do so.
