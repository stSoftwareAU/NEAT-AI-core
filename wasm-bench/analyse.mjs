// Issue #509 — summarise the benchmark CSV: median and dispersion per
// (variant, bench), the control→unchecked delta, and the parity check.
//
// Usage: node analyse.mjs results/<shape>.csv

import { readFileSync } from "node:fs";

const path = process.argv[2];
if (!path) {
  console.error("usage: node analyse.mjs <results.csv>");
  process.exit(2);
}

const rows = readFileSync(path, "utf8")
  .split("\n")
  .filter((line) => line.trim().length > 0)
  .map((line) => {
    const [variant, bench, sample, nanos, checksum, session] = line.split(",");
    return {
      variant,
      bench,
      sample: Number(sample),
      nanos: Number(nanos),
      checksum,
      session: Number(session ?? 0),
    };
  });

if (rows.length === 0) {
  console.error("analyse: no samples in " + path);
  process.exit(1);
}

const quantile = (sorted, q) => {
  const pos = (sorted.length - 1) * q;
  const lo = Math.floor(pos);
  const hi = Math.ceil(pos);
  return sorted[lo] + (sorted[hi] - sorted[lo]) * (pos - lo);
};

const benches = [...new Set(rows.map((r) => r.bench))];
const variants = [...new Set(rows.map((r) => r.variant))];

const stats = new Map();
for (const bench of benches) {
  for (const variant of variants) {
    const ns = rows
      .filter((r) => r.bench === bench && r.variant === variant)
      .map((r) => r.nanos)
      .sort((a, b) => a - b);
    if (ns.length === 0) continue;
    stats.set(`${bench}/${variant}`, {
      n: ns.length,
      min: ns[0],
      median: quantile(ns, 0.5),
      p25: quantile(ns, 0.25),
      p75: quantile(ns, 0.75),
      max: ns[ns.length - 1],
    });
  }
}

const ms = (n) => (n / 1e6).toFixed(3);
console.log("| bench | variant | n | median ms | IQR ms | min ms | max ms |");
console.log("| --- | --- | ---: | ---: | ---: | ---: | ---: |");
for (const bench of benches) {
  for (const variant of variants) {
    const s = stats.get(`${bench}/${variant}`);
    if (!s) continue;
    console.log(
      `| ${bench} | ${variant} | ${s.n} | ${ms(s.median)} | ${ms(s.p25)}–${ms(s.p75)} | ${ms(s.min)} | ${ms(s.max)} |`,
    );
  }
}

// Paired comparison: the driver alternates the two variants within a sample, so
// pairing them by occurrence cancels drift far better than comparing two
// independent medians on a loaded machine.
console.log("\n| bench | paired median ratio | paired IQR | pairs faster |");
console.log("| --- | ---: | ---: | ---: |");
for (const bench of benches) {
  const take = (v) => rows.filter((r) => r.bench === bench && r.variant === v).map((r) => r.nanos);
  const c = take("control");
  const u = take("unchecked");
  const pairs = Math.min(c.length, u.length);
  if (pairs === 0) continue;
  const ratios = [];
  for (let i = 0; i < pairs; i++) ratios.push(u[i] / c[i]);
  const sorted = [...ratios].sort((a, b) => a - b);
  const faster = ratios.filter((r) => r < 1).length;
  console.log(
    `| ${bench} | ${quantile(sorted, 0.5).toFixed(4)} | ` +
      `${quantile(sorted, 0.25).toFixed(4)}–${quantile(sorted, 0.75).toFixed(4)} | ` +
      `${faster}/${pairs} |`,
  );
}
console.log("(ratio < 1 = unchecked faster)");

// Repeatability: the same paired ratio, per session. A gain that only appears
// in one session is a session artefact, not an optimisation.
const sessions = [...new Set(rows.map((r) => r.session))].sort((a, b) => a - b);
console.log("\n| bench | " + sessions.map((s) => `session ${s}`).join(" | ") + " |");
console.log("| --- |" + sessions.map(() => " ---: |").join(""));
for (const bench of benches) {
  const cells = sessions.map((session) => {
    const take = (v) =>
      rows
        .filter((r) => r.bench === bench && r.variant === v && r.session === session)
        .map((r) => r.nanos);
    const c = take("control");
    const u = take("unchecked");
    const pairs = Math.min(c.length, u.length);
    if (pairs === 0) return "—";
    const ratios = [];
    for (let i = 0; i < pairs; i++) ratios.push(u[i] / c[i]);
    return quantile(ratios.sort((a, b) => a - b), 0.5).toFixed(4);
  });
  console.log(`| ${bench} | ${cells.join(" | ")} |`);
}
console.log("(per-session paired median ratio)");

console.log("\n| bench | median delta | min delta | parity |");
console.log("| --- | ---: | ---: | --- |");
for (const bench of benches) {
  const c = stats.get(`${bench}/control`);
  const u = stats.get(`${bench}/unchecked`);
  if (!c || !u) continue;
  const pct = (a, b) => (((b - a) / a) * 100).toFixed(2) + "%";
  // Parity is per sample index: a benchmark whose state evolves across samples
  // still has to produce bit-identical results between the two variants.
  const bySample = new Map();
  for (const r of rows.filter((x) => x.bench === bench)) {
    const key = `${r.variant}/${r.sample}`;
    const seen = bySample.get(key);
    if (seen && seen !== r.checksum) bySample.set(key, "unstable");
    else bySample.set(key, r.checksum);
  }
  const mismatches = [...new Set(rows.map((r) => r.sample))].filter((s) => {
    const a = bySample.get(`control/${s}`);
    const b = bySample.get(`unchecked/${s}`);
    return a !== undefined && b !== undefined && a !== b;
  });
  const parity = mismatches.length === 0
    ? "bit-identical"
    : `DIFFERS at samples ${mismatches.join(",")}`;
  console.log(`| ${bench} | ${pct(c.median, u.median)} | ${pct(c.min, u.min)} | ${parity} |`);
}
console.log("\n(negative delta = unchecked faster)");
