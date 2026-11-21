// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Joonhoe Kim
// Build-time configuration. Everything here is read while pages are rendered
// (server components / static export), never in the browser.
import path from "node:path";

/** The repository checkout the site is built from (site/ lives inside it). */
export const REPO_ROOT = process.env.REPO_ROOT
  ? path.resolve(process.env.REPO_ROOT)
  : path.resolve(process.cwd(), "..");

/** "/surface-pro-9-linux" on GitHub Pages, "" locally (see next.config.ts). */
export const BASE_PATH = (process.env.PAGES_BASE_PATH ?? "").replace(/\/+$/, "");

export const REPO_URL = (process.env.REPO_URL ?? "https://github.com/joonhoekim/surface-pro-9-linux").replace(
  /\/+$/,
  "",
);
export const REPO_BRANCH = process.env.REPO_BRANCH ?? "main";

export const SITE_NAME = "surface-pro-9-linux";
export const SITE_TAGLINE = "Linux on the Surface Pro 9 (Intel)";
export const SITE_DESCRIPTION =
  "Linux on the Microsoft Surface Pro 9 with an Intel CPU: kernel patches on top of linux-surface, packages for " +
  "Ubuntu, Debian and Arch Linux, a NixOS module, " +
  "platform files, a device helper, a touch and pen daemon, research notes and test results.";

/** Prefix a site-absolute path ("/docs/") with the base path, for raw HTML and <img>. */
export function withBase(p: string): string {
  return BASE_PATH + p;
}

/** GitHub URL of a repository path (file → blob, directory → tree). */
export function githubUrl(repoPath: string, isDir = false): string {
  const clean = repoPath.replace(/^\/+/, "").replace(/\/+$/, "");
  if (!clean) return REPO_URL;
  return `${REPO_URL}/${isDir ? "tree" : "blob"}/${REPO_BRANCH}/${clean.split("/").map(encodeURIComponent).join("/")}`;
}
