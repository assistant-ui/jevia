"use client";

import { useEffect, useState } from "react";

const LINKS = [
  ["overview", "Overview"],
  ["install", "Install"],
  ["quickstart", "Quickstart"],
  ["methods", "Methods"],
  ["adapters", "Harness adapters"],
  ["errors", "Errors"],
] as const;

type SectionId = (typeof LINKS)[number][0];

export function DocsSidebar() {
  const [activeId, setActiveId] = useState<SectionId>("overview");

  useEffect(() => {
    let frame = 0;

    function updateActiveSection() {
      frame = 0;
      const offset = window.innerWidth <= 800 ? 150 : 96;
      let nextId: SectionId = LINKS[0][0];

      for (const [id] of LINKS) {
        const section = document.getElementById(id);
        if (section && section.getBoundingClientRect().top <= offset) {
          nextId = id;
        }
      }

      setActiveId(nextId);
    }

    function scheduleUpdate() {
      if (frame === 0) {
        frame = window.requestAnimationFrame(updateActiveSection);
      }
    }

    updateActiveSection();
    window.addEventListener("scroll", scheduleUpdate, { passive: true });
    window.addEventListener("resize", scheduleUpdate);
    window.addEventListener("hashchange", scheduleUpdate);

    return () => {
      if (frame !== 0) window.cancelAnimationFrame(frame);
      window.removeEventListener("scroll", scheduleUpdate);
      window.removeEventListener("resize", scheduleUpdate);
      window.removeEventListener("hashchange", scheduleUpdate);
    };
  }, []);

  useEffect(() => {
    document
      .querySelector<HTMLAnchorElement>(
        '.docs-nav-group a[href="#' + activeId + '"]',
      )
      ?.scrollIntoView({ block: "nearest", inline: "nearest" });
  }, [activeId]);

  return (
    <aside className="docs-sidebar" aria-label="Documentation navigation">
      <div className="docs-sidebar-inner">
        <a className="docs-sidebar-title" href="#overview">
          <span aria-hidden="true">01</span>
          API reference
        </a>

        <div className="docs-nav-group">
          <p>Node API</p>
          <nav aria-label="Node API sections">
            {LINKS.map(([id, label]) => (
              <a
                key={id}
                href={"#" + id}
                aria-current={activeId === id ? "location" : undefined}
              >
                {label}
              </a>
            ))}
          </nav>
        </div>

        <div className="docs-sidebar-meta">
          <span>Package</span>
          <strong>jevia</strong>
          <span>Runtime</span>
          <strong>Node 20+</strong>
        </div>
      </div>
    </aside>
  );
}
