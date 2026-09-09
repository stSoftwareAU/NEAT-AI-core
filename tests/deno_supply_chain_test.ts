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
// "What" tests: each one reads the committed artefacts and drives a real
// `deno check` subprocess, asserting on the exit status and the diagnostic
// Deno actually reports.
//
// Run: deno test --allow-read --allow-write --allow-run=deno tests/deno_supply_chain_test.ts

import { assert, assertEquals, assertStringIncludes } from "@std/assert";

const DENO_JSON = new URL("../deno.json", import.meta.url);
const DENO_LOCK = new URL("../deno.lock", import.meta.url);

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
    delete lock.specifiers?.["jsr:@std/assert@1"];
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
