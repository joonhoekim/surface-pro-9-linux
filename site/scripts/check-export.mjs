// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Joonhoe Kim
// Check the static export in out/ after `next build`:
//
//   - every Markdown file the site should render has a page, and the page has text;
//   - every link and image that points into the site resolves to a file of the export, and
//     its #fragment to an element id of that page;
//   - every link to a file or directory of the repository on GitHub names a path that exists
//     in this checkout (a moved or misspelled relative link in the Markdown ends up here).
//
// Links to other sites are counted, not fetched. Exit status 1 when anything is wrong.
//   PAGES_BASE_PATH, REPO_URL, REPO_BRANCH, REPO_ROOT: as for the build.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const site = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const repo = process.env.REPO_ROOT ? path.resolve(process.env.REPO_ROOT) : path.resolve(site, "..");
const out = path.join(site, "out");
const base = (process.env.PAGES_BASE_PATH ?? "").replace(/\/+$/, "");
const repoUrl = (process.env.REPO_URL ?? "https://github.com/joonhoekim/surface-pro-9-linux").replace(/\/+$/, "");
const branch = process.env.REPO_BRANCH ?? "main";

if (!fs.existsSync(path.join(out, "index.html"))) {
  console.error("check-export: no out/index.html (run `pnpm run build` first)");
  process.exit(1);
}

const errors = [];
const err = (page, msg) => errors.push(`${page}: ${msg}`);

function* htmlFiles(dir) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) yield* htmlFiles(p);
    else if (e.name.endsWith(".html")) yield p;
  }
}

const unescape = (s) =>
  s
    .replace(/&#x([0-9a-f]+);/gi, (_, h) => String.fromCodePoint(parseInt(h, 16)))
    .replace(/&#(\d+);/g, (_, d) => String.fromCodePoint(Number(d)))
    .replace(/&quot;/g, '"')
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&amp;/g, "&");

/** The contents of the <div> that starts with `open` (nested <div>s counted), or null. */
function innerOf(html, open) {
  const start = html.indexOf(open);
  if (start < 0) return null;
  const tag = /<(\/?)div\b[^>]*>/g;
  tag.lastIndex = start + open.length;
  for (let depth = 1, m; (m = tag.exec(html)); ) {
    depth += m[1] ? -1 : 1;
    if (depth === 0) return html.slice(start + open.length, m.index);
  }
  return null;
}

// page (path relative to out/) -> { ids, links, images, proseText }
const pages = new Map();
for (const file of htmlFiles(out)) {
  const rel = path.relative(out, file).split(path.sep).join("/");
  const html = fs.readFileSync(file, "utf8").replace(/<script\b[\s\S]*?<\/script>/gi, "").replace(/<style\b[\s\S]*?<\/style>/gi, "");
  const ids = new Set([...html.matchAll(/\sid="([^"]*)"/g)].map((m) => unescape(m[1])));
  const links = [...html.matchAll(/<a\b[^>]*?\shref="([^"]*)"/g)].map((m) => unescape(m[1]));
  const images = [...html.matchAll(/<img\b[^>]*?\ssrc="([^"]*)"/g)].map((m) => unescape(m[1]));
  const prose = innerOf(html, '<div class="prose">');
  const proseText = prose === null ? null : unescape(prose.replace(/<[^>]+>/g, " ")).replace(/\s+/g, " ").trim();
  pages.set(rel, { ids, links, images, proseText });
}

/** The file of the export a site path ("/docs/x/", without the base path) is served from. */
function target(sitePath) {
  let p;
  try {
    p = decodeURIComponent(sitePath);
  } catch {
    return null;
  }
  p = p.replace(/^\/+/, "");
  const candidates = p === "" || p.endsWith("/") ? [p + "index.html"] : [p, p + "/index.html", p + ".html"];
  return candidates.find((c) => fs.existsSync(path.join(out, c)) && fs.statSync(path.join(out, c)).isFile()) ?? null;
}

let internal = 0;
let github = 0;
let external = 0;
const ghPrefix = new RegExp(`^${repoUrl.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}/(blob|tree)/${branch.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}/([^?#]*)`);

for (const [page, info] of pages) {
  for (const href of info.links) {
    if (href === "") {
      err(page, "empty href");
    } else if (href.startsWith("#")) {
      internal++;
      const id = decodeURIComponent(href.slice(1));
      if (id && !info.ids.has(id)) err(page, `no element with the id of ${href}`);
    } else if (ghPrefix.test(href)) {
      github++;
      const [, kind, p] = ghPrefix.exec(href);
      const abs = path.join(repo, decodeURIComponent(p));
      if (!fs.existsSync(abs)) err(page, `link to ${p}, which is not in the repository`);
      else if ((kind === "tree") !== fs.statSync(abs).isDirectory()) err(page, `${p}: linked as a ${kind === "tree" ? "directory" : "file"}, is not one`);
    } else if (/^[a-z][a-z0-9+.-]*:/i.test(href) || href.startsWith("//")) {
      external++;
    } else if (href.startsWith(base + "/")) {
      internal++;
      const [p, frag] = href.slice(base.length).split("#");
      const t = target(p.replace(/\?.*$/, ""));
      if (!t) err(page, `link to ${href}, which is not in the export`);
      else if (frag && t.endsWith(".html") && !pages.get(t).ids.has(decodeURIComponent(frag))) err(page, `${href}: the page has no element with that id`);
    } else {
      err(page, `link "${href}" is neither absolute, nor under the base path "${base}/", nor a fragment`);
    }
  }
  for (const src of info.images) {
    if (/^[a-z][a-z0-9+.-]*:/i.test(src) || src.startsWith("//")) continue;
    if (!src.startsWith(base + "/") || !target(src.slice(base.length).replace(/[?#].*$/, ""))) err(page, `image ${src} is not in the export`);
  }
}

// Every Markdown file that should be a page is one, with content.
const expected = fs.readFileSync(path.join(out, "pages.txt"), "utf8").split("\n").filter(Boolean);
let docs = 0;
for (const line of expected) {
  const [route, file] = line.split("\t");
  const rel = route.replace(/^\/+/, "") + "index.html";
  const info = pages.get(rel);
  docs++;
  if (!info) err(rel, `no page for ${file}`);
  else if (info.proseText === null) err(rel, `the page for ${file} has no rendered Markdown`);
  else {
    const source = fs.readFileSync(path.join(repo, file), "utf8").trim().length;
    // the rendered text of a document is at least a third of its source (markup takes the rest)
    if (info.proseText.length < Math.min(200, source / 3)) err(rel, `the page for ${file} is nearly empty (${info.proseText.length} characters from ${source})`);
  }
}
const landing = pages.get("index.html");
if (!landing.ids.has("hw-title") || !landing.ids.has("tests-title")) err("index.html", "the hardware or the test section is missing");

console.log(`check-export: ${pages.size} pages (${docs} documents), ${internal} internal links, ${github} links to repository files, ${external} links to other sites`);
if (errors.length) {
  for (const e of errors) console.error(`  ${e}`);
  console.error(`check-export: ${errors.length} problem(s)`);
  process.exit(1);
}
console.log("check-export: clean");
