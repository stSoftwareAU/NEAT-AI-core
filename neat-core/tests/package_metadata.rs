//! Issue #113 — verify the crate exposes complete C-METADATA metadata.
//!
//! Cargo populates `CARGO_PKG_*` environment variables from the resolved
//! manifest (including workspace inheritance) at compile time. Asserting on
//! them checks the observable outcome — what a registry, `cargo doc`, or a
//! supply-chain provenance tool would read — rather than how the manifest is
//! written.

#[test]
fn repository_metadata_points_at_upstream_source() {
    assert_eq!(
        env!("CARGO_PKG_REPOSITORY"),
        "https://github.com/stSoftwareAU/NEAT-AI-core",
    );
}

#[test]
fn core_metadata_set_is_complete() {
    // C-METADATA: description, license and repository must all be present.
    assert!(!env!("CARGO_PKG_DESCRIPTION").is_empty());
    assert_eq!(env!("CARGO_PKG_LICENSE"), "Apache-2.0");
    assert!(!env!("CARGO_PKG_REPOSITORY").is_empty());
}
