//! Criterion harness for data-parallel record scoring (Issue #179).
//!
//! Reports throughput (records/sec) for scoring a batch of records through one
//! creature on a single core versus all available cores, demonstrating the
//! per-core scaling that is the throughput lever for the "data" half of #175.
//!
//! Opt-in only — requires the `parallel` feature (rayon, native targets):
//!
//! ```text
//! cargo bench -p neat-core --features parallel --bench parallel_scoring
//! ```
//!
//! Without the feature the bench's `main` is a no-op shim so the target still
//! builds under `cargo build --all-targets` on every configuration. The
//! deterministic fixtures are shared with the `hot_paths` harness and the
//! `bench_fixtures` test via `benches/common/mod.rs`.

#[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
#[path = "common/mod.rs"]
#[allow(dead_code)] // shared fixture module; this bench uses only a subset
mod common;

#[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
mod bench {
    use super::common::{NETWORKS, NetSpec, build_inputs, build_network};
    use criterion::{Criterion, Throughput, criterion_group};
    use neat_core::network::CompiledNetwork;
    use rayon::ThreadPoolBuilder;
    use std::hint::black_box;

    fn spec(label: &str) -> &'static NetSpec {
        NETWORKS
            .iter()
            .find(|s| s.label == label)
            .unwrap_or_else(|| panic!("no NetSpec labelled {label}"))
    }

    fn build_records(net: &CompiledNetwork, count: usize) -> Vec<Vec<f32>> {
        (0..count)
            .map(|i| build_inputs(net.num_inputs(), 0x5C0E_0000 + i as u64))
            .collect()
    }

    /// Score `records` inside a rayon pool of exactly `threads` workers, so the
    /// 1-core and all-core measurements share one code path and differ only in
    /// pool size.
    fn score_in_pool(
        net: &CompiledNetwork,
        records: &[Vec<f32>],
        num_outputs: usize,
        threads: usize,
    ) -> Vec<Vec<f32>> {
        let pool = ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .expect("failed to build rayon pool");
        pool.install(|| net.score_records_parallel(records, num_outputs))
    }

    pub fn bench_parallel_scoring(c: &mut Criterion) {
        let all_cores = std::thread::available_parallelism().map_or(1, |n| n.get());
        const NUM_RECORDS: usize = 2048;

        for label in ["production", "production_2x"] {
            let s = spec(label);
            let net = build_network(s, 0x5EED);
            let records = build_records(&net, NUM_RECORDS);
            let num_outputs = s.num_outputs;

            let mut group = c.benchmark_group(format!("score_records/{label}"));
            group.throughput(Throughput::Elements(NUM_RECORDS as u64));

            group.bench_function("1_core", |b| {
                b.iter(|| black_box(score_in_pool(&net, black_box(&records), num_outputs, 1)))
            });

            group.bench_function(format!("{all_cores}_cores"), |b| {
                b.iter(|| {
                    black_box(score_in_pool(
                        &net,
                        black_box(&records),
                        num_outputs,
                        all_cores,
                    ))
                })
            });

            group.finish();
        }
    }

    criterion_group!(benches, bench_parallel_scoring);
}

#[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
criterion::criterion_main!(bench::benches);

#[cfg(not(all(feature = "parallel", not(target_arch = "wasm32"))))]
fn main() {
    eprintln!(
        "parallel_scoring bench is a no-op without `--features parallel` (native only); \
         run: cargo bench -p neat-core --features parallel --bench parallel_scoring"
    );
}
