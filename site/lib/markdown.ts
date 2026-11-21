// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Joonhoe Kim
// Markdown -> HTML at build time: GFM (tables, footnotes, task lists), GitHub-style
// heading ids, Shiki highlighting (light + dark), and repository-aware links.
import fs from "node:fs";
import path from "node:path";
import { unified } from "unified";
import remarkParse from "remark-parse";
import remarkGfm from "remark-gfm";
import remarkRehype from "remark-rehype";
import rehypeRaw from "rehype-raw";
import rehypeSlug from "rehype-slug";
import rehypeShiki from "@shikijs/rehype";
import rehypeStringify from "rehype-stringify";
import { visit, SKIP } from "unist-util-visit";
import { toString as hastToString } from "hast-util-to-string";
import { toString as mdastToString } from "mdast-util-to-string";
import type { Root as MdRoot } from "mdast";
import type { Root as HRoot, Element } from "hast";
import GithubSlugger from "github-slugger";
import { REPO_ROOT, githubUrl, withBase } from "./config";
import { findDocByFile, readDocSource, requireRoute } from "./docs";

export interface TocItem {
  depth: 2 | 3;
  id: string;
  text: string;
}

export interface Rendered {
  title: string;
  html: string;
  toc: TocItem[];
}

const SCHEME = /^[a-z][a-z0-9+.-]*:/i;

function statKind(repoPath: string): "file" | "dir" | null {
  try {
    const st = fs.statSync(path.join(REPO_ROOT, repoPath));
    return st.isDirectory() ? "dir" : "file";
  } catch {
    return null;
  }
}

/** Resolve a link target written in `fromFile` to a normalized repository path (or null if outside). */
function toRepoPath(target: string, fromFile: string): string | null {
  let p: string;
  try {
    p = decodeURIComponent(target);
  } catch {
    p = target;
  }
  const joined = p.startsWith("/") ? p.slice(1) : path.posix.join(path.posix.dirname(fromFile), p);
  const norm = path.posix.normalize(joined).replace(/\/+$/, "");
  if (norm === "." || norm === "") return "";
  if (norm.startsWith("..")) return null;
  return norm;
}

/** Rewrite a link: other rendered docs -> site routes, other repository files -> GitHub. */
export function rewriteLink(url: string, fromFile: string): string {
  if (!url || url.startsWith("#") || url.startsWith("//") || SCHEME.test(url)) return url;
  const m = /^([^?#]*)([?#].*)?$/.exec(url)!;
  const [pathPart, suffix = ""] = [m[1], m[2]];
  if (!pathPart) return url;
  const repoPath = toRepoPath(pathPart, fromFile);
  if (repoPath === null) return url;
  const hash = suffix.startsWith("#") ? suffix : suffix.includes("#") ? suffix.slice(suffix.indexOf("#")) : "";

  const direct = findDocByFile(repoPath);
  if (direct) return withBase(direct.route) + hash;
  const kind = statKind(repoPath);
  if (kind === "dir" || (kind === null && pathPart.endsWith("/"))) {
    const readme = findDocByFile(repoPath ? `${repoPath}/README.md` : "README.md");
    if (readme) return withBase(readme.route) + hash;
    return githubUrl(repoPath, true) + hash;
  }
  return githubUrl(repoPath) + suffix;
}

/** Rewrite an image source: site/public files and repository images copied by scripts/copy-doc-assets.mjs. */
export function rewriteImage(url: string, fromFile: string): string {
  if (!url || url.startsWith("//") || SCHEME.test(url)) return url;
  const repoPath = toRepoPath(url.replace(/[?#].*$/, ""), fromFile);
  if (!repoPath) return url;
  if (repoPath.startsWith("site/public/")) return withBase("/" + repoPath.slice("site/public/".length));
  if (statKind(repoPath) === "file") return withBase("/_repo/" + repoPath);
  return githubUrl(repoPath) + "?raw=true";
}

/** remark: drop the leading H1 (shown as the page title) and rewrite links and images. */
function remarkRepo(opts: { file: string; meta: { title: string } }) {
  return (tree: MdRoot) => {
    const first = tree.children.findIndex((n) => n.type !== "html" && n.type !== "yaml");
    const h = tree.children[first];
    if (h && h.type === "heading" && h.depth === 1) {
      opts.meta.title = mdastToString(h);
      tree.children.splice(first, 1);
    }
    visit(tree, (node) => {
      if (node.type === "link" || node.type === "definition") node.url = rewriteLink(node.url, opts.file);
      else if (node.type === "image") node.url = rewriteImage(node.url, opts.file);
    });
  };
}

function tableColumns(table: Element): number {
  let n = 0;
  visit(table, "element", (el: Element) => {
    if (el.tagName !== "tr") return;
    n = Math.max(n, el.children.filter((c) => c.type === "element" && (c.tagName === "th" || c.tagName === "td")).length);
    return SKIP;
  });
  return n;
}

function longestCell(table: Element): number {
  let n = 0;
  visit(table, "element", (el: Element) => {
    if (el.tagName !== "td") return;
    n = Math.max(n, hastToString(el).length);
    return SKIP;
  });
  return n;
}

/** rehype: images and links inside raw HTML, permalink anchors, scrollable tables, the table of contents. */
function rehypeSite(opts: { file: string; toc: TocItem[] }) {
  return (tree: HRoot) => {
    visit(tree, "element", (node: Element, index, parent) => {
      const p = node.properties;
      if (node.tagName === "img" && typeof p.src === "string" && !p.src.startsWith(withBase("/"))) {
        p.src = rewriteImage(p.src, opts.file);
      }
      if (node.tagName === "img") {
        p.loading = "lazy";
        p.decoding = "async";
      }
      if (node.tagName === "a" && typeof p.href === "string") {
        // remark already rewrote Markdown links; this catches raw HTML <a>
        if (!p.href.startsWith(withBase("/")) && !p.href.startsWith("https://")) p.href = rewriteLink(p.href, opts.file);
      }
      if (/^h[2-4]$/.test(node.tagName) && typeof p.id === "string") {
        const cls = Array.isArray(p.className) ? p.className : [];
        if (cls.includes("sr-only")) return;
        const text = hastToString(node).trim();
        if (node.tagName !== "h4") opts.toc.push({ depth: node.tagName === "h2" ? 2 : 3, id: p.id, text });
        node.children.push({
          type: "element",
          tagName: "a",
          properties: { className: ["anchor"], href: `#${p.id}`, ariaLabel: `Permalink to “${text}”` },
          children: [{ type: "text", value: "#" }],
        });
      }
      if (node.tagName === "table" && parent && typeof index === "number") {
        // Tables with many columns, or with long text in their cells, keep a readable width
        // and scroll sideways on a narrow screen instead of squeezing every column.
        const columns = tableColumns(node);
        const longest = longestCell(node);
        const size = columns >= 5 || longest > 400 ? "table-xwide" : columns >= 4 || longest > 160 ? "table-wide" : "table-fit";
        parent.children[index] = {
          type: "element",
          tagName: "div",
          properties: { className: ["table-wrap", size], tabIndex: 0, role: "region", ariaLabel: "Table (scrolls sideways)" },
          children: [node],
        };
        return SKIP;
      }
    });
  };
}

export async function renderMarkdown(source: string, file: string, fallbackTitle: string): Promise<Rendered> {
  const meta = { title: "" };
  const toc: TocItem[] = [];
  const processed = await unified()
    .use(remarkParse)
    .use(remarkGfm)
    .use(remarkRepo, { file, meta })
    .use(remarkRehype, {
      allowDangerousHtml: true,
      // GFM footnotes: the notes are listed under a visible "Notes" heading at the end of the page, and
      // components/FootnotePopover.tsx shows each one next to its marker as well
      footnoteLabel: "Notes",
      footnoteLabelTagName: "h2",
      footnoteLabelProperties: {},
    })
    .use(rehypeRaw)
    .use(rehypeSlug)
    .use(rehypeShiki, {
      themes: { light: "github-light", dark: "github-dark" },
      defaultColor: false,
      defaultLanguage: "text",
      fallbackLanguage: "text",
    })
    .use(rehypeSite, { file, toc })
    .use(rehypeStringify)
    .process(source);
  return { title: meta.title || fallbackTitle, html: String(processed), toc };
}

/**
 * The route of a rendered doc with the fragment of its first heading that matches `heading`,
 * computed the way the page's ids are (GitHub slugs, the leading H1 not counted). Without a
 * matching heading the route alone: a renamed section costs the fragment, not the build.
 */
export function headingLink(file: string, heading: RegExp): string {
  const route = requireRoute(file);
  const tree = unified().use(remarkParse).use(remarkGfm).parse(readDocSource(findDocByFile(file)!)) as MdRoot;
  const slugger = new GithubSlugger();
  const first = tree.children.findIndex((n) => n.type !== "html" && n.type !== "yaml");
  let hit = "";
  visit(tree, "heading", (h, index, parent) => {
    if (parent === tree && index === first && h.depth === 1) return;
    const text = mdastToString(h).trim();
    const id = slugger.slug(text);
    if (!hit && heading.test(text)) hit = id;
  });
  if (!hit) console.warn(`site: ${file} has no heading matching ${heading}; linking to the page without a fragment`);
  return hit ? `${route}#${hit}` : route;
}
