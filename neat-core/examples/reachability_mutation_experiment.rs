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
//! From clean committed sources, declare the protocol before running:
//!
//! ```text
//! export NEAT_EXPERIMENT_SOURCE_TREE="$(git rev-parse HEAD^{tree})"
//! cargo run --release -p neat-core --example reachability_mutation_experiment -- \
//!     --declare manifest.json
//! cargo run --release -p neat-core --example reachability_mutation_experiment -- \
//!     --manifest manifest.json --out result.json
//! ```
//!
//! Four generated teacher-to-student families are specified in
//! `docs/research/reachability-mutation-protocol-v2.md`. A result here does
//! **not** demonstrate a benefit (or harm) for real NEAT-AI corpora. Wall time
//! is recorded as indicative only; nothing asserts on it.

use neat_core::creature::{CreatureExport, NeuronExport, SynapseExport};
use neat_core::if_graft::{IfNodeSpec, graft_if_node};
use neat_core::loss::mse_record;
use neat_core::reachability::{ReachabilityClass, ReachabilityReport, reachability_diagnostics};
use neat_core::topology_ops::scan_available_connections;
use neat_core::{cleanup_creature, compile_creature, validate_creature_topology};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use std::error::Error;
use std::process::Command;
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
const BOOTSTRAP_RESAMPLES: usize = 12_000;
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
    if data.targets.is_empty() {
        return Err("empty scoring data".into());
    }
    if data.inputs.len() != data.targets.len() {
        return Err("scoring input/target length mismatch".into());
    }
    if data.targets.iter().any(|x| !x.is_finite())
        || data.inputs.iter().flatten().any(|x| !x.is_finite())
    {
        return Err("non-finite scoring data".into());
    }
    let mut net = compile_creature(c)?;
    let mut sum = 0.0f64;
    for (x, &t) in data.inputs.iter().zip(&data.targets) {
        sum += mse_record(&[t], &net.activate(x, NUM_OUTPUTS));
    }
    let score = sum / data.targets.len() as f64;
    if !score.is_finite() {
        return Err("non-finite MSE".into());
    }
    Ok(score)
}

// ---- Generated workload families ----
#[derive(Clone, Copy, Debug, PartialEq)]
enum Workload {
    Legacy,
    Sparse,
    Mixed,
    Conditional,
}
const WORKLOADS: [Workload; 4] = [
    Workload::Legacy,
    Workload::Sparse,
    Workload::Mixed,
    Workload::Conditional,
];

impl Workload {
    fn name(self) -> &'static str {
        match self {
            Self::Legacy => "legacy",
            Self::Sparse => "sparse",
            Self::Mixed => "mixed",
            Self::Conditional => "conditional",
        }
    }
    fn hidden(self) -> usize {
        if self == Self::Mixed { 48 } else { 24 }
    }
}

fn generated_teacher(workload: Workload, rng: &mut Rng) -> Res<CreatureExport> {
    if workload == Workload::Legacy {
        return Ok(teacher(rng));
    }
    let mut neurons = vec![neuron("constant", "c-0", 1.0, "IDENTITY")];
    let mut synapses = Vec::new();
    for h in 0..workload.hidden() {
        let id = format!("h{h}");
        let squash = if workload == Workload::Mixed {
            ["TANH", "LOGISTIC", "MEAN", "MAXIMUM"][h % 4]
        } else {
            "TANH"
        };
        neurons.push(neuron("hidden", &id, rng.range(-0.3, 0.3), squash));
        synapses.push(edge("c-0", &id, rng.range(-0.3, 0.3), None));
        let a = rng.below(NUM_INPUTS);
        let b = (a + 1 + rng.below(NUM_INPUTS - 1)) % NUM_INPUTS;
        for src in [a, b] {
            synapses.push(edge(
                &format!("input-{src}"),
                &id,
                rng.range(-1.0, 1.0),
                None,
            ));
        }
        synapses.push(edge(&id, "output-0", rng.range(-1.0, 1.0), None));
    }
    neurons.push(neuron("output", "output-0", 0.0, "IDENTITY"));
    let mut c = cleanup_creature(&creature(neurons, synapses))?.creature;
    if workload == Workload::Conditional {
        let mut spec = IfNodeSpec::new("decision", 0.0)
            .with_condition("input-0", 1.0)
            .with_target("output-0", 1.0);
        for h in 0..workload.hidden() {
            let id = format!("h{h}");
            spec = if h % 2 == 0 {
                spec.with_positive(id, rng.range(-1.0, 1.0))
            } else {
                spec.with_negative(id, rng.range(-1.0, 1.0))
            };
        }
        c = graft_if_node(&c, &spec)?;
    }
    Ok(c)
}

fn generated_student(workload: Workload, seed: u64, teacher: &CreatureExport) -> CreatureExport {
    if workload == Workload::Legacy {
        return initial_student(&mut Rng::stream(seed, STREAM_STUDENT));
    }
    let mut c = teacher.clone();
    c.synapses.retain(|s| {
        let hidden = s
            .to_uuid
            .strip_prefix('h')
            .and_then(|n| n.parse::<u64>().ok());
        !(s.from_uuid.starts_with("input-") && hidden.is_some_and(|h| h % 4 != seed % 4))
    });
    c
}

fn structure(c: &CreatureExport) -> Res<Value> {
    let rep = report(&graph(c)?, c)?;
    let counts = rep.interior_counts();
    Ok(
        json!({ "live": counts.live, "bias_driven": counts.bias_driven,
        "dead_end": counts.dead_end, "isolated": counts.isolated,
        "unused_inputs": rep.unused_inputs(), "input_independent_outputs": rep.input_independent_outputs(),
        "neurons": c.neurons.len(), "synapses": c.synapses.len() }),
    )
}

// ---- One arm ----
#[derive(Clone, Copy, PartialEq)]
enum Arm {
    Unbiased,
    Bridge,
}
impl Arm {
    fn name(self) -> &'static str {
        if self == Self::Bridge {
            "bridge"
        } else {
            "unbiased"
        }
    }
}

#[derive(Default)]
struct ScoreCost {
    seconds: f64,
    calls: usize,
    records: usize,
}
impl ScoreCost {
    fn add(&mut self, seconds: f64, records: usize) {
        self.seconds += seconds;
        self.calls += 1;
        self.records += records;
    }
    fn json(&self) -> Value {
        json!({ "seconds_indicative": self.seconds, "calls": self.calls, "records": self.records })
    }
}
fn timed_mse(c: &CreatureExport, data: &Data, cost: &mut ScoreCost) -> Res<f64> {
    let clock = Instant::now();
    let score = mse(c, data)?;
    cost.add(clock.elapsed().as_secs_f64(), data.targets.len());
    Ok(score)
}
fn rate(improvement: f64, seconds: f64) -> Option<f64> {
    if seconds > 0.0 {
        Some(improvement * 3600.0 / seconds)
    } else {
        None
    }
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
    bridge_selected: usize,
    rejected: BTreeMap<String, usize>,
    stop: &'static str,
    wall_seconds: f64,
    preparation_share_seconds: f64,
    calibration_cost: ScoreCost,
    validation_cost: ScoreCost,
    candidate_preparation_seconds: f64,
    final_live: usize,
    final_bias_driven: usize,
    final_dead_end: usize,
    final_isolated: usize,
    final_input_independent_outputs: usize,
    final_unused_inputs: usize,
    mean_dead_offspring: f64,
    mean_bias_offspring: f64,
}
impl ArmResult {
    fn improvement_cal(&self) -> f64 {
        self.initial_cal_mse - self.final_cal_mse
    }
    fn final_dead(&self) -> f64 {
        (self.final_dead_end + self.final_isolated) as f64
    }
    fn total_seconds(&self) -> f64 {
        self.wall_seconds + self.preparation_share_seconds
    }
    fn complete(&self) -> bool {
        matches!(self.stop, "evaluation_budget" | "empty_pool")
    }
    fn to_json(&self) -> Value {
        let imp = self.improvement_cal();
        json!({
            "initial_cal_mse": self.initial_cal_mse, "final_cal_mse": self.final_cal_mse,
            "initial_val_mse": self.initial_val_mse, "final_val_mse": self.final_val_mse,
            "improvement_cal": imp,
            "improvement_per_eval": if self.evaluations > 0 { Some(imp / self.evaluations as f64) } else { None },
            "search_seconds_indicative": self.wall_seconds,
            "preparation_share_seconds_indicative": self.preparation_share_seconds,
            "total_seconds_indicative": self.total_seconds(),
            "calibration_scoring": self.calibration_cost.json(), "validation_scoring": self.validation_cost.json(),
            "candidate_preparation_seconds_indicative": self.candidate_preparation_seconds,
            "improvement_per_eval_hour_indicative": rate(imp, self.calibration_cost.seconds),
            "improvement_per_total_hour_indicative": rate(imp, self.total_seconds()),
            "evaluations": self.evaluations, "proposals": self.proposals,
            "invalid_offspring": self.invalid_offspring, "accepted": self.accepted,
            "rejection_causes": self.rejected,
            "bridge_fallbacks": self.bridge_fallbacks, "bridge_selected": self.bridge_selected,
            "stop": self.stop, "complete": self.complete(),
            "final_interior": { "live": self.final_live, "bias_driven": self.final_bias_driven,
                "dead_end": self.final_dead_end, "isolated": self.final_isolated },
            "final_dead_structure": self.final_dead_end + self.final_isolated,
            "final_input_independent_outputs": self.final_input_independent_outputs,
            "final_unused_inputs": self.final_unused_inputs,
            "mean_dead_interior_over_valid_offspring": if self.evaluations > 1 { Some(self.mean_dead_offspring) } else { None },
            "mean_bias_driven_interior_over_valid_offspring": if self.evaluations > 1 { Some(self.mean_bias_offspring) } else { None },
        })
    }
}

/// Picks the next pair and whether it came from the nonempty bridge subset.
fn pick(
    arm: Arm,
    parent: &CreatureExport,
    g: &Graph,
    rng: &mut Rng,
) -> Res<Option<((u32, u32), bool)>> {
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
    let bridge = if arm == Arm::Bridge {
        bridge_candidates(&pool, &report(g, parent)?)?
    } else {
        Vec::new()
    };
    let guided = !bridge.is_empty();
    let chosen = if guided { &bridge } else { &pool };
    let k = rng.below(chosen.len() / 2);
    Ok(Some(((chosen[2 * k], chosen[2 * k + 1]), guided)))
}

fn candidate(parent: &CreatureExport, from: u32, to: u32, weight: f64) -> Res<CreatureExport> {
    if from as usize >= parent.input + parent.neurons.len()
        || to as usize >= parent.input + parent.neurons.len()
    {
        return Err("candidate endpoint outside parent".into());
    }
    let mut child = parent.clone();
    child.synapses.push(edge(
        &uuid_of(parent, from),
        &uuid_of(parent, to),
        weight,
        None,
    ));
    validate_creature_topology(&child).map_err(|e| format!("topology: {e:?}"))?;
    // Cleanup owns canonical ordering and the full creature validation as well as rewrites.
    Ok(cleanup_creature(&child)
        .map_err(|e| format!("cleanup: {e}"))?
        .creature)
}
fn accept(
    parent: &mut CreatureExport,
    parent_mse: &mut f64,
    child: CreatureExport,
    score: f64,
) -> bool {
    if score.is_finite() && score < *parent_mse {
        *parent = child;
        *parent_mse = score;
        true
    } else {
        false
    }
}

#[derive(Clone, Copy)]
struct Limits {
    evaluations: usize,
    proposals: usize,
}
const LIMITS: Limits = Limits {
    evaluations: EVAL_BUDGET,
    proposals: MAX_PROPOSALS,
};

fn run_arm(
    arm: Arm,
    seed: u64,
    start: &CreatureExport,
    cal: &Data,
    val: &Data,
    limits: Limits,
) -> Res<ArmResult> {
    let clock = Instant::now();
    if !start.forward_only {
        return Err("experiment requires forward-only parents".into());
    }
    validate_creature_topology(start)?;
    let mut select = Rng::stream(seed, STREAM_SELECT);
    let mut weights = Rng::stream(seed, STREAM_WEIGHTS);
    let mut parent = start.clone();
    let mut r = ArmResult::default();
    let mut parent_mse = timed_mse(&parent, cal, &mut r.calibration_cost)?;
    r.initial_cal_mse = parent_mse;
    r.initial_val_mse = timed_mse(&parent, val, &mut r.validation_cost)?;
    r.evaluations = 1;
    let (mut evaluated, mut dead_sum, mut bias_sum) = (0usize, 0usize, 0usize);
    r.stop = "proposal_limit";
    while evaluated < limits.evaluations && r.proposals < limits.proposals {
        let g = graph(&parent)?;
        let Some(((from, to), guided)) = pick(arm, &parent, &g, &mut select)? else {
            r.stop = "empty_pool";
            break;
        };
        r.proposals += 1;
        r.bridge_selected += usize::from(guided);
        r.bridge_fallbacks += usize::from(arm == Arm::Bridge && !guided);
        let clock = Instant::now();
        let child = candidate(&parent, from, to, weights.range(-1.0, 1.0));
        r.candidate_preparation_seconds += clock.elapsed().as_secs_f64();
        let child = match child {
            Ok(child) => child,
            Err(error) => {
                r.invalid_offspring += 1;
                *r.rejected.entry(error.to_string()).or_default() += 1;
                continue;
            }
        };
        let counts = report(&graph(&child)?, &child)?.interior_counts();
        dead_sum += counts.dead_end + counts.isolated;
        bias_sum += counts.bias_driven;
        let child_mse = timed_mse(&child, cal, &mut r.calibration_cost)?;
        evaluated += 1;
        r.evaluations += 1;
        if accept(&mut parent, &mut parent_mse, child, child_mse) {
            r.accepted += 1;
        } else {
            *r.rejected.entry("not_strictly_better".into()).or_default() += 1;
        }
    }
    if evaluated == limits.evaluations {
        r.stop = "evaluation_budget";
    }
    r.final_cal_mse = parent_mse;
    r.final_val_mse = timed_mse(&parent, val, &mut r.validation_cost)?;
    let rep = report(&graph(&parent)?, &parent)?;
    let counts = rep.interior_counts();
    r.final_live = counts.live;
    r.final_bias_driven = counts.bias_driven;
    r.final_dead_end = counts.dead_end;
    r.final_isolated = counts.isolated;
    r.final_input_independent_outputs = rep.input_independent_outputs().len();
    r.final_unused_inputs = rep.unused_inputs().len();
    if evaluated > 0 {
        r.mean_dead_offspring = dead_sum as f64 / evaluated as f64;
        r.mean_bias_offspring = bias_sum as f64 / evaluated as f64;
    }
    r.wall_seconds = clock.elapsed().as_secs_f64();
    Ok(r)
}

// ---- One trial ----
struct Prepared {
    parent: CreatureExport,
    cal: Data,
    val: Data,
    costs: Value,
    seconds: f64,
}
fn prepare(workload: Workload, seed: u64) -> Res<Prepared> {
    let clock = Instant::now();
    let t = Instant::now();
    let teacher = generated_teacher(workload, &mut Rng::stream(seed, STREAM_TEACHER))?;
    let teacher_seconds = t.elapsed().as_secs_f64();
    let t = Instant::now();
    let mut rng = Rng::stream(seed, STREAM_DATA);
    let cal = make_data(&teacher, &mut rng, CAL_RECORDS)?;
    let val = make_data(&teacher, &mut rng, VAL_RECORDS)?;
    let data_seconds = t.elapsed().as_secs_f64();
    let t = Instant::now();
    let raw = generated_student(workload, seed, &teacher);
    let before = structure(&raw)?;
    let cleanup_clock = Instant::now();
    let parent = cleanup_creature(&raw)?.creature;
    let cleanup_seconds = cleanup_clock.elapsed().as_secs_f64();
    let after = structure(&parent)?;
    let student_seconds = t.elapsed().as_secs_f64();
    let seconds = clock.elapsed().as_secs_f64();
    Ok(Prepared {
        parent,
        cal,
        val,
        seconds,
        costs: json!({
            "teacher_seconds_indicative": teacher_seconds, "data_seconds_indicative": data_seconds,
            "cleanup_seconds_indicative": cleanup_seconds, "student_preparation_seconds_indicative": student_seconds, "shared_preparation_seconds_indicative": seconds,
            "raw_structure": before, "cleaned_structure": after,
        }),
    })
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
    let lo = means[75];
    let hi = means[BOOTSTRAP_RESAMPLES - 76];
    Some((lo, hi))
}

struct Summary {
    pairs: usize,
    mean_diff: f64,
    ci: Option<(f64, f64)>,
    bridge_wins: usize,
    bridge_losses: usize,
    ties: usize,
}

/// `diffs` are bridge minus unbiased; `lower_is_better` orients wins.
fn summarise(diffs: &[f64], lower_is_better: bool) -> Res<Summary> {
    if diffs.iter().any(|x| !x.is_finite()) {
        return Err("non-finite paired difference".into());
    }
    let better = |d: f64| if lower_is_better { d < 0.0 } else { d > 0.0 };
    let worse = |d: f64| if lower_is_better { d > 0.0 } else { d < 0.0 };
    Ok(Summary {
        pairs: diffs.len(),
        mean_diff: mean(diffs),
        ci: bootstrap_ci(diffs),
        bridge_wins: diffs.iter().filter(|&&d| better(d)).count(),
        bridge_losses: diffs.iter().filter(|&&d| worse(d)).count(),
        ties: diffs.iter().filter(|&&d| d == 0.0).count(),
    })
}

impl Summary {
    fn to_json(&self) -> Value {
        json!({
            "pairs": self.pairs,
            "mean_bridge_minus_unbiased": if self.pairs > 0 { Some(self.mean_diff) } else { None },
            "bootstrap_ci9875": self.ci.map(|(l, h)| json!([l, h])),
            "bridge_wins": self.bridge_wins,
            "bridge_losses": self.bridge_losses,
            "ties": self.ties,
        })
    }
}

/// Predeclared criterion; see the module documentation.
fn verdict(val: &Summary, invalid: &Summary, bridge_fraction: f64) -> &'static str {
    match val.ci {
        Some((_, hi)) if hi < 0.0 && invalid.mean_diff <= 0.0 && bridge_fraction >= 0.2 => "better",
        Some((lo, _)) if lo > 0.0 => "worse",
        _ => "inconclusive",
    }
}

// ---- CLI and provenance ----
#[path = "reachability_mutation_experiment/driver.rs"]
mod driver;
fn main() -> Res<()> {
    driver::main()
}

#[cfg(test)]
#[path = "reachability_mutation_experiment/tests.rs"]
mod tests;
