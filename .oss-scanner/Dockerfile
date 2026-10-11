# OSS Scanner build image (https://github.com/anthropics/oss-scanner).
# The scanner builds this with the repository as the build context, then runs
# its audit offline, so every crate, toolchain component and tool the build and
# its tests need is fetched here. Rust matches the workspace `rust-version`
# (1.99, the NEAT-AI family floor); Deno matches the CI pin.
FROM rust:1.99.0-bookworm

ENV DEBIAN_FRONTEND=noninteractive \
    CARGO_TERM_COLOR=never \
    # Full DWARF for the scanner; the workspace dev profile keeps line tables only.
    CARGO_PROFILE_DEV_DEBUG=full \
    RUSTFLAGS="-C force-frame-pointers=yes"
RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates curl git jq python3 \
    && rm -rf /var/lib/apt/lists/*

# Deno runs the repo-owned TypeScript gates under tests/ and scripts/.
COPY --from=denoland/deno:bin-2.9.3 /deno /usr/local/bin/deno

# The wasm32 lane: the target plus the wasm-pack and wasm-bindgen CLIs, pinned
# by version and SHA-256 exactly as .github/workflows/wasm-bundle.yml pins them.
ARG WASM_PACK_VERSION=0.15.0
ARG WASM_PACK_SHA256=c09f971ecaed9a2efc80fdcea7a00ef6b53c7fadc8c57d1f61b53a6aa66b668a
ARG WASM_BINDGEN_VERSION=0.2.128
ARG WASM_BINDGEN_SHA256=b51f0208fdff83515a787bd8ab9ac5865ed84dabb66d0c709957bb59793c645f
RUN rustup target add wasm32-unknown-unknown \
    && cd /tmp \
    && asset="wasm-pack-v${WASM_PACK_VERSION}-x86_64-unknown-linux-musl" \
    && curl --proto '=https' --tlsv1.2 -fsSL -o "$asset.tar.gz" \
         "https://github.com/rustwasm/wasm-pack/releases/download/v${WASM_PACK_VERSION}/$asset.tar.gz" \
    && echo "${WASM_PACK_SHA256}  $asset.tar.gz" | sha256sum -c - \
    && tar -xzf "$asset.tar.gz" && install -m 0755 "$asset/wasm-pack" /usr/local/bin/ \
    && asset="wasm-bindgen-${WASM_BINDGEN_VERSION}-x86_64-unknown-linux-musl" \
    && curl --proto '=https' --tlsv1.2 -fsSL -o "$asset.tar.gz" \
         "https://github.com/wasm-bindgen/wasm-bindgen/releases/download/${WASM_BINDGEN_VERSION}/$asset.tar.gz" \
    && echo "${WASM_BINDGEN_SHA256}  $asset.tar.gz" | sha256sum -c - \
    && tar -xzf "$asset.tar.gz" && install -m 0755 "$asset/wasm-bindgen" /usr/local/bin/ \
    && rm -rf /tmp/wasm-*

# scanner contract: the checkout lives inside the image, at /src
COPY . /src
WORKDIR /src

# Fetch every crate for every target against the committed lockfile (the
# wasm-bench harness is outside the workspace and has its own), then build the
# library, every test target and the wasm32 crate with debug info, so the
# offline audit can `cargo test` without the network. A failure here fails the
# build.
RUN cargo fetch --locked \
    && cargo fetch --locked --manifest-path wasm-bench/Cargo.toml \
    && cargo build --workspace --all-features --locked \
    && cargo test --workspace --all-features --locked --no-run \
    && cargo build -p neat-core --target wasm32-unknown-unknown --locked \
    && deno install --frozen

# Build the wasm32 bundle the JS side loads (neat-core/wasm_activation/pkg);
# wasm-pack may fetch wasm-opt here while the network is still up. Optional, so
# a failure warns rather than aborting the build.
RUN (./scripts/build-wasm-bundle.sh --arch wasm32 --rev oss-scanner \
      --out /tmp/wasm_activation-pkg.tar.gz \
      || echo "WARNING: wasm32 bundle build failed")

# Smoke-run the cheap tests; a failure warns rather than aborting the build.
# The full suite is `cargo test --workspace --all-features` from /src.
RUN (cargo test --workspace --all-features --locked --lib -q -- --test-threads=2 \
      || echo "WARNING: cargo lib tests reported failures") \
    && (cargo test --locked -p neat-core -q --test simd_public_bounds \
          --test network_header_input_count --test propagate_codec_header_bounds \
          --test creature_validate_json_boundary \
      || echo "WARNING: boundary tests reported failures")
