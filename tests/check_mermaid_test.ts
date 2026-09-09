// Tests for the repo-owned Mermaid gate (Issue #379).
//
// "What" tests (AGENTS.md): every case calls the real validator with real
// Markdown text and asserts on observable outcomes — the findings returned,
// their messages, and the exit status of a whole-tree scan. No source greps.
//
// Run: deno test --allow-read tests/check_mermaid_test.ts

import { assert, assertEquals, assertStringIncludes } from "@std/assert";
import {
  checkMarkdown,
  checkTree,
  extractMermaidBlocks,
  formatFinding,
} from "../scripts/check_mermaid.ts";

const REPO_ROOT = new URL("..", import.meta.url).pathname;

Deno.test("extractMermaidBlocks records the fence line and diagram type", () => {
  const source = [
    "# Title",
    "",
    "```mermaid",
    "flowchart LR",
    "    A --> B",
    "```",
    "",
  ].join("\n");

  const blocks = extractMermaidBlocks("doc.md", source);

  assertEquals(blocks.length, 1);
  assertEquals(blocks[0].fenceLine, 3);
  assertEquals(blocks[0].diagramType, "flowchart");
  assertEquals(blocks[0].lines, ["flowchart LR", "    A --> B"]);
});

Deno.test("extractMermaidBlocks ignores mermaid fences nested inside a wider fence", () => {
  const source = [
    "Example of how to write one:",
    "",
    "````markdown",
    "```mermaid",
    "flowchart LR",
    "    A --> B;;",
    "```",
    "````",
  ].join("\n");

  assertEquals(extractMermaidBlocks("doc.md", source), []);
});

Deno.test("checkMarkdown flags an unescaped ';' in sequenceDiagram message text", () => {
  const source = [
    "```mermaid",
    "sequenceDiagram",
    "    participant GA as GitHub Actions",
    "    Note over GA: 1 concurrent run; publishers keep every run",
    "```",
  ].join("\n");

  const findings = checkMarkdown("doc.md", source);

  assertEquals(findings.length, 1);
  assertEquals(findings[0].diagramType, "sequenceDiagram");
  assertEquals(findings[0].fenceLine, 1);
  assertEquals(findings[0].blockLine, 3);
  assertStringIncludes(findings[0].message, "unescaped ';'");
  assertStringIncludes(
    findings[0].offendingLine,
    "Note over GA: 1 concurrent run; publishers keep every run",
  );
});

Deno.test("checkMarkdown accepts the same sequenceDiagram note once the ';' is replaced", () => {
  const source = [
    "```mermaid",
    "sequenceDiagram",
    "    participant GA as GitHub Actions",
    "    Note over GA: 1 concurrent run — publishers keep every run",
    "```",
  ].join("\n");

  assertEquals(checkMarkdown("doc.md", source), []);
});

Deno.test("checkMarkdown treats HTML entities as escaped, not as statement separators", () => {
  const source = [
    "```mermaid",
    "sequenceDiagram",
    "    Dev->>GA: push tag v&lt;version&gt;",
    "```",
  ].join("\n");

  assertEquals(checkMarkdown("doc.md", source), []);
});

Deno.test("checkMarkdown leaves ';' inside non-sequence diagrams alone", () => {
  // Mermaid parses ';' inside a quoted flowchart label as literal text.
  const source = [
    "```mermaid",
    "flowchart LR",
    '    A["reads env; writes nothing"] --> B',
    "```",
  ].join("\n");

  assertEquals(checkMarkdown("doc.md", source), []);
});

Deno.test("checkMarkdown reports every offending line in a block", () => {
  const source = [
    "```mermaid",
    "sequenceDiagram",
    "    A->>B: first; broken",
    "    B->>A: second; broken",
    "```",
  ].join("\n");

  const findings = checkMarkdown("doc.md", source);

  assertEquals(findings.map((f) => f.blockLine), [2, 3]);
});

Deno.test("checkMarkdown rejects an empty mermaid block", () => {
  const findings = checkMarkdown(
    "doc.md",
    ["```mermaid", "", "```"].join("\n"),
  );

  assertEquals(findings.length, 1);
  assertStringIncludes(findings[0].message, "empty");
});

Deno.test("checkMarkdown rejects an unknown diagram type", () => {
  const findings = checkMarkdown(
    "doc.md",
    ["```mermaid", "flowchat LR", "    A --> B", "```"].join("\n"),
  );

  assertEquals(findings.length, 1);
  assertStringIncludes(findings[0].message, "unknown Mermaid diagram type");
});

Deno.test("checkMarkdown fails loud on a mermaid fence that is never closed", () => {
  const findings = checkMarkdown(
    "doc.md",
    ["```mermaid", "flowchart LR", "    A --> B"].join("\n"),
  );

  assertEquals(findings.length, 1);
  assertStringIncludes(findings[0].message, "never closed");
});

Deno.test("checkMarkdown accepts every diagram type used in this repository", () => {
  for (
    const header of [
      "flowchart LR",
      "flowchart TD",
      "flowchart TB",
      "graph TD",
      "sequenceDiagram",
      "classDiagram",
      "stateDiagram-v2",
      "gitGraph",
    ]
  ) {
    const source = ["```mermaid", header, "    A --> B", "```"].join("\n");
    assertEquals(checkMarkdown("doc.md", source), [], header);
  }
});

Deno.test("formatFinding renders a file:line diagnostic naming the diagram type", () => {
  const [finding] = checkMarkdown(
    "docs/x.md",
    ["```mermaid", "sequenceDiagram", "    A->>B: a; b", "```"].join("\n"),
  );

  const text = formatFinding(finding);

  assertStringIncludes(text, "docs/x.md:1");
  assertStringIncludes(text, "(sequenceDiagram)");
  assertStringIncludes(text, "Line 2:");
});

Deno.test("checkTree scans committed Markdown and finds no Mermaid errors", async () => {
  const findings = await checkTree(REPO_ROOT);

  assertEquals(
    findings.map(formatFinding),
    [],
    "committed Markdown must have no Mermaid findings",
  );
});

Deno.test("checkTree surfaces a broken diagram written into the tree", async () => {
  const dir = await Deno.makeTempDir();
  try {
    await Deno.writeTextFile(
      `${dir}/bad.md`,
      ["```mermaid", "sequenceDiagram", "    A->>B: one; two", "```", ""].join(
        "\n",
      ),
    );

    const findings = await checkTree(dir);

    assertEquals(findings.length, 1);
    assert(findings[0].file.endsWith("bad.md"), findings[0].file);
  } finally {
    await Deno.remove(dir, { recursive: true });
  }
});
