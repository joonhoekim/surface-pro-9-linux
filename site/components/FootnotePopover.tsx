// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Joonhoe Kim
"use client";

import { useEffect } from "react";

// Footnote references (GFM `[^name]`, rendered by remark-gfm as <sup><a data-footnote-ref>)
// show their note in a floating panel: on mouse hover, on keyboard focus, and on a first tap
// on touch screens (a second tap follows the link to the note). The note itself stays at the
// bottom of the page; without JavaScript the reference is a plain link to it.
//
// One panel serves the page. It is positioned absolutely, outside the text flow, so opening it
// moves nothing. While it is open the reference is described by it (aria-describedby); Escape,
// a tap elsewhere, or leaving both the reference and the panel closes it.

const REF = 'a[data-footnote-ref]';
const POP_ID = "fn-popover";
const GUTTER = 16;
const OPEN_DELAY = 90;
const CLOSE_DELAY = 220;

function noteFor(ref: HTMLAnchorElement): HTMLElement | null {
  const id = decodeURIComponent((ref.getAttribute("href") ?? "").replace(/^#/, ""));
  return id ? document.getElementById(id) : null;
}

export default function FootnotePopover({ route }: { route: string }) {
  useEffect(() => {
    const prose = document.querySelector<HTMLElement>(".doc .prose");
    if (!prose || !prose.querySelector(REF)) return;

    const pop = document.createElement("div");
    pop.id = POP_ID;
    pop.className = "fn-popover";
    pop.setAttribute("role", "tooltip");
    pop.hidden = true;
    document.body.appendChild(pop);

    let current: HTMLAnchorElement | null = null;
    let savedDescribedBy: string | null = null;
    let openTimer = 0;
    let closeTimer = 0;
    let lastPointer = "mouse";

    const clearTimers = () => {
      window.clearTimeout(openTimer);
      window.clearTimeout(closeTimer);
    };

    const place = () => {
      if (!current) return;
      const r = current.getBoundingClientRect();
      const vw = document.documentElement.clientWidth;
      const vh = window.innerHeight;
      pop.style.maxWidth = `${Math.min(448, vw - 2 * GUTTER)}px`;
      const w = pop.offsetWidth;
      const h = pop.offsetHeight;
      let left = r.left + r.width / 2 - w / 2;
      left = Math.max(GUTTER, Math.min(left, vw - GUTTER - w));
      const below = r.bottom + 8;
      const above = r.top - 8 - h;
      const top = below + h <= vh - 8 || above < 8 ? below : above;
      pop.dataset.side = top === below ? "below" : "above";
      pop.style.left = `${left + window.scrollX}px`;
      pop.style.top = `${top + window.scrollY}px`;
    };

    const close = () => {
      clearTimers();
      if (!current) return;
      if (savedDescribedBy === null) current.removeAttribute("aria-describedby");
      else current.setAttribute("aria-describedby", savedDescribedBy);
      current.removeAttribute("aria-expanded");
      current = null;
      pop.hidden = true;
      pop.replaceChildren();
    };

    const open = (ref: HTMLAnchorElement) => {
      clearTimers();
      if (current === ref) return;
      close();
      const note = noteFor(ref);
      if (!note) return;
      const label = document.createElement("a");
      label.className = "fn-popover-label";
      label.href = ref.getAttribute("href") ?? "";
      label.textContent = `Note ${ref.textContent?.trim() ?? ""}`;
      label.tabIndex = -1;
      const body = document.createElement("div");
      body.className = "fn-popover-body";
      for (const child of Array.from(note.childNodes)) body.appendChild(child.cloneNode(true));
      body.querySelectorAll("[data-footnote-backref]").forEach((a) => a.remove());
      body.querySelectorAll("[id]").forEach((el) => el.removeAttribute("id"));
      body.querySelectorAll("a").forEach((a) => (a.tabIndex = -1));
      pop.replaceChildren(label, body);
      current = ref;
      savedDescribedBy = ref.getAttribute("aria-describedby");
      ref.setAttribute("aria-describedby", POP_ID);
      ref.setAttribute("aria-expanded", "true");
      pop.hidden = false;
      place();
    };

    const refOf = (t: EventTarget | null) =>
      t instanceof Element ? (t.closest(REF) as HTMLAnchorElement | null) : null;

    const onPointerDown = (e: PointerEvent) => {
      lastPointer = e.pointerType || "mouse";
      if (current && !pop.contains(e.target as Node) && refOf(e.target) !== current) close();
    };
    const onPointerOver = (e: PointerEvent) => {
      if (e.pointerType !== "mouse") return;
      const ref = refOf(e.target);
      if (ref && prose.contains(ref)) {
        window.clearTimeout(closeTimer);
        window.clearTimeout(openTimer);
        openTimer = window.setTimeout(() => open(ref), current ? 0 : OPEN_DELAY);
      } else if (pop.contains(e.target as Node)) {
        window.clearTimeout(closeTimer);
      }
    };
    const onPointerOut = (e: PointerEvent) => {
      if (e.pointerType !== "mouse") return;
      const from = refOf(e.target) ?? (pop.contains(e.target as Node) ? pop : null);
      if (!from) return;
      const to = e.relatedTarget as Node | null;
      if (to && (pop.contains(to) || (current && current.contains(to)))) return;
      window.clearTimeout(openTimer);
      closeTimer = window.setTimeout(close, CLOSE_DELAY);
    };
    const onFocusIn = (e: FocusEvent) => {
      const ref = refOf(e.target);
      if (ref && prose.contains(ref) && lastPointer === "mouse") open(ref);
    };
    const onFocusOut = (e: FocusEvent) => {
      if (refOf(e.target) && refOf(e.target) === current) close();
    };
    const onClick = (e: MouseEvent) => {
      const ref = refOf(e.target);
      if (!ref || !prose.contains(ref)) return;
      // touch and pen: the first tap shows the note, a second tap on the same marker follows the link
      if (lastPointer !== "mouse" && current !== ref) {
        e.preventDefault();
        open(ref);
      } else {
        close();
      }
    };
    const onKeyDown = (e: KeyboardEvent) => {
      lastPointer = "mouse";
      if (e.key === "Escape" && current) {
        const ref = current;
        close();
        ref.focus();
      }
    };
    let frame = 0;
    const onMove = () => {
      if (!current || frame) return;
      frame = window.requestAnimationFrame(() => {
        frame = 0;
        place();
      });
    };

    document.addEventListener("pointerdown", onPointerDown, true);
    document.addEventListener("pointerover", onPointerOver);
    document.addEventListener("pointerout", onPointerOut);
    document.addEventListener("focusin", onFocusIn);
    document.addEventListener("focusout", onFocusOut);
    document.addEventListener("click", onClick);
    document.addEventListener("keydown", onKeyDown);
    window.addEventListener("resize", onMove);
    window.addEventListener("scroll", onMove, { passive: true });
    return () => {
      close();
      window.cancelAnimationFrame(frame);
      document.removeEventListener("pointerdown", onPointerDown, true);
      document.removeEventListener("pointerover", onPointerOver);
      document.removeEventListener("pointerout", onPointerOut);
      document.removeEventListener("focusin", onFocusIn);
      document.removeEventListener("focusout", onFocusOut);
      document.removeEventListener("click", onClick);
      document.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("resize", onMove);
      window.removeEventListener("scroll", onMove);
      pop.remove();
    };
  }, [route]);
  return null;
}
