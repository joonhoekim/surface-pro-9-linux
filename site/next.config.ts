// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Joonhoe Kim
import type { NextConfig } from "next";

// GitHub Pages serves a project site under /<repo>; the workflow passes the
// path from actions/configure-pages. Empty for local development.
const basePath = (process.env.PAGES_BASE_PATH ?? "").replace(/\/+$/, "");

const nextConfig: NextConfig = {
  output: "export",
  trailingSlash: true,
  basePath: basePath || undefined,
  assetPrefix: basePath || undefined,
  images: { unoptimized: true },
  env: { PAGES_BASE_PATH: basePath },
  poweredByHeader: false,
  // `next dev` would otherwise write instruction files for coding tools into the project.
  agentRules: false,
};

export default nextConfig;
