// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Joonhoe Kim
// pages.txt: one line per rendered document, "<route>\t<repository file>". Read by
// scripts/check-export.mjs to see that every document became a page.
import { getDocs } from "@/lib/docs";

export const dynamic = "force-static";

export function GET() {
  const body = getDocs()
    .map((d) => `${d.route}\t${d.file}\n`)
    .join("");
  return new Response(body, { headers: { "content-type": "text/plain; charset=utf-8" } });
}
