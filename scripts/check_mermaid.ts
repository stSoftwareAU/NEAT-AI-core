// check_mermaid.ts — repo-owned Mermaid gate (Issue #379).
//
// Scans committed Markdown for ```mermaid blocks and fails loud on the parse
// errors that GitHub silently renders as "Unable to render rich display":
//
//   * an unescaped ';' in sequenceDiagram message text (Mermaid reads it as a
//     statement separator and truncates the message),
//   * an empty block,
//   * an unrecognised diagram type,
//   * a fence that is never closed.
//
// Scope is deliberately narrow — this is a structural gate, not a full Mermaid
// parser. It exists so the class of failure recorded in Issue #379 cannot land
// again. This repository commits and owns its own gate; there is no shared
// cross-repo Action and no dependency on an external worker module.
//
// Run: deno run --allow-read scripts/check_mermaid.ts [root-dir]

/** One ```mermaid block located in a Markdown file. */
export interface MermaidBlock {
  /** Path of the Markdown file the block came from. */
  file: string;
  /** 1-based line number of the opening ```mermaid fence. */
  fenceLine: number;
  /** First token of the block's first non-blank line (e.g. "flowchart"). */
  diagramType: string;
  /** Block body, fences excluded. */
  lines: string[];
  /** True when the opening fence is never closed by a matching fence. */
  unclosed: boolean;
}

/** A single Mermaid problem found in a block. */
export interface MermaidFinding {
  file: string;
  /** 1-based line number of the opening fence, for `file:line` diagnostics. */
  fenceLine: number;
  diagramType: string;
  /** 1-based line number *within* the block body (0 when block-level). */
  blockLine: number;
  message: string;
  offendingLine: string;
}

/**
 * Diagram keywords Mermaid accepts. Anything outside this list is far more
 * likely a typo (`flowchat`) than a diagram type, so it is reported rather
 * than passed through.
 */
const DIAGRAM_TYPES = new Set([
  "architecture-beta",
  "block-beta",
  "C4Component",
  "C4Container",
  "C4Context",
  "C4Deployment",
  "C4Dynamic",
  "classDiagram",
  "classDiagram-v2",
  "erDiagram",
  "flowchart",
  "flowchart-elk",
  "gantt",
  "gitGraph",
  "graph",
  "journey",
  "kanban",
  "mindmap",
  "packet-beta",
  "pie",
  "quadrantChart",
  "radar-beta",
  "requirementDiagram",
  "sankey-beta",
  "sequenceDiagram",
  "stateDiagram",
  "stateDiagram-v2",
  "timeline",
  "treemap-beta",
  "xychart-beta",
  "zenuml",
]);

const DIRECTORY_SKIP_LIST = new Set([".git", "node_modules", "target"]);

const FENCE_RE = /^(\s*)(`{3,}|~{3,})(.*)$/;

/**
 * A ';' that terminates an HTML entity (`&lt;`, `&amp;`, `&#8212;`) is escaped
 * text, not a statement separator — Mermaid renders it fine.
 */
function hasBareSemicolon(text: string): boolean {
  return text.replace(/&(?:#\d+|#x[0-9a-fA-F]+|\w+);/g, "").includes(";");
}

/** First token of the first non-blank body line, or "" for an empty block. */
function diagramTypeOf(body: string[]): string {
  const header = body.find((line) => line.trim() !== "");
  return header === undefined ? "" : header.trim().split(/[\s{]/)[0];
}

/** Extract every top-level ```mermaid block from a Markdown source. */
export function extractMermaidBlocks(
  file: string,
  source: string,
): MermaidBlock[] {
  const blocks: MermaidBlock[] = [];
  const lines = source.split("\n");

  let openMarker: string | null = null;
  let isMermaid = false;
  let fenceLine = 0;
  let body: string[] = [];

  lines.forEach((line, index) => {
    const match = FENCE_RE.exec(line);
    if (openMarker === null) {
      if (!match) return;
      openMarker = match[2];
      isMermaid = match[3].trim().toLowerCase() === "mermaid";
      fenceLine = index + 1;
      body = [];
      return;
    }
    // Inside a fence: only a bare marker of the same character and at least the
    // same length closes it — that is what keeps a ```mermaid sample nested in
    // a ````markdown block out of scope.
    const closes = match !== null &&
      match[2][0] === openMarker[0] &&
      match[2].length >= openMarker.length &&
      match[3].trim() === "";
    if (!closes) {
      if (isMermaid) body.push(line);
      return;
    }
    if (isMermaid) {
      blocks.push({
        file,
        fenceLine,
        diagramType: diagramTypeOf(body),
        lines: body,
        unclosed: false,
      });
    }
    openMarker = null;
    isMermaid = false;
  });

  if (openMarker !== null && isMermaid) {
    blocks.push({
      file,
      fenceLine,
      diagramType: diagramTypeOf(body),
      lines: body,
      unclosed: true,
    });
  }

  return blocks;
}

/** Validate one block, returning a finding per problem. */
export function validateBlock(block: MermaidBlock): MermaidFinding[] {
  const base = {
    file: block.file,
    fenceLine: block.fenceLine,
    diagramType: block.diagramType,
  };

  if (block.unclosed) {
    return [{
      ...base,
      blockLine: 0,
      message: "mermaid block opened here is never closed by a matching fence.",
      offendingLine: "",
    }];
  }

  if (block.diagramType === "") {
    return [{
      ...base,
      blockLine: 0,
      message: "mermaid block is empty — remove it or add a diagram.",
      offendingLine: "",
    }];
  }

  if (!DIAGRAM_TYPES.has(block.diagramType)) {
    return [{
      ...base,
      blockLine: 1,
      message: `unknown Mermaid diagram type '${block.diagramType}' — the ` +
        "block will not render.",
      offendingLine: block.lines.find((l) => l.trim() !== "")?.trim() ?? "",
    }];
  }

  if (block.diagramType !== "sequenceDiagram") return [];

  const findings: MermaidFinding[] = [];
  block.lines.forEach((line, index) => {
    const colon = line.indexOf(":");
    if (colon === -1) return;
    if (!hasBareSemicolon(line.slice(colon + 1))) return;
    findings.push({
      ...base,
      blockLine: index + 1,
      message:
        "message text contains unescaped ';' which Mermaid parses as a " +
        "statement separator. Replace ';' with ',' or ' — ' (or remove it).",
      offendingLine: line.trim(),
    });
  });
  return findings;
}

/** Validate every Mermaid block in one Markdown source. */
export function checkMarkdown(file: string, source: string): MermaidFinding[] {
  return extractMermaidBlocks(file, source).flatMap(validateBlock);
}

/** Render a finding as a `file:line (type): message` diagnostic. */
export function formatFinding(finding: MermaidFinding): string {
  const where = finding.blockLine > 0 ? `Line ${finding.blockLine}: ` : "";
  const offender = finding.offendingLine === ""
    ? ""
    : ` Offending line: ${finding.offendingLine}`;
  return `[mermaid] ${finding.file}:${finding.fenceLine} ` +
    `(${finding.diagramType || "unknown"}): ` +
    `${where}${finding.message}${offender}`;
}

/** Recursively collect committed Markdown files under `root`. */
export async function collectMarkdownFiles(root: string): Promise<string[]> {
  const found: string[] = [];
  for await (const entry of Deno.readDir(root)) {
    const path = `${root.replace(/\/$/, "")}/${entry.name}`;
    if (entry.isDirectory) {
      if (DIRECTORY_SKIP_LIST.has(entry.name)) continue;
      found.push(...await collectMarkdownFiles(path));
    } else if (entry.isFile && entry.name.toLowerCase().endsWith(".md")) {
      found.push(path);
    }
  }
  return found.sort();
}

/** Scan every Markdown file under `root` and return all findings. */
export async function checkTree(root: string): Promise<MermaidFinding[]> {
  const files = await collectMarkdownFiles(root);
  const findings: MermaidFinding[] = [];
  for (const file of files) {
    findings.push(...checkMarkdown(file, await Deno.readTextFile(file)));
  }
  return findings;
}

if (import.meta.main) {
  const root = Deno.args[0] ?? ".";
  const findings = await checkTree(root);
  for (const finding of findings) console.error(formatFinding(finding));
  if (findings.length > 0) {
    console.error(
      `check-mermaid: FAILED — ${findings.length} Mermaid problem(s) found.`,
    );
    Deno.exit(1);
  }
  console.log("check-mermaid: all Mermaid blocks passed");
}
