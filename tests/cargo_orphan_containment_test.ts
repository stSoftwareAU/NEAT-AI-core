// Containment gate for the orphaned transitive Cargo crates — Issues #676,
// #677, #720.
//
// Three unmaintained crates reach this graph, all forced in by `criterion`, the
// benchmark harness `neat-core` declares as a dev-dependency.
//
// `winapi` is the legacy raw-FFI Windows bindings crate: last release 0.3.9
// (2020-06-26), no maintainer triage since, superseded by `windows-sys`. It is
// not a choice this repository makes. It arrives as
// `winapi ← page_size ← criterion`, and neither edge is removable here:
// criterion 0.8.x declares `page_size ^0.6` as a plain, non-optional,
// non-target-gated dependency (so no feature set drops it), and every published
// page_size release — 0.1.0 through the latest 0.6.0 — depends on `winapi`, not
// on `windows-sys`. The exposure is bounded instead: criterion is a
// dev-dependency, so the crate never reaches the shipped library or the wasm
// bundle, and page_size's winapi edge is `cfg(windows)`-gated.
//
// `tinytemplate` is the templating engine criterion renders its local HTML
// benchmark reports with: last release 1.2.1 (2021-03-04), with "Project
// dead?", "Maintenance?" and a CVE-2023-38497 report all still open and
// unanswered upstream. It arrives as `tinytemplate <- criterion`, one edge
// shorter and no more removable: criterion 0.8.2 declares it non-optional, and
// the `html_reports` feature that renders those reports carries an empty
// feature list — so turning the feature off costs the reports and resolves the
// crate regardless. It is bounded by the same dev-dependency boundary.
//
// `alloca` is the dynamic stack-allocation helper criterion declares for native
// targets: last release 0.4.0 (2021-01), upstream repository
// dormant since 2023-08, with an undefined-behaviour report and a wasm32 build
// failure both open and unanswered. It arrives as `alloca <- criterion`, the
// same one-edge shape as tinytemplate and just as unremovable: criterion 0.8.2
// declares it non-optional for `cfg(any(windows, unix))`, so every native build
// resolves it whatever features are chosen here. It is bounded by the same
// dev-dependency boundary.
//
// "Bounded" was an assumption nothing enforced. This gate makes it an
// invariant: `deny.toml` pins each wrapper chain so `cargo deny check bans`
// fails the build the moment anything other than page_size pulls `winapi`, or
// anything other than criterion pulls `page_size`, `tinytemplate` or `alloca`,
// and these tests fail if that policy is dropped from `deny.toml`, if the
// committed `Cargo.lock` grows a new path to any of those crates, or if
// criterion stops being a dev-dependency.
//
// An edge disappears — with no source change here — once criterion drops the
// dependency upstream, or (for winapi) page_size migrates to `windows-sys`.
// Re-bump then and delete that crate's `deny.toml` entry; these tests are what
// tells you the shape changed.
//
// "What" tests: each one reads a committed artefact and asserts on the graph it
// actually describes, and the parsers are exercised against synthetic fixtures
// so a violation is proven detectable rather than assumed.
//
// Run: deno test --allow-read tests/cargo_orphan_containment_test.ts

import { assert, assertEquals } from "@std/assert";

const REPO_ROOT = new URL("../", import.meta.url);
const CARGO_LOCK = new URL("Cargo.lock", REPO_ROOT);
const DENY_TOML = new URL("deny.toml", REPO_ROOT);
const CORE_MANIFEST = new URL("neat-core/Cargo.toml", REPO_ROOT);

/** One `[[package]]` block of a `Cargo.lock`. */
export interface LockPackage {
  name: string;
  version: string;
  /** Dependency crate names, with the version/source suffix stripped. */
  dependencies: string[];
}

/** One `[bans] deny` entry of `deny.toml`. */
export interface BanEntry {
  /** The banned crate, from the entry's `crate` (or legacy `name`) key. */
  name: string;
  /** Crates permitted to depend on `name`; empty when none is declared. */
  wrappers: string[];
}

/** Strip the surrounding quotes from a TOML basic string. */
function unquote(value: string): string {
  return value.replace(/^\s*"/, "").replace(/"\s*$/, "");
}

/**
 * Every `[[package]]` block of a `Cargo.lock`. Lockfiles are a fixed, generated
 * TOML subset — one key per line, dependencies as a bare string array — so the
 * fields this gate needs are read directly rather than through a TOML library.
 */
export function parseLockPackages(lockText: string): LockPackage[] {
  const packages: LockPackage[] = [];
  let current: LockPackage | undefined;
  let inDependencies = false;

  for (const raw of lockText.split("\n")) {
    const line = raw.trim();
    if (line === "[[package]]") {
      current = { name: "", version: "", dependencies: [] };
      packages.push(current);
      inDependencies = false;
      continue;
    }
    if (current === undefined) continue;
    if (inDependencies) {
      if (line === "]") {
        inDependencies = false;
        continue;
      }
      // Entries are "name", "name version" or "name version (source)".
      const entry = unquote(line.replace(/,$/, ""));
      const name = entry.split(" ")[0];
      if (name !== "") current.dependencies.push(name);
      continue;
    }
    if (line.startsWith("name = ")) current.name = unquote(line.slice(7));
    else if (line.startsWith("version = ")) {
      current.version = unquote(line.slice(10));
    } else if (line.startsWith("dependencies = [")) inDependencies = true;
  }
  return packages;
}

/** Sorted names of the packages that depend on `crate`, deduplicated. */
export function dependentsOf(
  packages: readonly LockPackage[],
  crate: string,
): string[] {
  const dependents = new Set<string>();
  for (const pkg of packages) {
    if (pkg.dependencies.includes(crate)) dependents.add(pkg.name);
  }
  return [...dependents].sort();
}

/**
 * The `[bans] deny` entries of a `deny.toml`, each with the `wrappers`
 * allow-list that bounds which crates may depend on it.
 */
export function parseBanDenyEntries(denyText: string): BanEntry[] {
  const section = /\[bans\][\s\S]*?\bdeny\s*=\s*\[([\s\S]*?)\n\]/.exec(
    denyText,
  );
  if (section === null) return [];
  const entries: BanEntry[] = [];
  for (const match of section[1].matchAll(/\{([^}]*)\}/g)) {
    const body = match[1];
    // cargo-deny spells the package `crate = "…"`; `name = "…"` is its legacy
    // spelling and is still accepted, so both are read here.
    const name = /\bcrate\s*=\s*"([^"]*)"/.exec(body)?.[1] ??
      /\bname\s*=\s*"([^"]*)"/.exec(body)?.[1];
    if (name === undefined) continue;
    const wrapperList = /wrappers\s*=\s*\[([^\]]*)\]/.exec(body)?.[1] ?? "";
    const wrappers = [...wrapperList.matchAll(/"([^"]*)"/g)].map((w) => w[1]);
    entries.push({ name, wrappers });
  }
  return entries;
}

/**
 * The manifest sections — `dependencies`, `dev-dependencies`, `build-dependencies`,
 * including any `target.'cfg(…)'.` prefix — that declare `crate`.
 */
export function sectionsDeclaring(
  manifestText: string,
  crate: string,
): string[] {
  const sections: string[] = [];
  let section = "";
  for (const raw of manifestText.split("\n")) {
    const line = raw.trim();
    const header = /^\[([^\]]+)\]$/.exec(line);
    if (header !== null) {
      section = header[1];
      continue;
    }
    const key = /^([A-Za-z0-9_.-]+)\s*=/.exec(line)?.[1] ??
      /^"([^"]+)"\s*=/.exec(line)?.[1];
    if (key === crate && !sections.includes(section)) sections.push(section);
  }
  return sections;
}

/** The committed root lockfile, parsed once per test that needs it. */
async function lockPackages(): Promise<LockPackage[]> {
  return parseLockPackages(await Deno.readTextFile(CARGO_LOCK));
}

Deno.test("Cargo.lock reaches winapi through page_size and nothing else", async () => {
  const packages = await lockPackages();
  // Guard against a vacuous pass if the parser ever stops finding packages.
  assert(packages.length > 0, "Cargo.lock parsed to no packages");
  assertEquals(dependentsOf(packages, "winapi"), ["page_size"]);
});

Deno.test("Cargo.lock reaches page_size through criterion and nothing else", async () => {
  const packages = await lockPackages();
  assertEquals(dependentsOf(packages, "page_size"), ["criterion"]);
});

Deno.test("Cargo.lock reaches tinytemplate through criterion and nothing else", async () => {
  const packages = await lockPackages();
  assertEquals(dependentsOf(packages, "tinytemplate"), ["criterion"]);
});

Deno.test("Cargo.lock reaches alloca through criterion and nothing else", async () => {
  const packages = await lockPackages();
  assertEquals(dependentsOf(packages, "alloca"), ["criterion"]);
});

Deno.test("criterion is declared as a dev-dependency only, so neither crate ships", async () => {
  const manifest = await Deno.readTextFile(CORE_MANIFEST);
  assertEquals(sectionsDeclaring(manifest, "criterion"), ["dev-dependencies"]);
});

Deno.test("deny.toml pins both wrapper chains so cargo deny fails on a new path", async () => {
  const entries = parseBanDenyEntries(await Deno.readTextFile(DENY_TOML));
  const byName = new Map(entries.map((entry) => [entry.name, entry.wrappers]));
  assertEquals(
    byName.get("winapi"),
    ["page_size"],
    "deny.toml [bans] must deny winapi except through page_size",
  );
  assertEquals(
    byName.get("page_size"),
    ["criterion"],
    "deny.toml [bans] must deny page_size except through criterion",
  );
  assertEquals(
    byName.get("tinytemplate"),
    ["criterion"],
    "deny.toml [bans] must deny tinytemplate except through criterion",
  );
  assertEquals(
    byName.get("alloca"),
    ["criterion"],
    "deny.toml [bans] must deny alloca except through criterion",
  );
});

Deno.test("dependentsOf reports a second path into a denied crate", () => {
  const packages = parseLockPackages(`
[[package]]
name = "page_size"
version = "0.6.0"
dependencies = [
 "libc",
 "winapi",
]

[[package]]
name = "neat-core"
version = "0.20.1"
dependencies = [
 "serde 1.0.0 (registry+https://github.com/rust-lang/crates.io-index)",
 "winapi",
]
`);
  assertEquals(packages.length, 2);
  assertEquals(dependentsOf(packages, "winapi"), ["neat-core", "page_size"]);
  // The version/source suffix is stripped, so the name matches either spelling.
  assertEquals(dependentsOf(packages, "serde"), ["neat-core"]);
  assertEquals(dependentsOf(packages, "windows-sys"), []);
});

Deno.test("parseBanDenyEntries reports an unwrapped ban", () => {
  const entries = parseBanDenyEntries(`
[bans]
multiple-versions = "warn"
deny = [
    { crate = "winapi" },
    { name = "page_size", wrappers = ["criterion", "iai"] },
]
`);
  assertEquals(entries, [
    { name: "winapi", wrappers: [] },
    { name: "page_size", wrappers: ["criterion", "iai"] },
  ]);
  assertEquals(parseBanDenyEntries('[bans]\nmultiple-versions = "warn"\n'), []);
});

Deno.test("sectionsDeclaring separates a shipped dependency from a dev one", () => {
  const manifest = `
[dependencies]
serde = { version = "1", features = ["derive"] }

[target.'cfg(not(target_family = "wasm"))'.dependencies]
criterion = "0.8"

[dev-dependencies]
criterion = { version = "0.8", features = ["html_reports"] }
`;
  assertEquals(sectionsDeclaring(manifest, "criterion"), [
    `target.'cfg(not(target_family = "wasm"))'.dependencies`,
    "dev-dependencies",
  ]);
  assertEquals(sectionsDeclaring(manifest, "serde"), ["dependencies"]);
  assertEquals(sectionsDeclaring(manifest, "tempfile"), []);
});
