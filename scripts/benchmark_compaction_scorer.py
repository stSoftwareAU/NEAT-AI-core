#!/usr/bin/env python3
"""Run the frozen native scorer on the public legacy benchmark artifacts."""
import hashlib
from datetime import datetime, timezone
import json
import math
import os
from pathlib import Path
import platform
import subprocess
import struct
import sys
import time
import tomllib

SCORER_REVISION = "a164969ac4d96ff39eace7086ad22c0538d95705"
LEGACY_REVISION = "60d6b4e5d3af20afbaaeb3420d588ad673ab8dab"
CORE_REVISION = "3759355634b1cabc8034cd76df5dbd44a867d18c"


def command(args, cwd, **kwargs):
    return subprocess.check_output(args, cwd=cwd, text=True, **kwargs)


def sha(data):
    return hashlib.sha256(data).hexdigest()


def main():
    if len(sys.argv) not in (4, 5):
        raise SystemExit("Usage: benchmark_compaction_scorer.py <frozen scorer checkout> <artifacts> <report.json> [repetitions >= 3]")
    checkout, artifacts, destination = map(Path, sys.argv[1:4])
    checkout, artifacts = checkout.resolve(), artifacts.resolve()
    repetitions = int(sys.argv[4]) if len(sys.argv) == 5 else 3
    if repetitions < 3:
        raise ValueError("At least three measured repetitions are required")
    if command(["git", "rev-parse", "HEAD"], checkout).strip() != SCORER_REVISION:
        raise ValueError("Scorer checkout is not the frozen revision")
    if command(["git", "status", "--porcelain", "--untracked-files=no"], checkout):
        raise ValueError("Scorer checkout has tracked modifications")
    lock_bytes = (checkout / "Cargo.lock").read_bytes()
    lock = tomllib.loads(lock_bytes.decode())
    core = next(p for p in lock["package"] if p["name"] == "neat-core")
    if core.get("source") != "git+https://github.com/stSoftwareAU/NEAT-AI-core?tag=v0.23.7#" + CORE_REVISION:
        raise ValueError("Unexpected native scorer core pin")
    reference = json.loads((artifacts / "reference.json").read_text())
    if reference["revision"] != LEGACY_REVISION or reference["seed"] != 1910:
        raise ValueError("Unexpected legacy artifact source")
    if (reference["input"], reference["output"], reference["recordCount"]) != (8, 4, 32):
        raise ValueError("Unexpected public corpus shape")
    corpus = (artifacts / "data/records.bin").read_bytes()
    if len(corpus) != 32 * (8 + 4) * 4:
        raise ValueError("Corpus is not 32 complete f32 records")
    published = json.loads((Path(__file__).resolve().parent.parent /
                            "docs/research/compaction-synthetic-performance.json").read_text())
    expected_corpus = b"".join(struct.pack("<f", value)
                               for inputs, targets in zip(published["probes"], published["targets"])
                               for value in inputs + targets)
    if corpus != expected_corpus:
        raise ValueError("Corpus differs from the published input/target rows")
    graphs = reference["graphs"]
    if [g["name"] for g in graphs] != ["original", "safe", "aggressive"]:
        raise ValueError("Expected original, safe and aggressive graphs")
    for graph, original in zip(graphs, published["rows"][0]["graphs"]):
        wire = (artifacts / "creatures" / (graph["name"] + ".json")).read_bytes()
        if (sha(wire) != graph["sha256"] or graph["sha256"] != original["sha256"]
                or graph["mse"] != original["mse"] or graph["growthCost"] != 1e-7):
            raise ValueError("Graph hash or growth cost mismatch")
    # Build the actual CLI with its lockfile; never accept an unrelated binary.
    build = ["cargo", "build", "--release", "--locked", "--bin", "rust_scorer"]
    subprocess.run(build, cwd=checkout, check=True, stdout=sys.stderr)
    if (checkout / "Cargo.lock").read_bytes() != lock_bytes:
        raise ValueError("Build changed the lockfile")
    metadata = json.loads(command(["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"], checkout))
    binary = Path(metadata["target_directory"]) / "release/rust_scorer"
    compiler = command(["rustc", "-Vv"], checkout).strip()
    # Use scorer defaults, apart from the explicit CPU-only selection. Ambient
    # emergency overrides must not silently change the recorded experiment.
    env = {k: v for k, v in os.environ.items() if not k.startswith("NEAT_SCORER_") and k != "RAYON_NUM_THREADS"}
    rows = []
    errors = {}
    for iteration in range(repetitions + 1):
        for graph in graphs:
            name = graph["name"]
            args = [str(binary), "--gpu", "off", "--cost", "MSE", str(artifacts / "creatures" / (name + ".json")), str(artifacts / "data")]
            start = time.perf_counter()
            result = json.loads(command(args, checkout, env=env))
            elapsed = (time.perf_counter() - start) * 1000
            if result["recordCount"] != 32 or result["forwardOnly"] != graph["forwardOnly"]:
                raise ValueError("Scorer changed record count or graph mode")
            if result["gpuBackend"] != "cpu-fallback" or result["costName"] != "MSE":
                raise ValueError("Expected explicit CPU-only scoring")
            if not all(math.isfinite(result[k]) for k in ("error", "score")):
                raise ValueError("Non-finite native result")
            values = (result["error"], result["score"])
            if iteration == 0:
                errors[name] = values
            elif errors[name] != values:
                raise ValueError("Native results changed between repetitions")
            rows.append({"iteration": iteration, "warmup": iteration == 0, "name": name,
                         "processMs": elapsed, "result": result,
                         "errorDeltaFromTs": result["error"] - graph["mse"],
                         "scoreDeltaFromTs": result["score"] - graph["score"]})
        print(f"Completed native {'warmup' if iteration == 0 else f'sample {iteration}/{repetitions}'}", file=sys.stderr)
    report = {"schema": 1, "measuredAt": datetime.now(timezone.utc).isoformat(),
              "scorerRevision": SCORER_REVISION,
              "nativeCore": core, "cargoLockSha256": sha(lock_bytes),
              "binarySha256": sha(binary.read_bytes()), "compiler": compiler,
              "buildCommand": build, "cargoConfig": (checkout / ".cargo/config.toml").read_text(),
              "buildRustflagsEnvironment": {k: os.environ.get(k) for k in ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS")},
              "host": {"os": platform.system(), "arch": platform.machine()},
              "corpusSha256": sha(corpus), "corpusBytes": len(corpus),
              "reference": reference, "repetitions": repetitions, "warmups": 1,
              "mode": "single creature CLI, --gpu off --cost MSE, no scorer environment overrides",
              "timingCaveat": "Fresh process wall time includes startup/load/I/O/scoring; not comparable to TS activation-only timing.",
              "comparisonCaveat": "Observed differences only; no tolerance or consumer acceptance gate is introduced.",
              "rows": rows}
    destination.write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
