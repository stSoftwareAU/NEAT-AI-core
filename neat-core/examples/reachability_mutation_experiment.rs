//! Paired experiment for Issue #759: does reachability-guided connection
//! addition beat unbiased connection addition?
//!
//! This is opt-in research tooling, separate from CI: it changes no default and
//! nothing gates on its numbers. Two arms share, per seed, the teacher, the data,
//! the initial student and the weight draw sequence; only the candidate
//! selection policy differs (`unbiased` picks any available forward pair,
//! `bridge` picks from `bridge_candidates` and falls back to the full pool when
//! that is empty). Acceptance is strict improvement of the calibration MSE;
//! validation records are scored only for the initial and final creature.
//!
//! Run:
//!
//! ```text
//! cargo run --release -p neat-core --example reachability_mutation_experiment -- \
//!     --out result.json [--rev <sha>] [--seeds <n>]
//! ```
//!
//! The task is a synthetic teacher-to-student regression. A result here does
//! **not** demonstrate a benefit (or harm) for real NEAT-AI corpora. Wall time
//! is recorded as indicative only; nothing asserts on it.

use neat_core::creature::{CreatureExport, NeuronExport, SynapseExport};
use neat_core::reachability::{ReachabilityClass, ReachabilityReport, reachability_diagnostics};
use neat_core::topology_ops::scan_available_connections;
use neat_core::{cleanup_creature, compile_creature, validate_creature_topology};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::error::Error;
use std::time::Instant;

// ---- Predeclared design (echoed in the manifest) ----
const DEFAULT_SEEDS: u64 = 40;
const NUM_INPUTS: usize = 4;
const NUM_OUTPUTS: usize = 1;
const TEACHER_HIDDEN: usize = 4;
const CAL_RECORDS: usize = 256;
const VAL_RECORDS: usize = 256;
const EVAL_BUDGET: usize = 150;
const MAX_PROPOSALS: usize = 600;
const BOOTSTRAP_RESAMPLES: usize = 2000;
const BOOTSTRAP_SEED: u64 = 0x00B0_0757_12AB;
const PAPER: &str = "https://arxiv.org/abs/2610.17425";

const STREAM_TEACHER: u64 = 1;
const STREAM_DATA: u64 = 2;
const STREAM_STUDENT: u64 = 3;
const STREAM_WEIGHTS: u64 = 4;
const STREAM_SELECT: u64 = 5;

type Res<T> = Result<T, Box<dyn Error>>;

/// Filters a flattened `[from, to, ...]` candidate buffer (the shape
/// `scan_available_connections` returns) to bridge proposals: the source is
/// input-reached (`Live` or `DeadEnd`), the target reaches an output (`Live` or
/// `BiasDriven`), and `Live -> Live` is excluded. Order is preserved. Refuses an
/// odd-length buffer or a pair index outside the report.
fn bridge_candidates(available_pairs: &[u32], report: &ReachabilityReport) -> Res<Vec<u32>> {
    if available_pairs.len() % 2 != 0 {
        return Err(format!("odd-length candidate buffer: {}", available_pairs.len()).into());
    }
    let n = report.classes().len();
    let mut kept = Vec::new();
    for (pair_index, pair) in available_pairs.as_chunks::<2>().0.iter().enumerate() {
        let (from, to) = (pair[0], pair[1]);
        let (Some(src), Some(dst)) = (report.class_of(from as usize), report.class_of(to as usize))
        else {
            return Err(format!(
                "candidate pair {pair_index} ({from} -> {to}) is outside {n} neurons"
            )
            .into());
        };
        use ReachabilityClass::*;
        let input_reached = matches!(src, Live | DeadEnd);
        let reaches_output = matches!(dst, Live | BiasDriven);
        if input_reached && reaches_output && !(src == Live && dst == Live) {
            kept.extend_from_slice(pair);
        }
    }
    Ok(kept)
}

// ---- RNG ----
struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// Independent sub-stream for `(seed, stream)`.
    fn stream(seed: u64, stream: u64) -> Rng {
        let mut a = Rng(seed);
        let first = a.next_u64();
        Rng(first ^ Rng(stream.wrapping_mul(0xD6E8_FEB8_6659_FD93)).next_u64())
    }
    fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
}

// ---- Creature builders ----
fn neuron(kind: &str, uuid: &str, bias: f64, squash: &str) -> NeuronExport {
    NeuronExport {
        id: None,
        neuron_type: kind.to_string(),
        uuid: uuid.to_string(),
        bias,
        squash: Some(squash.to_string()),
    }
}

fn edge(from: &str, to: &str, weight: f64, role: Option<&str>) -> SynapseExport {
    SynapseExport {
        from_uuid: from.to_string(),
        to_uuid: to.to_string(),
        weight,
        synapse_type: role.map(str::to_string),
    }
}

fn creature(neurons: Vec<NeuronExport>, synapses: Vec<SynapseExport>) -> CreatureExport {
    CreatureExport {
        input: NUM_INPUTS,
        output: NUM_OUTPUTS,
        neurons,
        synapses,
        semantic_version: None,
        forward_only: true,
        memetic: None,
    }
}

fn teacher(rng: &mut Rng) -> CreatureExport {
    let mut neurons = Vec::new();
    let mut synapses = Vec::new();
    for h in 0..TEACHER_HIDDEN {
        let id = format!("t-{h}");
        neurons.push(neuron("hidden", &id, 0.0, "TANH"));
        let a = rng.below(NUM_INPUTS);
        let b = (a + 1 + rng.below(NUM_INPUTS - 1)) % NUM_INPUTS;
        for src in [a, b] {
            let w = rng.range(-1.0, 1.0);
            synapses.push(edge(&format!("input-{src}"), &id, w, None));
        }
        synapses.push(edge(&id, "output-0", rng.range(-1.0, 1.0), None));
    }
    neurons.push(neuron("output", "output-0", 0.0, "IDENTITY"));
    creature(neurons, synapses)
}

fn initial_student(rng: &mut Rng) -> CreatureExport {
    let mut w = || rng.range(-0.3, 0.3);
    let neurons = vec![
        neuron("constant", "c-0", 1.0, "IDENTITY"),
        neuron("hidden", "h0", 0.0, "TANH"),
        neuron("hidden", "h1", 0.0, "LOGISTIC"),
        neuron("hidden", "h2", 0.0, "TANH"),
        neuron("hidden", "h3", 0.0, "IF"),
        neuron("hidden", "h4", 0.0, "TANH"),
        neuron("output", "output-0", 0.0, "IDENTITY"),
    ];
    let synapses = vec![
        edge("input-0", "h0", w(), None),
        edge("c-0", "h1", w(), None),
        edge("h1", "h2", w(), None),
        edge("input-1", "h3", 1.0, Some("condition")),
        edge("h0", "h3", w(), Some("positive")),
        edge("c-0", "h3", w(), Some("negative")),
        edge("input-2", "h4", w(), None),
        edge("h0", "output-0", w(), None),
        edge("h2", "output-0", w(), None),
        edge("h3", "output-0", w(), None),
        edge("h4", "output-0", w(), None),
    ];
    creature(neurons, synapses)
}

// ---- Index graph ----
struct Graph {
    from: Vec<u32>,
    to: Vec<u32>,
    is_constant: Vec<u8>,
    n: usize,
}

fn graph(c: &CreatureExport) -> Res<Graph> {
    let mut idx: HashMap<&str, u32> = HashMap::new();
    for (j, n) in c.neurons.iter().enumerate() {
        idx.insert(n.uuid.as_str(), (c.input + j) as u32);
    }
    let lookup = |u: &str| -> Res<u32> {
        if let Some(i) = u.strip_prefix("input-") {
            return Ok(i.parse()?);
        }
        idx.get(u)
            .copied()
            .ok_or_else(|| format!("unknown uuid {u}").into())
    };
    let (mut from, mut to) = (Vec::new(), Vec::new());
    for s in &c.synapses {
        from.push(lookup(&s.from_uuid)?);
        to.push(lookup(&s.to_uuid)?);
    }
    let n = c.input + c.neurons.len();
    let mut is_constant = vec![0u8; n];
    for (j, nr) in c.neurons.iter().enumerate() {
        is_constant[c.input + j] = u8::from(nr.neuron_type == "constant");
    }
    let outputs_last = c.neurons[c.neurons.len() - c.output..]
        .iter()
        .all(|x| x.neuron_type == "output");
    if !outputs_last {
        return Err("output neurons are not the final block".into());
    }
    Ok(Graph {
        from,
        to,
        is_constant,
        n,
    })
}

fn report(g: &Graph, c: &CreatureExport) -> Res<ReachabilityReport> {
    Ok(reachability_diagnostics(
        &g.from, &g.to, g.n, c.input, c.output,
    )?)
}

fn uuid_of(c: &CreatureExport, index: u32) -> String {
    let i = index as usize;
    if i < c.input {
        format!("input-{i}")
    } else {
        c.neurons[i - c.input].uuid.clone()
    }
}

// ---- Scoring ----
struct Data {
    inputs: Vec<[f32; NUM_INPUTS]>,
    targets: Vec<f32>,
}

fn make_data(teacher: &CreatureExport, rng: &mut Rng, count: usize) -> Res<Data> {
    let mut net = compile_creature(teacher)?;
    let mut inputs = Vec::with_capacity(count);
    let mut targets = Vec::with_capacity(count);
    for _ in 0..count {
        let mut x = [0f32; NUM_INPUTS];
        for v in &mut x {
            *v = rng.range(-1.0, 1.0) as f32;
        }
        targets.push(net.activate(&x, NUM_OUTPUTS)[0]);
        inputs.push(x);
    }
    Ok(Data { inputs, targets })
}

fn mse(c: &CreatureExport, data: &Data) -> Res<f64> {
    let mut net = compile_creature(c)?;
    let mut sum = 0.0f64;
    for (x, &t) in data.inputs.iter().zip(&data.targets) {
        let d = f64::from(net.activate(x, NUM_OUTPUTS)[0]) - f64::from(t);
        sum += d * d;
    }
    Ok(sum / data.targets.len() as f64)
}

// ---- One arm ----
#[derive(Clone, Copy, PartialEq)]
enum Arm {
    Unbiased,
    Bridge,
}

#[derive(Default)]
struct ArmResult {
    initial_cal_mse: f64,
    final_cal_mse: f64,
    initial_val_mse: f64,
    final_val_mse: f64,
    evaluations: usize,
    proposals: usize,
    invalid_offspring: usize,
    accepted: usize,
    bridge_fallbacks: usize,
    stopped_empty_pool: bool,
    wall_seconds: f64,
    final_live: usize,
    final_bias_driven: usize,
    final_dead_end: usize,
    final_isolated: usize,
    final_input_independent_outputs: usize,
    final_unused_inputs: usize,
    mean_nonlive_offspring: f64,
}

impl ArmResult {
    fn improvement_cal(&self) -> f64 {
        self.initial_cal_mse - self.final_cal_mse
    }
    fn final_nonlive(&self) -> f64 {
        (self.final_bias_driven + self.final_dead_end + self.final_isolated) as f64
    }
    fn to_json(&self) -> Value {
        let imp = self.improvement_cal();
        json!({
            "initial_cal_mse": self.initial_cal_mse,
            "final_cal_mse": self.final_cal_mse,
            "initial_val_mse": self.initial_val_mse,
            "final_val_mse": self.final_val_mse,
            "improvement_cal": imp,
            "improvement_per_eval": imp / self.evaluations as f64,
            "wall_seconds_indicative": self.wall_seconds,
            "improvement_per_eval_hour_indicative": imp / (self.wall_seconds / 3600.0),
            "evaluations": self.evaluations,
            "proposals": self.proposals,
            "invalid_offspring": self.invalid_offspring,
            "accepted": self.accepted,
            "bridge_fallbacks": self.bridge_fallbacks,
            "stopped_empty_pool": self.stopped_empty_pool,
            "final_interior": {
                "live": self.final_live,
                "bias_driven": self.final_bias_driven,
                "dead_end": self.final_dead_end,
                "isolated": self.final_isolated,
            },
            "final_input_independent_outputs": self.final_input_independent_outputs,
            "final_unused_inputs": self.final_unused_inputs,
            "mean_nonlive_interior_over_valid_offspring": self.mean_nonlive_offspring,
        })
    }
}

fn nonlive(rep: &ReachabilityReport) -> usize {
    let c = rep.interior_counts();
    c.bias_driven + c.dead_end + c.isolated
}

/// Picks the next `(from, to)` pair under the arm's policy.
fn pick(
    arm: Arm,
    parent: &CreatureExport,
    g: &Graph,
    rng: &mut Rng,
    fallbacks: &mut usize,
) -> Res<Option<(u32, u32)>> {
    let pool = scan_available_connections(
        &g.from,
        &g.to,
        &g.is_constant,
        g.n as u32,
        parent.input as u32,
    );
    if pool.is_empty() {
        return Ok(None);
    }
    let chosen = if arm == Arm::Bridge {
        let bridge = bridge_candidates(&pool, &report(g, parent)?)?;
        if bridge.is_empty() {
            *fallbacks += 1;
            pool
        } else {
            bridge
        }
    } else {
        pool
    };
    let k = rng.below(chosen.len() / 2);
    Ok(Some((chosen[2 * k], chosen[2 * k + 1])))
}

fn run_arm(arm: Arm, seed: u64, start: &CreatureExport, cal: &Data, val: &Data) -> Res<ArmResult> {
    let clock = Instant::now();
    let mut select = Rng::stream(seed, STREAM_SELECT);
    let mut weights = Rng::stream(seed, STREAM_WEIGHTS);
    let mut parent = start.clone();
    let mut parent_mse = mse(&parent, cal)?;
    let mut r = ArmResult {
        initial_cal_mse: parent_mse,
        initial_val_mse: mse(&parent, val)?,
        evaluations: 1,
        ..ArmResult::default()
    };
    let (mut evaluated, mut nonlive_sum) = (0usize, 0usize);
    while evaluated < EVAL_BUDGET && r.proposals < MAX_PROPOSALS {
        let g = graph(&parent)?;
        let Some((from, to)) = pick(arm, &parent, &g, &mut select, &mut r.bridge_fallbacks)? else {
            r.stopped_empty_pool = true;
            break;
        };
        r.proposals += 1;
        let weight = weights.range(-1.0, 1.0);
        let mut child = parent.clone();
        child.synapses.push(edge(
            &uuid_of(&parent, from),
            &uuid_of(&parent, to),
            weight,
            None,
        ));
        if validate_creature_topology(&child).is_err() {
            r.invalid_offspring += 1;
            continue;
        }
        nonlive_sum += nonlive(&report(&graph(&child)?, &child)?);
        let child_mse = mse(&child, cal)?;
        evaluated += 1;
        r.evaluations += 1;
        if child_mse < parent_mse {
            parent = child;
            parent_mse = child_mse;
            r.accepted += 1;
        }
    }
    r.final_cal_mse = parent_mse;
    r.final_val_mse = mse(&parent, val)?;
    let rep = report(&graph(&parent)?, &parent)?;
    let counts = rep.interior_counts();
    r.final_live = counts.live;
    r.final_bias_driven = counts.bias_driven;
    r.final_dead_end = counts.dead_end;
    r.final_isolated = counts.isolated;
    r.final_input_independent_outputs = rep.input_independent_outputs().len();
    r.final_unused_inputs = rep.unused_inputs().len();
    r.mean_nonlive_offspring = if evaluated == 0 {
        0.0
    } else {
        nonlive_sum as f64 / evaluated as f64
    };
    r.wall_seconds = clock.elapsed().as_secs_f64();
    Ok(r)
}

// ---- One trial ----
fn run_trial(seed: u64) -> Res<(ArmResult, ArmResult)> {
    let teacher = teacher(&mut Rng::stream(seed, STREAM_TEACHER));
    let mut data_rng = Rng::stream(seed, STREAM_DATA);
    let cal = make_data(&teacher, &mut data_rng, CAL_RECORDS)?;
    let val = make_data(&teacher, &mut data_rng, VAL_RECORDS)?;
    let raw = initial_student(&mut Rng::stream(seed, STREAM_STUDENT));
    let parent = cleanup_creature(&raw).map_err(|e| format!("cleanup refused: {e:?}"))?;
    let unbiased = run_arm(Arm::Unbiased, seed, &parent.creature, &cal, &val)?;
    let bridge = run_arm(Arm::Bridge, seed, &parent.creature, &cal, &val)?;
    Ok((unbiased, bridge))
}

// ---- Statistics ----
fn mean(v: &[f64]) -> f64 {
    if v.is_empty() {
        0.0
    } else {
        v.iter().sum::<f64>() / v.len() as f64
    }
}

fn bootstrap_ci(diffs: &[f64]) -> Option<(f64, f64)> {
    if diffs.is_empty() {
        return None;
    }
    let mut rng = Rng(BOOTSTRAP_SEED);
    let mut means: Vec<f64> = (0..BOOTSTRAP_RESAMPLES)
        .map(|_| {
            let s: f64 = (0..diffs.len())
                .map(|_| diffs[rng.below(diffs.len())])
                .sum();
            s / diffs.len() as f64
        })
        .collect();
    means.sort_by(f64::total_cmp);
    let lo = means[(BOOTSTRAP_RESAMPLES as f64 * 0.025) as usize];
    let hi = means[(BOOTSTRAP_RESAMPLES as f64 * 0.975) as usize - 1];
    Some((lo, hi))
}

struct Summary {
    mean_diff: f64,
    ci: Option<(f64, f64)>,
    bridge_wins: usize,
    bridge_losses: usize,
    ties: usize,
}

/// `diffs` are bridge minus unbiased; `lower_is_better` orients wins.
fn summarise(diffs: &[f64], lower_is_better: bool) -> Summary {
    let better = |d: f64| if lower_is_better { d < 0.0 } else { d > 0.0 };
    let worse = |d: f64| if lower_is_better { d > 0.0 } else { d < 0.0 };
    Summary {
        mean_diff: mean(diffs),
        ci: bootstrap_ci(diffs),
        bridge_wins: diffs.iter().filter(|&&d| better(d)).count(),
        bridge_losses: diffs.iter().filter(|&&d| worse(d)).count(),
        ties: diffs.iter().filter(|&&d| d == 0.0).count(),
    }
}

impl Summary {
    fn to_json(&self) -> Value {
        json!({
            "mean_bridge_minus_unbiased": self.mean_diff,
            "bootstrap_ci95": self.ci.map(|(l, h)| json!([l, h])),
            "bridge_wins": self.bridge_wins,
            "bridge_losses": self.bridge_losses,
            "ties": self.ties,
        })
    }
}

/// Predeclared criterion; see the module documentation.
fn verdict(val: &Summary, invalid: &Summary) -> &'static str {
    match val.ci {
        Some((_, hi)) if hi < 0.0 && invalid.mean_diff <= 0.0 => "better",
        Some((lo, _)) if lo > 0.0 => "worse",
        _ => "inconclusive",
    }
}

// ---- CLI ----
struct Args {
    out: String,
    rev: String,
    seeds: u64,
}

fn parse_args() -> Res<Args> {
    let mut args = std::env::args().skip(1);
    let (mut out, mut rev, mut seeds) = (None, "unknown".to_string(), DEFAULT_SEEDS);
    while let Some(flag) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--out" => out = Some(value()?),
            "--rev" => rev = value()?,
            "--seeds" => seeds = value()?.parse()?,
            other => return Err(format!("unknown argument {other}").into()),
        }
    }
    if seeds == 0 {
        return Err("--seeds must be at least 1".into());
    }
    let out = out.ok_or("missing required --out <path.json>")?;
    Ok(Args { out, rev, seeds })
}

fn manifest(args: &Args) -> Value {
    json!({
        "crate_version": env!("CARGO_PKG_VERSION"),
        "git_revision": args.rev,
        "paper": { "url": PAPER, "status": "unverified (fetch failed)" },
        "seed_range": [0, args.seeds],
        "task": "synthetic teacher-to-student regression: random forward-only teacher \
                 (4 TANH hidden, 2 random inputs each, IDENTITY output); student starts \
                 with bias-driven and IF structure, then cleanup_creature; connection \
                 addition only; acceptance = strictly lower calibration MSE",
        "constants": {
            "default_seeds": DEFAULT_SEEDS,
            "num_inputs": NUM_INPUTS,
            "num_outputs": NUM_OUTPUTS,
            "teacher_hidden": TEACHER_HIDDEN,
            "cal_records": CAL_RECORDS,
            "val_records": VAL_RECORDS,
            "eval_budget": EVAL_BUDGET,
            "max_proposals": MAX_PROPOSALS,
            "bootstrap_resamples": BOOTSTRAP_RESAMPLES,
            "bootstrap_seed": BOOTSTRAP_SEED,
        },
        "verdict_rule": "bridge better only if the paired bootstrap CI of (bridge - \
                         unbiased) final_val_mse lies entirely below 0 and the mean \
                         invalid_offspring difference <= 0; worse if the CI lies \
                         entirely above 0; otherwise inconclusive",
    })
}

fn main() -> Res<()> {
    let args = parse_args()?;
    let (mut trials, mut failures) = (Vec::new(), Vec::new());
    let mut pairs = Vec::new();
    for seed in 0..args.seeds {
        match run_trial(seed) {
            Ok((u, b)) => {
                trials
                    .push(json!({ "seed": seed, "unbiased": u.to_json(), "bridge": b.to_json() }));
                pairs.push((u, b));
            }
            Err(e) => failures.push(json!({ "seed": seed, "error": e.to_string() })),
        }
    }
    let diff = |f: &dyn Fn(&ArmResult) -> f64| -> Vec<f64> {
        pairs.iter().map(|(u, b)| f(b) - f(u)).collect()
    };
    let val = summarise(&diff(&|r| r.final_val_mse), true);
    let imp = summarise(&diff(&|r| r.improvement_cal()), false);
    let inv = summarise(&diff(&|r| r.invalid_offspring as f64), true);
    let dead = summarise(&diff(&|r| r.final_nonlive()), true);
    let verdict = verdict(&val, &inv);
    let result = json!({
        "manifest": manifest(&args),
        "coverage": { "attempted": args.seeds, "completed": pairs.len() },
        "failures": failures,
        "summary": {
            "final_val_mse": val.to_json(),
            "improvement_cal": imp.to_json(),
            "invalid_offspring": inv.to_json(),
            "final_nonlive_interior": dead.to_json(),
            "verdict": verdict,
        },
        "trials": trials,
    });
    std::fs::write(&args.out, serde_json::to_string_pretty(&result)?)?;
    println!(
        "seeds attempted {} completed {} failed {}",
        args.seeds,
        pairs.len(),
        failures.len()
    );
    println!(
        "final_val_mse bridge-unbiased mean {:.6} ci {:?} (wins {} losses {} ties {})",
        val.mean_diff, val.ci, val.bridge_wins, val.bridge_losses, val.ties
    );
    println!(
        "improvement_cal mean {:.6}; invalid_offspring mean {:.3}; nonlive interior mean {:.3}",
        imp.mean_diff, inv.mean_diff, dead.mean_diff
    );
    println!("verdict: {verdict}\nwrote {}", args.out);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use neat_core::reachability::{ReachabilityClass as C, reachability_diagnostics};

    /// Splits `(from, to)` pairs into the parallel arrays the API takes.
    fn split(edges: &[(u32, u32)]) -> (Vec<u32>, Vec<u32>) {
        (
            edges.iter().map(|e| e.0).collect(),
            edges.iter().map(|e| e.1).collect(),
        )
    }

    /// Inputs 0,1; interior 2..=7; outputs 8,9 with these edges:
    /// 0->2, 2->8, 4->3, 3->8, 0->5, 1->9 and 6 reading from 5 (6 is DeadEnd now).
    /// Classes: 0 L, 1 L, 2 L, 3 B, 4 B, 5 D, 6 D, 7 I, 8 L, 9 L.
    fn bridge_report() -> ReachabilityReport {
        let (f, t) = split(&[(0, 2), (2, 8), (4, 3), (3, 8), (0, 5), (1, 9), (5, 6)]);
        reachability_diagnostics(&f, &t, 10, 2, 2).unwrap()
    }

    #[test]
    fn bridge_candidates_keep_only_pairs_that_join_a_path() {
        let r = bridge_report();
        assert_eq!(r.class_of(6), Some(C::DeadEnd));
        let pairs = [
            0, 2, // Live -> Live: dropped
            0, 3, // Live -> BiasDriven: kept
            5, 8, // DeadEnd -> output (Live): kept
            4, 8, // BiasDriven source: dropped
            0, 7, // Isolated target: dropped
            6, 3, // DeadEnd -> BiasDriven: kept
            5, 6, // DeadEnd -> DeadEnd: target reaches no output, dropped
            7, 8, // Isolated source: dropped
        ];
        assert_eq!(
            bridge_candidates(&pairs, &r).unwrap(),
            vec![0, 3, 5, 8, 6, 3]
        );
    }

    #[test]
    fn bridge_candidates_refuse_bad_buffers() {
        let r = bridge_report();
        let odd = bridge_candidates(&[0, 3, 5], &r).unwrap_err().to_string();
        assert!(odd.contains("odd"), "unexpected message: {odd}");
        assert!(bridge_candidates(&[0, 3], &r).is_ok());
        let range = bridge_candidates(&[0, 3, 5, 10], &r)
            .unwrap_err()
            .to_string();
        assert!(range.contains("outside"), "unexpected message: {range}");
        assert!(bridge_candidates(&[0, 3, 5, 9], &r).is_ok());
    }
}
