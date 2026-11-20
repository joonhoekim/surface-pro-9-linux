// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Joonhoe Kim
import Link from "next/link";
import Logo from "./Logo";
import ThemeToggle from "./ThemeToggle";
import { REPO_URL, SITE_NAME } from "@/lib/config";
import { requireRoute } from "@/lib/docs";

export default function SiteHeader() {
  return (
    <header className="site-header">
      <div className="site-header-inner">
        <Link href="/" className="brand">
          <Logo />
          <span>{SITE_NAME}</span>
        </Link>
        <nav aria-label="Main" className="main-nav">
          <Link href="/docs/">Docs</Link>
          <Link href={requireRoute("docs/install-nixos.md")}>Install</Link>
          <Link href={requireRoute("docs/hardware-status.md")}>Hardware</Link>
          <Link href={requireRoute("docs/research/gaps/README.md")} className="nav-optional">
            Research
          </Link>
          <a href={REPO_URL}>GitHub</a>
        </nav>
        <ThemeToggle />
      </div>
    </header>
  );
}
