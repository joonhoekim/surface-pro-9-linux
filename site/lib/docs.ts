// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Joonhoe Kim
// The registry of Markdown files the site renders, and their routes.
//
//   README.md                          -> /docs/
//   docs/<path>.md                     -> /docs/<path>/          (every file, picked up automatically)
//   docs/<dir>/README.md               -> /docs/<dir>/
//   <dir>/README.md outside docs/      -> /docs/repo/<dir>/      (every one, picked up automatically)
//   <dir>/<NAME>.md outside docs/      -> /docs/repo/<dir>/<name>/
//   CONTRIBUTING.md, SECURITY.md, ...  -> /docs/repo/<name>/     (ROOT_PAGES below)
//
// Which files exist is asked of git (tracked files and new files that are not ignored), so a
// local checkout never publishes what the repository does not hold. Navigation groups and
// their order are the tables below; a new file needs no change here unless its place matters.
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { REPO_ROOT } from "./config";

export type DocGroup = "project" | "guide" | "status" | "gaps" | "control" | "more" | "source";

export interface DocEntry {
  /** repository-relative POSIX path, e.g. "docs/hardware-status.md" */
  file: string;
  /** route segments under /docs */
  slug: string[];
  /** site route without base path, always with a trailing slash */
  route: string;
  title: string;
  /** shorter label for the navigation, when the title is long */
  navTitle: string;
  group: DocGroup;
  /** file of the doc this one is shown indented under */
  parent?: string;
}

export const GROUP_LABELS: Record<DocGroup, string> = {
  project: "Project",
  guide: "Install and use",
  status: "Status and tests",
  gaps: "Research: Linux against Windows",
  control: "Research: how Windows controls it",
  more: "More documentation",
  source: "In the source tree",
};

/** Files that are not rendered and not in the navigation; links to them go to GitHub. */
const HIDDEN = new Set<string>(["site/README.md"]);

/** Root-level pages besides README.md, in navigation order. */
const ROOT_PAGES = ["CHANGELOG.md", "CONTRIBUTING.md", "SECURITY.md", "TRADEMARKS.md"];

/** Group of a file under docs/: first matching rule; anything else lands in "more". */
const DOCS_GROUPS: [RegExp, DocGroup][] = [
  [/^docs\/research\/gaps\//, "gaps"],
  [/^docs\/research\/control\//, "control"],
  [/^docs\/(install|install-[^/]+|nixos|helper|helper-[^/]+|digitizerd)\.md$/, "guide"],
  [/^docs\/(hardware|hardware-[^/]+|test-[^/]+)\.md$/, "status"],
  [/^docs\/reference\//, "status"],
];

/** Preferred order inside the groups; files not listed follow, sorted by path. */
const ORDER = [
  "README.md",
  ...ROOT_PAGES,
  "docs/install.md",
  "docs/install-prepare.md",
  "docs/install-debian-ubuntu.md",
  "docs/install-arch.md",
  "docs/install-nixos.md",
  "docs/install-from-source.md",
  "docs/nixos.md",
  "docs/helper.md",
  "docs/helper-reference.md",
  "docs/digitizerd.md",
  "docs/hardware-status.md",
  "docs/test-plan.md",
  "docs/test-results.md",
  "docs/hardware.md",
  "docs/reference/README.md",
  "docs/research/gaps/README.md",
  "docs/research/gaps/pen-touch.md",
  "docs/research/gaps/pen-vendor-exchange.md",
  "docs/research/gaps/pen-ble-trace.md",
  "docs/research/gaps/camera.md",
  "docs/research/gaps/audio.md",
  "docs/research/gaps/display-power.md",
  "docs/research/gaps/sam-platform.md",
  "docs/research/control/power-thermal.md",
  "docs/research/control/standby.md",
  "docs/research/control/display.md",
  "docs/research/control/battery-charging.md",
  "docs/research/control/input.md",
  "docs/research/control/typecover.md",
  "docs/research/control/camera.md",
  "kernel/README.md",
  "kernel/PROVENANCE.md",
  "userspace/platform/README.md",
  "helper/crates/sp9-digitizer/README.md",
  "userspace/pen/README.md",
  "userspace/iptsd/README.md",
  "userspace/libcamera/README.md",
  "tools/README.md",
];

/** Shown indented under another doc although the directory layout does not say so. */
const PARENTS: Record<string, string> = {
  "docs/helper-reference.md": "docs/helper.md",
  "docs/install-prepare.md": "docs/install.md",
  "docs/install-debian-ubuntu.md": "docs/install.md",
  "docs/install-arch.md": "docs/install.md",
  "docs/install-nixos.md": "docs/install.md",
  "docs/install-from-source.md": "docs/install.md",
};

/** Directories never searched for Markdown when git cannot be asked. */
const WALK_SKIP = new Set(["node_modules", "site", "target", "out", "external", "private", "hw", "LICENSES"]);

function listMarkdown(): string[] {
  try {
    const out = execFileSync("git", ["ls-files", "-z", "--cached", "--others", "--exclude-standard", "--", "*.md"], {
      cwd: REPO_ROOT,
      encoding: "utf8",
      maxBuffer: 16 * 1024 * 1024,
      stdio: ["ignore", "pipe", "ignore"],
    });
    const files = out.split("\0").filter((f) => f && fs.existsSync(path.join(REPO_ROOT, f)));
    if (files.length) return files;
  } catch {
    /* not a git checkout: walk the tree */
  }
  const found: string[] = [];
  const walk = (dir: string, depth: number) => {
    if (depth > 8) return;
    for (const e of fs.readdirSync(path.join(REPO_ROOT, dir), { withFileTypes: true })) {
      if (e.name.startsWith(".") || WALK_SKIP.has(e.name) || e.name.startsWith("result")) continue;
      const rel = dir ? `${dir}/${e.name}` : e.name;
      if (e.isDirectory()) walk(rel, depth + 1);
      else if (e.name.toLowerCase().endsWith(".md")) found.push(rel);
    }
  };
  walk("", 0);
  return found;
}

function readTitle(file: string, fallback: string): string {
  const text = fs.readFileSync(path.join(REPO_ROOT, file), "utf8");
  const m = /^#[ \t]+(.+?)[ \t#]*$/m.exec(text);
  return m ? m[1].replace(/[`*_]/g, "").trim() : fallback;
}

/** Navigation labels where the rule in navTitle() says too little. */
const NAV_TITLES: Record<string, string> = {
  "docs/research/gaps/pen-vendor-exchange.md": "Slim Pen 2: haptics, coupling, reconnection",
  "docs/nixos.md": "NixOS module",
};

/** A long "Subject: what the page says about it" title is shortened to its subject. */
function navTitle(file: string, title: string): string {
  if (NAV_TITLES[file]) return NAV_TITLES[file];
  if (title.length <= 40) return title;
  const head = /^(.{3,40}?)(?::\s| — | - )/.exec(title);
  return head ? head[1] : title;
}

function routeOf(slug: string[]): string {
  return slug.length ? `/docs/${slug.join("/")}/` : "/docs/";
}

function place(file: string): { slug: string[]; group: DocGroup } | null {
  if (HIDDEN.has(file) || file.startsWith("site/") || file.startsWith(".")) return null;
  const dir = path.posix.dirname(file);
  const base = path.posix.basename(file, path.posix.extname(file));
  const tail = base.toLowerCase() === "readme" ? [] : [base.toLowerCase()];
  if (dir === ".") {
    if (file === "README.md") return { slug: [], group: "project" };
    return ROOT_PAGES.includes(file) ? { slug: ["repo", ...tail], group: "project" } : null;
  }
  if (file.startsWith("docs/")) {
    const sub = dir === "docs" ? [] : dir.slice("docs/".length).split("/");
    const slug = [...sub, ...tail];
    // docs/README.md would be "/docs/", which is the repository README; "repo" is the source tree's prefix
    if (slug.length === 0 || slug[0] === "repo") {
      throw new Error(`site: ${file} has no route of its own (/docs/ and /docs/repo/ are taken); rename it or hide it in site/lib/docs.ts`);
    }
    return { slug, group: DOCS_GROUPS.find(([re]) => re.test(file))?.[1] ?? "more" };
  }
  return { slug: ["repo", ...dir.split("/"), ...tail], group: "source" };
}

let cache: DocEntry[] | null = null;

/** All rendered docs, in navigation order. Re-read on every call in development. */
export function getDocs(): DocEntry[] {
  if (cache && process.env.NODE_ENV === "production") return cache;
  const entries: DocEntry[] = [];
  for (const file of listMarkdown()) {
    const p = place(file);
    if (!p) continue;
    const dir = path.posix.dirname(file);
    const fallback = path.posix.basename(file) === "README.md" ? dir : path.posix.basename(file, ".md");
    const title = readTitle(file, fallback);
    entries.push({ file, slug: p.slug, route: routeOf(p.slug), title, navTitle: navTitle(file, title), group: p.group });
  }
  if (!entries.some((e) => e.file === "README.md")) {
    throw new Error(`site: no README.md in ${REPO_ROOT} (REPO_ROOT must be the repository checkout)`);
  }

  const routes = new Map<string, string>();
  for (const e of entries) {
    const other = routes.get(e.route);
    if (other) throw new Error(`site: ${other} and ${e.file} both map to ${e.route}`);
    routes.set(e.route, e.file);
  }

  // Indent a file under the README of its directory, or of the nearest directory above that has
  // one in the same group. The top README of a group is nobody's parent by this rule, except
  // where the group is its directory (the research groups).
  const byFile = new Map(entries.map((e) => [e.file, e]));
  for (const e of entries) {
    if (PARENTS[e.file] && byFile.has(PARENTS[e.file])) {
      e.parent = PARENTS[e.file];
      continue;
    }
    if (e.group === "project") continue;
    let dir = path.posix.dirname(e.file);
    if (path.posix.basename(e.file) === "README.md") dir = path.posix.dirname(dir);
    for (; dir !== "." && dir !== "docs" && dir !== "docs/research"; dir = path.posix.dirname(dir)) {
      const readme = byFile.get(`${dir}/README.md`);
      if (readme && readme.group === e.group && readme !== e) {
        e.parent = readme.parent ?? readme.file;
        break;
      }
    }
  }

  const groups = Object.keys(GROUP_LABELS) as DocGroup[];
  const rank = (f: string) => {
    const i = ORDER.indexOf(f);
    return i === -1 ? ORDER.length : i;
  };
  // a child sorts right after its parent
  const key = (e: DocEntry): [number, number, string, number, number, string] => {
    const top = e.parent ? byFile.get(e.parent)! : e;
    return [groups.indexOf(e.group), rank(top.file), top.file, e.parent ? 1 : 0, rank(e.file), e.file];
  };
  entries.sort((a, b) => {
    const ka = key(a);
    const kb = key(b);
    for (let i = 0; i < ka.length; i++) {
      if (ka[i] < kb[i]) return -1;
      if (ka[i] > kb[i]) return 1;
    }
    return 0;
  });

  cache = entries;
  return entries;
}

export function findDocBySlug(slug: string[]): DocEntry | undefined {
  const key = slug.join("/");
  return getDocs().find((d) => d.slug.join("/") === key);
}

export function findDocByFile(file: string): DocEntry | undefined {
  return getDocs().find((d) => d.file === file);
}

/** The route of a file the site's own pages link to; the build stops when the file is gone. */
export function requireRoute(file: string): string {
  const d = findDocByFile(file);
  if (!d) throw new Error(`site: ${file} is linked from the site's own pages but is not in the repository (moved or renamed?)`);
  return d.route;
}

export function readDocSource(entry: DocEntry): string {
  return fs.readFileSync(path.join(REPO_ROOT, entry.file), "utf8");
}

/** The docs before and after this one in navigation order. */
export function neighbours(entry: DocEntry): { prev?: DocEntry; next?: DocEntry } {
  const docs = getDocs();
  const i = docs.findIndex((d) => d.file === entry.file);
  return { prev: i > 0 ? docs[i - 1] : undefined, next: i >= 0 && i < docs.length - 1 ? docs[i + 1] : undefined };
}
