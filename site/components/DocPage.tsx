// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Joonhoe Kim
import Link from "next/link";
import DocsSidebar from "./DocsSidebar";
import { TocAside, TocInline } from "./Toc";
import PanelToggles from "./PanelToggles";
import { githubUrl } from "@/lib/config";
import { neighbours, readDocSource, type DocEntry } from "@/lib/docs";
import { renderMarkdown } from "@/lib/markdown";

export async function renderDoc(entry: DocEntry) {
  return renderMarkdown(readDocSource(entry), entry.file, entry.title);
}

export default async function DocPage({ entry }: { entry: DocEntry }) {
  const doc = await renderDoc(entry);
  const { prev, next } = neighbours(entry);
  return (
    <div className="docs-layout">
      <DocsSidebar current={entry.route} />
      <main id="main" className="doc-main">
        <article className="doc">
          <PanelToggles hasToc={doc.toc.length >= 2} />
          <p className="doc-path">
            <a href={githubUrl(entry.file)}>{entry.file}</a>
          </p>
          <h1>{doc.title}</h1>
          <TocInline items={doc.toc} />
          <div className="prose" dangerouslySetInnerHTML={{ __html: doc.html }} />
          <nav className="pager" aria-label="Previous and next page">
            {prev ? (
              <Link href={prev.route} className="pager-prev" rel="prev">
                <span className="pager-dir">Previous</span>
                <span className="pager-title">{prev.route === "/docs/" ? "Overview" : prev.navTitle}</span>
              </Link>
            ) : (
              <span />
            )}
            {next && (
              <Link href={next.route} className="pager-next" rel="next">
                <span className="pager-dir">Next</span>
                <span className="pager-title">{next.navTitle}</span>
              </Link>
            )}
          </nav>
          <p className="doc-source">
            <a href={githubUrl(entry.file)}>View this page&apos;s Markdown on GitHub</a>
          </p>
        </article>
      </main>
      <TocAside items={doc.toc} />
    </div>
  );
}
