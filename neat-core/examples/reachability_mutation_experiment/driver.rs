//! Manifest-bound execution and failure-preserving result output.

use super::*;
use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

fn command(program: &str, args: &[&str]) -> Res<Vec<u8>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().ok_or("missing workspace")?;
    let output = Command::new(program).args(args).current_dir(root).output()?;
    if !output.status.success() {
        return Err(format!("{program} {args:?}: {}", String::from_utf8_lossy(&output.stderr)).into());
    }
    Ok(output.stdout)
}

fn git(args: &[&str]) -> Res<String> {
    Ok(String::from_utf8(command("git", args)?)?.trim().to_string())
}

fn source_matches(expected: &[u8], embedded: &[u8], path: &str) -> Res<()> {
    if expected != embedded {
        return Err(format!("stale executable source for {path}; rebuild with cargo run").into());
    }
    Ok(())
}

fn provenance() -> Res<Value> {
    command("git", &["diff", "--exit-code", "--quiet", "HEAD"])?;
    for (path, embedded) in [
        ("neat-core/examples/reachability_mutation_experiment.rs", include_bytes!("../reachability_mutation_experiment.rs").as_slice()),
        ("neat-core/examples/reachability_mutation_experiment/driver.rs", include_bytes!("driver.rs").as_slice()),
    ] {
        let committed = command("git", &["show", &format!("HEAD:{path}")])?;
        source_matches(&committed, embedded, path)?;
    }
    let executable = std::env::current_exe()?;
    Ok(json!({
        "commit": git(&["rev-parse", "HEAD"])? ,
        "tree": git(&["rev-parse", "HEAD^{tree}"])? ,
        "cargo_lock_blob": git(&["rev-parse", "HEAD:Cargo.lock"])? ,
        "executable_blob": git(&["hash-object", executable.to_str().ok_or("non-UTF8 executable path")?])?,
        "compiler_available_at_execution": String::from_utf8(command("rustc", &["-Vv"])?)?,
        "arch": std::env::consts::ARCH, "os": std::env::consts::OS,
    }))
}

fn manifest() -> Res<Value> {
    Ok(json!({
        "schema_version": 2,
        "protocol": "docs/research/reachability-mutation-protocol-v2.md",
        "source": provenance()?,
        "crate_version": env!("CARGO_PKG_VERSION"),
        "paper": { "url": PAPER, "version": "v1", "verified": "2026-11-14",
            "title": "When Sparsity Breaks Connectivity: Functional Reachability and Guided Topology Dynamics in Extreme Sparse Training",
            "relation": "Independent bridge-only experiment; not a replication of guided DST" },
        "workloads": WORKLOADS.map(|w| json!({"name": w.name(), "hidden": if w == Workload::Legacy { TEACHER_HIDDEN } else { w.hidden() }})),
        "seeds": [0, DEFAULT_SEEDS], "paired_trials": DEFAULT_SEEDS * WORKLOADS.len() as u64,
        "num_inputs": NUM_INPUTS, "num_outputs": NUM_OUTPUTS,
        "calibration_records": CAL_RECORDS, "untouched_validation_records": VAL_RECORDS,
        "offspring_evaluation_limit": EVAL_BUDGET, "proposal_limit": MAX_PROPOSALS,
        "bootstrap": { "resamples": BOOTSTRAP_RESAMPLES, "seed": BOOTSTRAP_SEED,
            "percentile_indices": [75, BOOTSTRAP_RESAMPLES - 76], "confidence": 0.9875 },
        "random_streams": { "teacher": STREAM_TEACHER, "data": STREAM_DATA,
            "student": STREAM_STUDENT, "weights": STREAM_WEIGHTS, "selection": STREAM_SELECT },
        "arm_order": "unbiased then bridge on even seeds; reversed on odd seeds",
        "acceptance": "topology validation + cleanup/full validation + finite strictly lower calibration MSE",
        "decision": "family: validation interval below zero, invalid difference <= 0, bridge subset >= 20% of proposals; adoption requires all extended families; any incomplete pair withholds verdict",
        "costs": "shared teacher/data/cleanup preparation charged half to each arm; separate calibration/validation scoring including compilation; total process excludes cargo build",
    }))
}

fn check_manifest(expected: &Value, actual: &Value) -> Res<()> {
    if expected != actual {
        return Err("manifest differs from executable protocol/provenance; declare and run the same committed build".into());
    }
    Ok(())
}

fn completion(expected: usize, completed: usize, failures: usize) -> Res<()> {
    if completed != expected || failures != 0 {
        return Err(format!("incomplete experiment: {completed}/{expected} paired trials, {failures} failures; see result file").into());
    }
    Ok(())
}

fn family_summary(pairs: &[(ArmResult, ArmResult)]) -> Res<Value> {
    let diff = |f: &dyn Fn(&ArmResult) -> f64| -> Vec<f64> {
        pairs.iter().map(|(u, b)| f(b) - f(u)).collect()
    };
    let val = summarise(&diff(&|r| r.final_val_mse), true)?;
    let invalid = summarise(&diff(&|r| r.invalid_offspring as f64), true)?;
    let proposals: usize = pairs.iter().map(|(_, b)| b.proposals).sum();
    let bridges: usize = pairs.iter().map(|(_, b)| b.bridge_selected).sum();
    let fraction = if proposals > 0 { bridges as f64 / proposals as f64 } else { 0.0 };
    let mut metrics = BTreeMap::new();
    for (name, lower, getter) in [
        ("improvement_cal", false, ArmResult::improvement_cal as fn(&ArmResult) -> f64),
        ("final_dead_structure", true, ArmResult::final_dead),
        ("total_seconds_indicative", true, ArmResult::total_seconds),
        ("final_bias_driven", true, (|r: &ArmResult| r.final_bias_driven as f64) as fn(&ArmResult) -> f64),
        ("improvement_per_eval", false, |r: &ArmResult| r.improvement_cal() / r.evaluations as f64),
    ] {
        metrics.insert(name, summarise(&diff(&getter), lower)?.to_json());
    }
    Ok(json!({
        "final_val_mse": val.to_json(), "invalid_offspring": invalid.to_json(),
        "metrics": metrics, "bridge_subset_proposals": bridges, "bridge_proposals": proposals,
        "bridge_subset_fraction": fraction,
        "verdict": if pairs.len() == DEFAULT_SEEDS as usize { verdict(&val, &invalid, fraction) } else { "incomplete" },
    }))
}

fn run(manifest: Value, file: &mut std::fs::File) -> Res<()> {
    let clock = Instant::now();
    let (mut trials, mut failures, mut summaries) = (Vec::new(), Vec::new(), BTreeMap::new());
    let mut completed = 0;
    for workload in WORKLOADS {
        let mut pairs = Vec::new();
        for seed in 0..DEFAULT_SEEDS {
            let prepared = match prepare(workload, seed) {
                Ok(p) => p,
                Err(e) => {
                    failures.push(json!({"workload": workload.name(), "seed": seed, "stage": "preparation", "cause": e.to_string()}));
                    continue;
                }
            };
            let mut results = [None, None];
            let order = if seed % 2 == 0 { [Arm::Unbiased, Arm::Bridge] } else { [Arm::Bridge, Arm::Unbiased] };
            for arm in order {
                match run_arm(arm, seed, &prepared.parent, &prepared.cal, &prepared.val, LIMITS) {
                    Ok(mut r) => {
                        r.preparation_share_seconds = prepared.seconds / 2.0;
                        if !r.complete() {
                            failures.push(json!({"workload": workload.name(), "seed": seed, "stage": arm.name(), "cause": r.stop}));
                        }
                        results[usize::from(arm == Arm::Bridge)] = Some(r);
                    }
                    Err(e) => failures.push(json!({"workload": workload.name(), "seed": seed, "stage": arm.name(), "cause": e.to_string()})),
                }
            }
            trials.push(json!({ "workload": workload.name(), "seed": seed, "preparation": prepared.costs,
                "arm_order": order.map(Arm::name),
                "unbiased": results[0].as_ref().map(ArmResult::to_json), "bridge": results[1].as_ref().map(ArmResult::to_json) }));
            if let [Some(u), Some(b)] = results
                && u.complete() && b.complete() {
                pairs.push((u, b));
                completed += 1;
            }
        }
        summaries.insert(workload.name(), family_summary(&pairs)?);
        println!("{}: {} pairs, {}", workload.name(), pairs.len(), summaries[workload.name()]["verdict"]);
    }
    let expected = DEFAULT_SEEDS as usize * WORKLOADS.len();
    let status = completion(expected, completed, failures.len());
    let overall = if status.is_err() { "incomplete" }
        else if WORKLOADS[1..].iter().all(|w| summaries[w.name()]["verdict"] == "better") { "better_on_generated_families_only" }
        else { "do_not_adopt" };
    let result = json!({
        "schema_version": 2, "manifest": manifest,
        "coverage": { "planned_pairs": expected, "attempted_pairs": expected, "completed_pairs": completed },
        "failures": failures, "summary": summaries, "verdict": overall,
        "experiment_seconds_indicative": clock.elapsed().as_secs_f64(),
        "trials": trials,
    });
    file.seek(SeekFrom::Start(0))?;
    file.set_len(0)?;
    serde_json::to_writer_pretty(&mut *file, &result)?;
    file.flush()?;
    status
}

pub(super) fn main() -> Res<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["--declare", path] => {
            let manifest = manifest()?;
            let file = OpenOptions::new().write(true).create_new(true).open(path)?;
            serde_json::to_writer_pretty(file, &manifest)?;
            Ok(())
        }
        ["--manifest", path, "--out", out] => {
            let declared: Value = serde_json::from_reader(std::fs::File::open(path)?)?;
            check_manifest(&declared, &manifest()?)?;
            let mut file = OpenOptions::new().write(true).create_new(true).open(out)?;
            serde_json::to_writer(&mut file, &json!({"status": "incomplete_running", "manifest": declared}))?;
            file.flush()?;
            run(declared, &mut file)
        }
        _ => Err("use --declare <manifest.json>, then --manifest <manifest.json> --out <new-results.json>".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_pairs_and_failures_cannot_pass_completion() {
        assert!(completion(2, 2, 0).is_ok());
        assert_eq!(completion(2, 1, 0).unwrap_err().to_string(), "incomplete experiment: 1/2 paired trials, 0 failures; see result file");
        assert_eq!(completion(2, 2, 1).unwrap_err().to_string(), "incomplete experiment: 2/2 paired trials, 1 failures; see result file");
    }

    #[test]
    fn manifest_and_binary_must_match_the_declared_source() {
        let declared = json!({"source": "a", "budget": 150});
        assert!(check_manifest(&declared, &declared).is_ok());
        let error = check_manifest(&declared, &json!({"source": "b", "budget": 150})).unwrap_err();
        assert!(error.to_string().contains("manifest differs"));
        assert!(source_matches(b"a", b"a", "source.rs").is_ok());
        assert_eq!(source_matches(b"a", b"b", "source.rs").unwrap_err().to_string(), "stale executable source for source.rs; rebuild with cargo run");
    }
}
