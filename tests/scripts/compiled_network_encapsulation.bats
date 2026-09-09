#!/usr/bin/env bats
# CompiledNetwork field encapsulation (Issue #625).
#
# `CompiledNetwork`'s forward and batched-scoring paths call the
# `simd::*_unchecked` kernels and discharge their index contract from the
# load-time `NetworkError::InvalidSynapseIndex` check in `new`. That discharge
# only holds while nothing outside the crate can rewrite the validated values,
# so the fields carrying them are private and reachable only through read-only
# accessors.
#
# These are "what" tests, not source greps: each builds a throwaway crate that
# depends on the live `neat-core` by path and asks **cargo** what happens. The
# refusal probe must be rejected by the compiler naming the private fields; the
# companion probe compiles *and runs* the same fixture through the accessors,
# so a refusal can never come from a broken fixture or a broken harness
# (AGENTS.md oracle rule 5).

setup() {
  REPO_ROOT="$(cd "${BATS_TEST_DIRNAME}/../.." && pwd)"
  export REPO_ROOT
  # The probe resolves its own lockfile, so it gets its own target directory
  # (as `rust_build_profiles.bats` does). Sharing the workspace's would let the
  # two resolutions clobber each other's artefacts.
}

# Emit a probe crate in $1 whose `main` body is $2.
probe_crate() {
  local dir="$1" body="$2"
  mkdir -p "${dir}/src"
  cat >"${dir}/Cargo.toml" <<EOF
[workspace]

[package]
name = "encapsulation-probe"
version = "0.0.0"
edition = "2024"

[dependencies]
neat-core = { path = "${REPO_ROOT}/neat-core" }
EOF
  cat >"${dir}/src/main.rs" <<EOF
use neat_core::network::CompiledNetwork;

/// 1 input, 1 identity output, weight 1.0, bias 0.5 — the smallest network
/// that loads and activates, so \`new\` returns \`Ok\` and the probe's only
/// variable is what it does with the loaded value.
fn minimal_network_bytes() -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(&2u32.to_le_bytes()); // num_neurons
    data.extend_from_slice(&1u32.to_le_bytes()); // num_inputs
    data.extend_from_slice(&0.5_f64.to_le_bytes()); // bias
    data.push(0); // squash_type IDENTITY
    data.push(0); // is_constant
    data.extend_from_slice(&1u16.to_le_bytes()); // num_synapses
    data.extend_from_slice(&0u16.to_le_bytes()); // from_index
    data.push(0); // synapse_type
    data.push(0); // padding
    data.extend_from_slice(&1.0_f64.to_le_bytes()); // weight
    data
}

fn main() {
${body}
}
EOF
}

@test "safe code outside the crate cannot rewrite a validated network's synapse indices" {
  local dir="${BATS_TEST_TMPDIR}/refused"
  probe_crate "$dir" '    let mut net = CompiledNetwork::new(&minimal_network_bytes()).expect("valid");
    net.synapses[0].from_index = 60_000;
    net.hot_from[0] = 60_000;
    let _ = net.activate(&[1.0], 1);'

  run env CARGO_TARGET_DIR="${dir}/target" cargo check --manifest-path "${dir}/Cargo.toml" --offline
  [ "$status" -ne 0 ] || {
    echo "the out-of-bounds mutation compiled — the SIMD index invariant is not closed by construction"
    echo "$output"
    false
  }
  # Both writes named in Issue #625 must be refused, and refused *because the
  # field is private* — not because the probe failed to build for some other
  # reason.
  echo "$output" | grep -q 'field `synapses` of struct `CompiledNetwork` is private'
  echo "$output" | grep -q 'field `hot_from` of struct `CompiledNetwork` is private'
}

@test "the same probe compiles and activates when it reads through the accessors" {
  local dir="${BATS_TEST_TMPDIR}/allowed"
  probe_crate "$dir" '    let mut net = CompiledNetwork::new(&minimal_network_bytes()).expect("valid");
    assert_eq!(net.synapses()[0].from_index, 0);
    assert_eq!(net.hot_from()[0], 0);
    assert_eq!(net.activations().len(), net.num_neurons());
    // identity(2.0 * 1.0 + 0.5)
    let out = net.activate(&[2.0], 1);
    assert!((out[0] - 2.5).abs() < 1e-5, "{out:?}");'

  run env CARGO_TARGET_DIR="${dir}/target" cargo run --quiet --manifest-path "${dir}/Cargo.toml" --offline
  [ "$status" -eq 0 ] || {
    echo "$output"
    false
  }
}
