"use client";

import { type MouseEvent, useCallback, useEffect, useRef, useState } from "react";

import { navHighlightBox, navHighlightTarget } from "../lib/nav-highlight";

interface DocsNavLinksProps<Id extends string> {
  sections: readonly { id: Id; number: string; label: string }[];
  activeId: Id;
  onNavigate: (event: MouseEvent<HTMLAnchorElement>, id: Id) => void;
}

/** One interruptible highlight, matching Beautiful UI's 220ms ease-out glide. */
export function DocsNavLinks<Id extends string>({
  sections, activeId, onNavigate,
}: DocsNavLinksProps<Id>) {
  const navRef = useRef<HTMLElement>(null);
  const highlightRef = useRef<HTMLSpanElement>(null);
  const [hoveredId, setHoveredId] = useState<Id | null>(null);
  const [focusedId, setFocusedId] = useState<Id | null>(null);
  const [input, setInput] = useState<"pointer" | "keyboard">("pointer");
  const targetId = navHighlightTarget(activeId, hoveredId, focusedId);
  const keyboard = input === "keyboard";

  const positionHighlight = useCallback((animate: boolean) => {
    const nav = navRef.current;
    const highlight = highlightRef.current;
    const target = nav?.querySelector<HTMLElement>('[data-highlighted="true"]');
    if (!nav || !highlight || !target) return;

    const box = navHighlightBox(target.getBoundingClientRect(), nav.getBoundingClientRect());
    highlight.dataset.animate = String(animate && nav.dataset.highlightReady === "true");
    highlight.style.transform = `translate3d(${box.x}px, ${box.y}px, 0)`;
    highlight.style.width = `${box.width}px`;
    highlight.style.height = `${box.height}px`;
    nav.dataset.highlightReady = "true";
  }, []);

  useEffect(() => {
    positionHighlight(!keyboard);
  }, [targetId, keyboard, positionHighlight]);

  useEffect(() => {
    const nav = navRef.current;
    if (!nav) return;
    // Snap across viewport changes, but keep gliding when the selected label's
    // font weight changes a mobile tab's width. Keep the observer mounted across
    // target changes so it does not restart the transition.
    let viewportWidth = window.innerWidth;
    const observer = new ResizeObserver(() => {
      const resized = viewportWidth !== window.innerWidth;
      viewportWidth = window.innerWidth;
      if (resized) setHoveredId(null);
      positionHighlight(!resized && nav.dataset.input !== "keyboard");
    });
    observer.observe(nav);
    nav.querySelectorAll("a").forEach((link) => observer.observe(link));
    return () => observer.disconnect();
  }, [sections, positionHighlight]);

  return (
    <nav
      ref={navRef}
      className="docs-nav"
      data-input={keyboard ? "keyboard" : "pointer"}
      aria-label="Node API sections"
      onPointerLeave={() => setHoveredId(null)}
      onPointerCancel={() => setHoveredId(null)}
      onPointerDown={() => setInput("pointer")}
    >
      <span ref={highlightRef} className="docs-nav-highlight" aria-hidden="true" />
      {sections.map(({ id, number, label }) => (
        <a
          key={id}
          href={"#" + id}
          aria-current={activeId === id ? "location" : undefined}
          data-highlighted={targetId === id}
          onPointerEnter={(event) => {
            if (event.pointerType === "mouse" && window.matchMedia("(min-width: 801px) and (hover: hover) and (pointer: fine)").matches) {
              setHoveredId(id);
              setFocusedId(null);
              setInput("pointer");
            }
          }}
          onFocus={(event) => {
            if (event.currentTarget.matches(":focus-visible")) {
              setHoveredId(null);
              setFocusedId(id);
              setInput("keyboard");
            }
          }}
          onBlur={() => setFocusedId(null)}
          onClick={(event) => onNavigate(event, id)}
        >
          <span className="docs-nav-index" aria-hidden="true">{number}</span>
          <span>{label}</span>
        </a>
      ))}
    </nav>
  );
}
