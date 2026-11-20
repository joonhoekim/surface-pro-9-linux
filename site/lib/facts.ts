// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Joonhoe Kim
// Facts for the landing page, read from the repository at build time (never copied by hand):
//
//   README.md               the "Hardware | Verdict | ..." table      -> hardware verdicts
//   docs/test-results.md    the "ID | Result | ..." table, last row   -> test status per area
//   docs/test-plan.md       "## Area (PREFIX)" headings, "ID | ..."   -> area names, defined tests
//   kernel/PROVENANCE.md    the "Base" and "Patches" tables           -> kernel base, patch origins
//   kernel/patches/         the patch files                           -> patch count
//
// Every reader throws when the table it needs is missing or has a value it does not know: a
// build that stops is better than a landing page with wrong numbers.
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { unified } from "unified";
import remarkParse from "remark-parse";
import remarkGfm from "remark-gfm";
import { toString } from "mdast-util-to-string";
import type { Root, Table, TableRow } from "mdast";
import { REPO_ROOT } from "./config";

class FactsError extends Error {
  constructor(file: string, what: string) {
    super(`site/lib/facts.ts: ${file}: ${what}. The landing page reads this file; adapt the reader in site/lib/facts.ts if the document changed shape.`);
    this.name = "FactsError";
  }
}

function parse(file: string): Root {
  let text: string;
  try {
    text = fs.readFileSync(path.join(REPO_ROOT, file), "utf8");
  } catch {
    throw new FactsError(file, "cannot be read");
  }
  return unified().use(remarkParse).use(remarkGfm).parse(text) as Root;
}

const cells = (row: TableRow) => row.children.map((c) => toString(c).replace(/\s+/g, " ").trim());
const header = (t: Table) => cells(t.children[0]).map((h) => h.toLowerCase());

/** Tables of a document, each with the text of the nearest heading above it. */
function tables(tree: Root): { table: Table; heading: string }[] {
  const out: { table: Table; heading: string }[] = [];
  let heading = "";
  for (const node of tree.children) {
    if (node.type === "heading") heading = toString(node).trim();
    else if (node.type === "table") out.push({ table: node, heading });
  }
  return out;
}

/** Tables whose header has all of `columns` (lower case), with the index of each. */
function tablesWith(tree: Root, columns: string[]): { table: Table; heading: string; idx: number[] }[] {
  return tables(tree).flatMap(({ table, heading }) => {
    const head = header(table);
    const idx = columns.map((c) => head.indexOf(c));
    return idx.every((i) => i >= 0) ? [{ table, heading, idx }] : [];
  });
}

// ---------------------------------------------------------------- hardware verdicts (README.md)

export type Verdict = "works" | "caveat" | "partial" | "unverified" | "broken";

export const VERDICT_LABEL: Record<Verdict, string> = {
  works: "works",
  caveat: "works, with open items",
  partial: "partial",
  unverified: "unverified",
  broken: "does not work",
};

export interface HardwareRow {
  name: string;
  /** the README's own words, e.g. "Works, focus unverified" */
  verdict: string;
  kind: Verdict;
}

function classifyVerdict(text: string): Verdict | null {
  const t = text.toLowerCase().trim();
  if (/^(does not work|not working|broken|fails?\b|no driver)/.test(t)) return "broken";
  if (/^unverified\b/.test(t)) return "unverified";
  if (/^partial(ly)?\b/.test(t)) return "partial";
  // "Works" and "Works (reading)" are plain; anything added after it is an open item
  if (/^works\b/.test(t)) return /^works(\s*\([^)]*\))?[.\s]*$/.test(t) ? "works" : "caveat";
  return null;
}

export function getHardware(): HardwareRow[] {
  const file = "README.md";
  const found = tablesWith(parse(file), ["hardware", "verdict"]);
  if (found.length !== 1) {
    throw new FactsError(file, `expected one table with the columns "Hardware" and "Verdict", found ${found.length}`);
  }
  const [{ table, idx }] = found;
  const rows = table.children.slice(1).map((r) => {
    const c = cells(r);
    const name = c[idx[0]] ?? "";
    const verdict = c[idx[1]] ?? "";
    const kind = classifyVerdict(verdict);
    if (!name || !kind) {
      throw new FactsError(
        file,
        `hardware row "${name}" has the verdict "${verdict}", which does not start with Works, Partial, Unverified or Does not work`,
      );
    }
    return { name, verdict, kind };
  });
  if (rows.length < 5) throw new FactsError(file, `the hardware table has only ${rows.length} rows`);
  return rows;
}

// ---------------------------------------------------------------- tests (docs/test-plan.md, docs/test-results.md)

export type TestResult = "pass" | "partial" | "fail" | "unverified" | "norecord";

export const RESULT_LABEL: Record<TestResult, string> = {
  pass: "pass",
  partial: "partial",
  fail: "fail",
  unverified: "unverified",
  norecord: "no record",
};

export const RESULT_ORDER: TestResult[] = ["pass", "partial", "fail", "unverified", "norecord"];

export interface TestArea {
  prefix: string;
  title: string;
  counts: Record<TestResult, number>;
  total: number;
}

export interface TestSummary {
  areas: TestArea[];
  totals: Record<TestResult, number>;
  total: number;
  /** date of the last commit that changed docs/test-results.md, when git knows it */
  updated: string | null;
}

const TEST_ID = /^([A-Z][A-Z0-9]*)-(\d+)$/;
const zero = (): Record<TestResult, number> => ({ pass: 0, partial: 0, fail: 0, unverified: 0, norecord: 0 });

// the date of the last commit that changed FILE (the workflow checks out the full history)
function lastCommitDate(file: string): string | null {
  try {
    const d = execFileSync("git", ["log", "-1", "--format=%cs", "--", file], {
      cwd: REPO_ROOT,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
    }).trim();
    return /^\d{4}-\d{2}-\d{2}$/.test(d) ? d : null;
  } catch {
    return null;
  }
}

export function getTests(): TestSummary {
  const planFile = "docs/test-plan.md";
  const resultsFile = "docs/test-results.md";

  // The plan: which IDs exist, and the name of each area ("## Pen (PEN)").
  const plan = parse(planFile);
  const titles = new Map<string, string>();
  for (const node of plan.children) {
    if (node.type !== "heading") continue;
    const m = /^(.+?)\s*\(([A-Z][A-Z0-9]*)\)$/.exec(toString(node).trim());
    if (m) titles.set(m[2], m[1]);
  }
  const planned: string[] = [];
  for (const { table, idx } of tablesWith(plan, ["id"])) {
    for (const r of table.children.slice(1)) {
      const id = cells(r)[idx[0]] ?? "";
      if (TEST_ID.test(id) && !planned.includes(id)) planned.push(id);
    }
  }
  if (planned.length === 0) throw new FactsError(planFile, 'no table with an "ID" column that holds test IDs such as PEN-1');

  // The results: one row per run; the last row of an ID is its state.
  const found = tablesWith(parse(resultsFile), ["id", "result"]);
  if (found.length === 0) throw new FactsError(resultsFile, 'no table with the columns "ID" and "Result"');
  const state = new Map<string, TestResult>();
  for (const { table, idx } of found) {
    for (const r of table.children.slice(1)) {
      const c = cells(r);
      const id = c[idx[0]] ?? "";
      if (!TEST_ID.test(id)) throw new FactsError(resultsFile, `"${id}" in the ID column is not a test ID`);
      const word = (c[idx[1]] ?? "").toLowerCase().match(/^[a-z]+/)?.[0] ?? "";
      if (word !== "pass" && word !== "partial" && word !== "fail" && word !== "unverified") {
        throw new FactsError(resultsFile, `${id} has the result "${c[idx[1]]}", not pass, partial, fail or unverified`);
      }
      state.set(id, word);
    }
  }
  const unknown = [...state.keys()].filter((id) => !planned.includes(id));
  if (unknown.length) throw new FactsError(resultsFile, `results for tests that ${planFile} does not define: ${unknown.join(", ")}`);

  const areas: TestArea[] = [];
  for (const id of planned) {
    const prefix = TEST_ID.exec(id)![1];
    let area = areas.find((a) => a.prefix === prefix);
    if (!area) areas.push((area = { prefix, title: titles.get(prefix) ?? prefix, counts: zero(), total: 0 }));
    area.counts[state.get(id) ?? "norecord"]++;
    area.total++;
  }
  const totals = zero();
  for (const a of areas) for (const k of RESULT_ORDER) totals[k] += a.counts[k];
  return { areas, totals, total: planned.length, updated: lastCommitDate(resultsFile) };
}

// ---------------------------------------------------------------- kernel (kernel/PROVENANCE.md, kernel/patches/)

export interface KernelFacts {
  /** "7.3.4" */
  base: string;
  /** "7.3": the linux-surface series */
  series: string;
  patches: number;
  /** patches by origin, when PROVENANCE.md lists every patch file; null while it is behind */
  origins: { upstream: number; pending: number; local: number } | null;
}

export function getKernel(): KernelFacts {
  const file = "kernel/PROVENANCE.md";
  const tree = parse(file);

  let base = "";
  for (const { table } of tables(tree)) {
    for (const r of table.children) {
      const c = cells(r);
      if (/^upstream$/i.test(c[0] ?? "")) base = /\bv(\d+\.\d+(?:\.\d+)?)\b/.exec(c[1] ?? "")?.[1] ?? "";
    }
  }
  if (!base) throw new FactsError(file, 'no table row "Upstream | ... vX.Y.Z ..." that names the base version');

  let patches: number;
  try {
    patches = fs.readdirSync(path.join(REPO_ROOT, "kernel/patches")).filter((n) => /^\d{4}-.*\.patch$/.test(n)).length;
  } catch {
    throw new FactsError("kernel/patches/", "cannot be read");
  }
  if (patches === 0) throw new FactsError("kernel/patches/", "holds no NNNN-*.patch files");

  const found = tablesWith(tree, ["file", "status"]);
  if (found.length !== 1) throw new FactsError(file, `expected one table with the columns "File" and "Status", found ${found.length}`);
  const [{ table, idx }] = found;
  const origins = { upstream: 0, pending: 0, local: 0 };
  let listed = 0;
  for (const r of table.children.slice(1)) {
    const c = cells(r);
    if (!/^\d{4}$/.test(c[idx[0]] ?? "")) throw new FactsError(file, `"${c[idx[0]]}" in the File column is not a patch number`);
    const status = (c[idx[1]] ?? "").toLowerCase();
    const kind = (["upstream", "pending", "local"] as const).find((k) => status.startsWith(k));
    if (!kind) throw new FactsError(file, `patch ${c[idx[0]]} has the status "${c[idx[1]]}", not upstream, pending or local`);
    origins[kind]++;
    listed++;
  }
  if (listed !== patches) {
    console.warn(`site: kernel/PROVENANCE.md lists ${listed} patches, kernel/patches/ holds ${patches}: the landing page leaves out the origins`);
  }
  return { base, series: base.split(".").slice(0, 2).join("."), patches, origins: listed === patches ? origins : null };
}
