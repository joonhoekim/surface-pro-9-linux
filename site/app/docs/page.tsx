// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Joonhoe Kim
import type { Metadata } from "next";
import { notFound } from "next/navigation";
import DocPage from "@/components/DocPage";
import { findDocBySlug } from "@/lib/docs";

export const metadata: Metadata = { title: "Overview" };

export default function DocsIndex() {
  const entry = findDocBySlug([]);
  if (!entry) notFound();
  return <DocPage entry={entry} />;
}
