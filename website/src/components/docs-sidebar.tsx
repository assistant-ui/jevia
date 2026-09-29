"use client";

import { type MouseEvent, useEffect, useRef, useState } from "react";

import { DocsPageActions } from "./docs-page-actions";
import { DocsNavLinks } from "./docs-nav-links";

const SECTIONS = [
  { id: "overview", number: "00", label: "Overview", title: "Jevia documentation" },
  { id: "install", number: "01", label: "Install", title: "Install and validate" },
  { id: "quickstart", number: "02", label: "CLI workflow", title: "Route, run, and inspect" },
  { id: "adaptive", number: "03", label: "Adaptive loop", title: "How routing learns" },
  { id: "adapters", number: "04", label: "Harnesses", title: "Connect any harness" },
  { id: "methods", number: "05", label: "Node.js SDK", title: "Embed Jevia in a harness" },
  { id: "outcomes", number: "06", label: "Outcomes", title: "Verify and report outcomes" },
  { id: "storage", number: "07", label: "Storage", title: "Choose and configure storage" },
  { id: "errors", number: "08", label: "Maintenance", title: "Cache, diagnostics, and recovery" },
] as const;

type SectionId = (typeof SECTIONS)[number]["id"];

const NAVIGATION_FALLBACK_DELAY_MS = 5_000;
const NAVIGATION_SETTLE_DELAY_MS = 250;

export function DocsSidebar() {
  const [activeId, setActiveId] = useState<SectionId>("overview");
  const navigationTargetRef = useRef<SectionId | null>(null);
  const navigationReleaseTimerRef = useRef<number | null>(null);

  useEffect(() => {
    let frame = 0;

    function clearNavigationTarget() {
      navigationTargetRef.current = null;

      if (navigationReleaseTimerRef.current !== null) {
        window.clearTimeout(navigationReleaseTimerRef.current);
        navigationReleaseTimerRef.current = null;
      }
    }

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
      const navigationTarget = navigationTargetRef.current;

      if (navigationTarget !== null) {
        setActiveId(navigationTarget);
        return;
      }

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

    function releaseNavigationTarget() {
      clearNavigationTarget();
      updateActiveSection();
    }

    function scheduleUpdate() {
      if (navigationTargetRef.current !== null) {
        if (navigationReleaseTimerRef.current !== null) {
          window.clearTimeout(navigationReleaseTimerRef.current);
        }
        navigationReleaseTimerRef.current = window.setTimeout(
          releaseNavigationTarget,
          NAVIGATION_SETTLE_DELAY_MS,
        );
      }

      if (frame === 0) {
        frame = window.requestAnimationFrame(updateActiveSection);
      }
    }

    updateActiveSection();
    window.addEventListener("scroll", scheduleUpdate, { passive: true });
    window.addEventListener("resize", scheduleUpdate);
    window.addEventListener("hashchange", scheduleUpdate);
    window.addEventListener("popstate", scheduleUpdate);

    return () => {
      if (frame !== 0) window.cancelAnimationFrame(frame);
      if (navigationReleaseTimerRef.current !== null) {
        window.clearTimeout(navigationReleaseTimerRef.current);
      }
      window.removeEventListener("scroll", scheduleUpdate);
      window.removeEventListener("resize", scheduleUpdate);
      window.removeEventListener("hashchange", scheduleUpdate);
      window.removeEventListener("popstate", scheduleUpdate);
    };
  }, []);

  useEffect(() => {
    function centerActiveLink() {
      if (window.innerWidth > 800) return;
      const anchor = document.querySelector<HTMLAnchorElement>(
        '.docs-nav-group a[href="#' + activeId + '"]',
      );
      const navigation = anchor?.closest<HTMLElement>(".docs-nav-group");
      if (!anchor || !navigation) return;

      const left = navigation.scrollLeft + anchor.getBoundingClientRect().left -
        navigation.getBoundingClientRect().left - navigation.clientWidth / 2 + anchor.offsetWidth / 2;
      navigation.scrollTo({ left, behavior: "auto" });
    }
    centerActiveLink();
    window.addEventListener("resize", centerActiveLink);
    return () => window.removeEventListener("resize", centerActiveLink);
  }, [activeId]);

  const activeSection =
    SECTIONS.find((section) => section.id === activeId) ?? SECTIONS[0];

  function beginSectionNavigation(
    event: MouseEvent<HTMLAnchorElement>,
    id: SectionId,
  ) {
    if (
      event.button !== 0 ||
      event.metaKey ||
      event.ctrlKey ||
      event.shiftKey ||
      event.altKey
    ) {
      return;
    }

    event.preventDefault();
    if (id === activeId) return;

    if (navigationReleaseTimerRef.current !== null) {
      window.clearTimeout(navigationReleaseTimerRef.current);
    }

    navigationTargetRef.current = id;
    navigationReleaseTimerRef.current = window.setTimeout(() => {
      navigationTargetRef.current = null;
      navigationReleaseTimerRef.current = null;
      window.dispatchEvent(new Event("scroll"));
    }, NAVIGATION_FALLBACK_DELAY_MS);
    setActiveId(id);

    const hash = `#${id}`;
    if (window.location.hash !== hash) {
      window.history.pushState(null, "", hash);
    }

    document.getElementById(id)?.scrollIntoView({
      behavior: event.detail === 0 || window.matchMedia("(prefers-reduced-motion: reduce)").matches
        ? "instant"
        : "smooth",
      block: "start",
    });
  }

  return (
    <>
      <header className="docs-page-bar">
        <a className="docs-sidebar-title" href="#overview">
          <span aria-hidden="true">00</span>
          Documentation
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
            <DocsNavLinks sections={SECTIONS} activeId={activeId} onNavigate={beginSectionNavigation} />
          </div>
        </div>
      </aside>
    </>
  );
}
