// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Joonhoe Kim
"use client";

import { useEffect } from "react";

// The navigation is longer than a screen. On wide layouts, where it scrolls by itself next to
// the page, bring the current page's entry into view (the page itself is not scrolled).
export default function CurrentIntoView() {
  useEffect(() => {
    const side = document.querySelector<HTMLElement>(".docs-sidebar");
    const cur = side?.querySelector<HTMLElement>('.docs-nav-wide a[aria-current="page"]');
    if (!side || !cur || getComputedStyle(side).position !== "sticky") return;
    const top = cur.offsetTop - side.offsetTop;
    if (top < side.scrollTop || top + cur.offsetHeight > side.scrollTop + side.clientHeight) {
      side.scrollTop = Math.max(0, top - side.clientHeight / 3);
    }
  }, []);
  return null;
}
