/** Shared provenance guard for frozen legacy captures and measurements. */
export const LEGACY_COMPACTION_REVISION =
  "60d6b4e5d3af20afbaaeb3420d588ad673ab8dab";

export async function legacyCompactionSource(checkout: string): Promise<URL> {
  const head = await new Deno.Command("git", {
    args: ["-C", checkout, "rev-parse", "HEAD"],
  }).output();
  if (
    !head.success ||
    new TextDecoder().decode(head.stdout).trim() !== LEGACY_COMPACTION_REVISION
  ) {
    throw new Error(`Legacy checkout must be at ${LEGACY_COMPACTION_REVISION}`);
  }
  const diff = await new Deno.Command("git", {
    args: ["-C", checkout, "status", "--porcelain", "--untracked-files=no"],
  }).output();
  if (!diff.success || diff.stdout.length) {
    throw new Error("Legacy checkout must have no tracked modifications");
  }
  return new URL(`file://${await Deno.realPath(checkout)}/`);
}
