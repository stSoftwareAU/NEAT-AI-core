//! One home for the wasm SIMD intrinsic path (Issue #541).
//!
//! `core::arch::wasm32` exists only when `target_arch = "wasm32"`; a
//! `wasm64-unknown-unknown` build reaches the *same* SIMD128 intrinsics through
//! `core::arch::wasm64`. Both wasm kernels ([`crate::simd`],
//! [`crate::elastic_distribution`]) import from here so the arch split is
//! written once — a second copy is how a wasm64 build comes to compile on one
//! kernel and fail on the other.
//!
//! The module is gated on `target_family = "wasm"`, so native builds never see
//! it.

#![cfg(target_family = "wasm")]

#[cfg(target_arch = "wasm32")]
pub use core::arch::wasm32::*;

// `core::arch::wasm64` is unstable (`simd_wasm64`, rust-lang/rust#90599); the
// crate root enables the feature for `target_arch = "wasm64"` only, which is
// already a nightly-plus-`-Z build-std` target.
#[cfg(target_arch = "wasm64")]
pub use core::arch::wasm64::*;
