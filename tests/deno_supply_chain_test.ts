// Supply-chain gate for this repository's JSR (Deno) dependencies — Issue #603.
//
// The Cargo half of the tree is quarantined by `bump-deps.sh`
// (VIBE_BUMP_QUARANTINE_HOURS) and pinned by `Cargo.lock`. The Deno half had
// neither: the `.ts` gates imported floating `jsr:@std/assert@1` ranges with no
// `deno.json` and no committed `deno.lock`, so every CI run re-resolved
// whatever JSR served at that instant — no release-age embargo, no integrity
// pin. `deno.json` now carries a 24h `minimumDependencyAge` and a **frozen**
// `deno.lock`, and these tests are what fails if either is removed.
//
// Issue #646 added the third leg: the versions themselves live in `deno.json`'s
// import map, because `deno outdated` — the updater the scheduled
// `deno-outdated.yml` workflow runs — only sees dependencies declared there. A
// source that goes back to an inline `jsr:@std/assert@1` specifier is invisible
// to it, and the weekly update silently bumps nothing.
//
// "What" tests: each one reads the committed artefacts and drives a real
// `deno check` subprocess, asserting on the exit status and the diagnostic
// Deno actually reports.
//
// Run: deno test --allow-read --allow-write --allow-run=deno tests/deno_supply_chain_test.ts

import { assert, assertEquals, assertStringIncludes } from "@std/assert";

const REPO_ROOT = new URL("../", import.meta.url);
const DENO_JSON = new URL("deno.json", REPO_ROOT);
const DENO_LOCK = new URL("deno.lock", REPO_ROOT);

/** Directories holding the `.ts` gates whose dependencies must stay managed. */
const SOURCE_DIRS = ["scripts", "tests"];

/** The fleet-wide external quarantine floor, in hours (VIBE_BUMP_QUARANTINE_HOURS default). */
const QUARANTINE_HOURS = 24;

/** Internal `stSoftwareAU` scopes bump at 0h and are excluded from the floor. */
const INTERNAL_SCOPES = ["jsr:@stsoftware/*", "npm:@stsoftware/*"];

interface DenoConfig {
  lock?: { path?: string; frozen?: boolean };
  minimumDependencyAge?: { age?: string; exclude?: string[] } | string;
}

interface DenoLock {
  version?: string;
  specifiers?: Record<string, string>;
  jsr?: Record<string, { integrity?: string }>;
}

async function readJson<T>(url: URL): Promise<T> {
  // Fail loud: a missing artefact is the very regression this file guards.
  return JSON.parse(await Deno.readTextFile(url)) as T;
}

/**
 * Hours in an ISO-8601 duration, for the subset Deno's `minimumDependencyAge`
 * accepts here: `P<n>D` / `P<n>W` / `PT<n>H` / `PT<n>M` and combinations.
 * `P1D` → 24, `PT36H` → 36, `PT90M` → 1.5.
 */
export function isoDurationHours(duration: string): number {
  const match =
    /^P(?:(\d+)W)?(?:(\d+)D)?(?:T(?:(\d+)H)?(?:(\d+)M)?(?:(\d+)S)?)?$/
      .exec(duration);
  if (!match) throw new Error(`not an ISO-8601 duration: ${duration}`);
  const [, weeks, days, hours, minutes, seconds] = match;
  const n = (
    value: string | undefined,
  ) => (value === undefined ? 0 : Number(value));
  return n(weeks) * 7 * 24 + n(days) * 24 + n(hours) + n(minutes) / 60 +
    n(seconds) / 3600;
}

/** Run `deno check mod.ts` in `cwd`, returning the exit code and stderr. */
async function denoCheck(
  cwd: string,
): Promise<{ code: number; stderr: string }> {
  const { code, stderr } = await new Deno.Command(Deno.execPath(), {
    args: ["check", "mod.ts"],
    cwd,
    stdin: "null",
    stdout: "piped",
    stderr: "piped",
  }).output();
  return { code, stderr: new TextDecoder().decode(stderr) };
}

/**
 * A throwaway workspace carrying the repository's *own* `deno.json` and
 * `deno.lock`, plus a module importing the same JSR specifier the repository's
 * gates import. The config under test is the committed one, not a copy written
 * by the test (oracle rule 4 — one definition, exercised live).
 */
async function fixtureWorkspace(): Promise<string> {
  const dir = await Deno.makeTempDir({ prefix: "neat-core-jsr-" });
  await Deno.copyFile(DENO_JSON, `${dir}/deno.json`);
  await Deno.copyFile(DENO_LOCK, `${dir}/deno.lock`);
  await Deno.writeTextFile(
    `${dir}/mod.ts`,
    'import { assertEquals } from "@std/assert";\nassertEquals(1, 1);\n',
  );
  return dir;
}

Deno.test("deno.json quarantines external JSR and npm releases for at least 24 hours", async () => {
  const config = await readJson<DenoConfig>(DENO_JSON);
  const age = config.minimumDependencyAge;
  assert(
    age !== undefined && typeof age === "object" && typeof age.age === "string",
    "deno.json must declare an object-form minimumDependencyAge",
  );
  const declared = (age as { age: string }).age;
  assert(
    isoDurationHours(declared) >= QUARANTINE_HOURS,
    `minimumDependencyAge ${declared} = ${
      isoDurationHours(declared)
    }h is under ` +
      `the ${QUARANTINE_HOURS}h external floor`,
  );
  // Internal stSoftwareAU deps are exempt from the floor — they bump at 0h.
  for (const scope of INTERNAL_SCOPES) {
    assert(
      (age as { exclude?: string[] }).exclude?.includes(scope),
      `minimumDependencyAge.exclude must carry ${scope}`,
    );
  }
});

Deno.test("deno.json freezes the lockfile so CI cannot re-resolve a floating range", async () => {
  const config = await readJson<DenoConfig>(DENO_JSON);
  assertEquals(config.lock?.path, "deno.lock");
  assertEquals(config.lock?.frozen, true);
});

Deno.test("deno.lock pins every JSR dependency with an integrity hash", async () => {
  const lock = await readJson<DenoLock>(DENO_LOCK);
  const jsr = lock.jsr ?? {};
  const pinned = Object.entries(jsr);
  // Guard against a vacuous pass on an empty lockfile: the repository's gates
  // import @std/assert, so at least that package must be pinned.
  assert(pinned.length > 0, "deno.lock pins no JSR packages");
  assert(
    pinned.some(([name]) => name.startsWith("@std/assert@")),
    `deno.lock does not pin @std/assert: ${pinned.map(([n]) => n).join(", ")}`,
  );
  for (const [name, entry] of pinned) {
    assert(
      /^[0-9a-f]{64}$/.test(entry.integrity ?? ""),
      `${name} has no SHA-256 integrity hash in deno.lock`,
    );
  }
  // Every specifier the sources import must resolve to an exact version.
  for (const [specifier, resolved] of Object.entries(lock.specifiers ?? {})) {
    assert(
      /^\d+\.\d+\.\d+/.test(resolved),
      `${specifier} resolves to a non-exact version: ${resolved}`,
    );
  }
});

Deno.test("a JSR resolution outside the committed lockfile fails the frozen gate", async () => {
  const dir = await fixtureWorkspace();
  try {
    // Positive control — the committed config and lockfile cover this import,
    // so the check passes. Without it a network or cache failure below would
    // masquerade as the quarantine working.
    const pinned = await denoCheck(dir);
    assertEquals(
      pinned.code,
      0,
      `deno check failed against the committed lockfile: ${pinned.stderr}`,
    );

    // Now drop @std/assert from the lockfile copy, exactly as an unpinned
    // re-resolution of the floating `@1` range would look to Deno.
    const lock = JSON.parse(
      await Deno.readTextFile(`${dir}/deno.lock`),
    ) as DenoLock;
    for (const specifier of Object.keys(lock.specifiers ?? {})) {
      if (specifier.startsWith("jsr:@std/assert@")) {
        delete lock.specifiers![specifier];
      }
    }
    for (const name of Object.keys(lock.jsr ?? {})) {
      if (name.startsWith("@std/assert@")) delete lock.jsr![name];
    }
    await Deno.writeTextFile(`${dir}/deno.lock`, JSON.stringify(lock, null, 2));

    const unpinned = await denoCheck(dir);
    assert(
      unpinned.code !== 0,
      "deno check accepted a JSR resolution the lockfile does not pin",
    );
    assertStringIncludes(unpinned.stderr, "lockfile is out of date");
  } finally {
    await Deno.remove(dir, { recursive: true });
  }
});

Deno.test("isoDurationHours converts the durations the quarantine floor uses", () => {
  assertEquals(isoDurationHours("P1D"), 24);
  assertEquals(isoDurationHours("PT36H"), 36);
  assertEquals(isoDurationHours("PT90M"), 1.5);
  assertEquals(isoDurationHours("P1W"), 168);
  assertEquals(isoDurationHours("P1DT12H"), 36);
});

/** Every `.ts` file under `SOURCE_DIRS`, as absolute file URLs. */
async function sourceModules(dir = ""): Promise<URL[]> {
  const roots = dir ? [dir] : SOURCE_DIRS;
  const found: URL[] = [];
  for (const root of roots) {
    const base = new URL(`${root}/`, REPO_ROOT);
    for await (const entry of Deno.readDir(base)) {
      if (entry.isDirectory) {
        found.push(...await sourceModules(`${root}/${entry.name}`));
      } else if (entry.name.endsWith(".ts")) {
        found.push(new URL(entry.name, base));
      }
    }
  }
  return found.sort((a, b) => a.href.localeCompare(b.href));
}

interface InfoModule {
  specifier: string;
  dependencies?: { specifier: string }[];
}

Deno.test("every JSR dependency the sources import is declared in deno.json", async () => {
  const modules = await sourceModules();
  assert(modules.length > 0, "no TypeScript sources found to inspect");

  // One barrel importing every source, so Deno's own resolver reports the whole
  // graph in a single pass. `deno info` resolves; it never executes.
  const dir = await Deno.makeTempDir({ prefix: "neat-core-imports-" });
  try {
    const barrel = `${dir}/all.ts`;
    await Deno.writeTextFile(
      barrel,
      modules.map((url) => `import "${url.href}";`).join("\n") + "\n",
    );
    const { code, stdout, stderr } = await new Deno.Command(Deno.execPath(), {
      // `--no-lock`: the frozen lockfile is the previous test's subject, and
      // it would reject an inline specifier before this one could name it.
      args: [
        "info",
        "--json",
        "--no-lock",
        "--config",
        DENO_JSON.pathname,
        barrel,
      ],
      stdin: "null",
      stdout: "piped",
      stderr: "piped",
    }).output();
    assertEquals(
      code,
      0,
      `deno info failed: ${new TextDecoder().decode(stderr)}`,
    );

    const graph = JSON.parse(new TextDecoder().decode(stdout)) as {
      modules?: InfoModule[];
    };
    const unmanaged: string[] = [];
    for (const module of graph.modules ?? []) {
      // Only this repository's own sources are ours to fix; a third-party
      // package's internal specifiers are its own business.
      if (!module.specifier.startsWith(REPO_ROOT.href)) continue;
      for (const dependency of module.dependencies ?? []) {
        if (/^(jsr|npm):/.test(dependency.specifier)) {
          unmanaged.push(
            `${module.specifier.slice(REPO_ROOT.href.length)} imports ` +
              `${dependency.specifier}`,
          );
        }
      }
    }
    assertEquals(
      unmanaged,
      [],
      "these imports bypass the deno.json import map, so `deno outdated` " +
        `cannot update them:\n  ${unmanaged.join("\n  ")}`,
    );
  } finally {
    await Deno.remove(dir, { recursive: true });
  }
});

Deno.test("deno.json declares the import map deno outdated updates", async () => {
  const config = await readJson<
    DenoConfig & { imports?: Record<string, string> }
  >(DENO_JSON);
  const imports = config.imports ?? {};
  const entries = Object.entries(imports);
  assert(entries.length > 0, "deno.json declares no imports to keep updated");
  for (const [name, specifier] of entries) {
    // An exact pin is what `deno outdated --update --latest` rewrites; a
    // floating range would drift underneath the frozen lockfile instead.
    assert(
      /^(jsr|npm):@?[^@]+@\d+\.\d+\.\d+/.test(specifier),
      `${name} must map to an exact jsr:/npm: version, got ${specifier}`,
    );
  }
});
