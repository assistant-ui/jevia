"use client";

import { useEffect, useState } from "react";

import { DocsPageActions } from "./docs-page-actions";

const SECTIONS = [
  { id: "overview", number: "00", label: "Overview", title: "Get started with Jevia" },
  { id: "install", number: "01", label: "Install", title: "Install the Node.js package" },
  { id: "quickstart", number: "02", label: "Quickstart", title: "Create a client and route" },
  { id: "methods", number: "03", label: "Methods", title: "Client methods" },
  { id: "adapters", number: "04", label: "Harness adapters", title: "Open any harness" },
  { id: "errors", number: "05", label: "Errors", title: "Cancellation and errors" },
] as const;

type SectionId = (typeof SECTIONS)[number]["id"];

export function DocsSidebar() {
  const [activeId, setActiveId] = useState<SectionId>("overview");

  useEffect(() => {
    let frame = 0;

    function updateActiveSection() {
      frame = 0;
      const pageBarBottom =
        document.querySelector(".docs-page-bar")?.getBoundingClientRect().bottom ?? 126;
      const mobileNavigationHeight =
        window.innerWidth <= 800
          ? (document.querySelector(".docs-sidebar")?.getBoundingClientRect().height ?? 0)
          : 0;
      const offset = pageBarBottom + mobileNavigationHeight + 21;
      let nextId: SectionId = SECTIONS[0].id;
      const atPageEnd =
        Math.ceil(window.scrollY + window.innerHeight) >=
        document.documentElement.scrollHeight - 1;

      if (atPageEnd) {
        nextId = SECTIONS[SECTIONS.length - 1].id;
      } else {
        for (const { id } of SECTIONS) {
          const section = document.getElementById(id);
          if (section && section.getBoundingClientRect().top <= offset) {
            nextId = id;
          }
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
    if (window.innerWidth > 800) return;

    const anchor = document.querySelector<HTMLAnchorElement>(
      '.docs-nav-group a[href="#' + activeId + '"]',
    );
    const navigation = anchor?.closest<HTMLElement>(".docs-nav-group");
    if (!anchor || !navigation) return;

    const left =
      anchor.offsetLeft - navigation.clientWidth / 2 + anchor.offsetWidth / 2;
    navigation.scrollTo({ left, behavior: "auto" });
  }, [activeId]);

  const activeSection =
    SECTIONS.find((section) => section.id === activeId) ?? SECTIONS[0];

  return (
    <>
      <header className="docs-page-bar">
        <a className="docs-sidebar-title" href="#overview">
          <span aria-hidden="true">01</span>
          API reference
        </a>
        <div className="docs-page-bar-main">
          <h1>{activeSection.title}</h1>
          <DocsPageActions />
        </div>
        <span className="frame-junctions docs-frame-junctions" aria-hidden="true" />
      </header>

      <aside className="docs-sidebar" aria-label="Documentation navigation">
        <div className="docs-sidebar-inner">
          <div className="docs-nav-group">
            <nav aria-label="Node API sections">
              {SECTIONS.map(({ id, number, label }) => (
                <a
                  key={id}
                  href={"#" + id}
                  aria-current={activeId === id ? "location" : undefined}
                >
                  <span className="docs-nav-index" aria-hidden="true">
                    {number}
                  </span>
                  <span>{label}</span>
                </a>
              ))}
            </nav>
          </div>
        </div>
      </aside>
    </>
  );
}
