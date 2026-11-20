//! Issue #767: rewrites one creature file into the persisted presentation
//! contract (`docs/CREATURE_JSON_PRESENTATION.md`) so the churn table in that
//! document reproduces from the tree.
//!
//! Usage: `cargo run -q -p neat-core --example creature_presentation_rewrite -- <creature.json>`
//! The result goes to stdout with no trailing newline, as the contract requires.

use neat_core::creature_presentation_json;
use serde_json::Value;

fn main() {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: creature_presentation_rewrite <creature.json>");
        std::process::exit(2);
    };
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        eprintln!("{path}: cannot read file: {e}");
        std::process::exit(1);
    });
    let value: Value = serde_json::from_str(&text).unwrap_or_else(|e| {
        eprintln!("{path}: invalid JSON: {e}");
        std::process::exit(1);
    });
    match creature_presentation_json(&value) {
        Ok(out) => print!("{out}"),
        Err(e) => {
            eprintln!("{path}: presentation error: {e}");
            std::process::exit(1);
        }
    }
}
