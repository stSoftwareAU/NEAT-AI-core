//! Shared helpers for integration tests (`tests/*.rs` and `tests/*/main.rs`).
//!
//! Layout matches [NEAT-AI-Discovery](https://github.com/stSoftwareAU/NEAT-AI-Discovery):
//! `tests/common/mod.rs` plus `#[path = "../common/mod.rs"] mod common;` in subdirectory harnesses.

use std::path::PathBuf;

/// `neat-core` manifest directory (fixtures live under `tests/data/` when added).
#[allow(dead_code)]
pub fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Minimal valid creature JSON for compile / activation smoke tests.
pub fn minimal_creature_json() -> &'static str {
    r#"{
        "input": 2,
        "output": 1,
        "neurons": [
            {"type": "hidden", "uuid": "hidden-1", "bias": 0.0, "squash": "TANH"},
            {"type": "output", "uuid": "output-0", "bias": 0.0, "squash": "IDENTITY"}
        ],
        "synapses": [
            {"fromUUID": "input-0", "toUUID": "hidden-1", "weight": 1.0},
            {"fromUUID": "input-1", "toUUID": "hidden-1", "weight": 0.5},
            {"fromUUID": "hidden-1", "toUUID": "output-0", "weight": 1.0}
        ],
        "forwardOnly": true
    }"#
}
