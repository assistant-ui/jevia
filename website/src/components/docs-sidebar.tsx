"use client";

import { type MouseEvent, useEffect, useRef, useState } from "react";

import { glidePosition, scrollDuration } from "../lib/scroll-motion";
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

const NAVIGATION_FALLBACK_DELAY_MS = 5_000;
const NAVIGATION_SETTLE_DELAY_MS = 250;
const SCROLL_POSITION_TOLERANCE = 2;
const SCROLL_KEYS = new Set([
  "ArrowDown",
  "ArrowUp",
  "End",
  "Home",
  "PageDown",
  "PageUp",
  " ",
]);

function getDocsScrollOffset() {
  const pageBarBottom =
    document.querySelector(".docs-page-bar")?.getBoundingClientRect().bottom ?? 126;
  const mobileNavigationHeight =
    window.innerWidth <= 800
      ? (document.querySelector(".docs-sidebar")?.getBoundingClientRect().height ?? 0)
      : 0;

  return pageBarBottom + mobileNavigationHeight + 21;
}

export function DocsSidebar() {
  const [activeId, setActiveId] = useState<SectionId>("overview");
  const navigationTargetRef = useRef<SectionId | null>(null);
  const navigationReleaseTimerRef = useRef<number | null>(null);
  const scrollFrameRef = useRef(0);
  const scrollStartRef = useRef(0);
  const scrollTargetRef = useRef<number | null>(null);
  const scrollStartedAtRef = useRef(0);
  const scrollDurationRef = useRef(0);

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
      const offset = getDocsScrollOffset();
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
          if (
            section &&
            section.getBoundingClientRect().top <= offset + SCROLL_POSITION_TOLERANCE
          ) {
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

    function stopScrollMotion() {
      if (scrollFrameRef.current === 0) return;

      window.cancelAnimationFrame(scrollFrameRef.current);
      scrollFrameRef.current = 0;
      scrollTargetRef.current = null;
      clearNavigationTarget();
      scheduleUpdate();
    }

    function stopScrollMotionFromKeyboard(event: KeyboardEvent) {
      if (SCROLL_KEYS.has(event.key)) stopScrollMotion();
    }

    updateActiveSection();
    window.addEventListener("scroll", scheduleUpdate, { passive: true });
    window.addEventListener("resize", scheduleUpdate);
    window.addEventListener("hashchange", scheduleUpdate);
    window.addEventListener("popstate", scheduleUpdate);
    window.addEventListener("pointerdown", stopScrollMotion, { passive: true });
    window.addEventListener("touchstart", stopScrollMotion, { passive: true });
    window.addEventListener("wheel", stopScrollMotion, { passive: true });
    window.addEventListener("keydown", stopScrollMotionFromKeyboard);

    return () => {
      if (frame !== 0) window.cancelAnimationFrame(frame);
      if (scrollFrameRef.current !== 0) {
        window.cancelAnimationFrame(scrollFrameRef.current);
      }
      if (navigationReleaseTimerRef.current !== null) {
        window.clearTimeout(navigationReleaseTimerRef.current);
      }
      window.removeEventListener("scroll", scheduleUpdate);
      window.removeEventListener("resize", scheduleUpdate);
      window.removeEventListener("hashchange", scheduleUpdate);
      window.removeEventListener("popstate", scheduleUpdate);
      window.removeEventListener("pointerdown", stopScrollMotion);
      window.removeEventListener("touchstart", stopScrollMotion);
      window.removeEventListener("wheel", stopScrollMotion);
      window.removeEventListener("keydown", stopScrollMotionFromKeyboard);
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

  function animateScroll(time: number) {
    const target = scrollTargetRef.current;
    if (target === null) {
      scrollFrameRef.current = 0;
      return;
    }

    const next = glidePosition(
      scrollStartRef.current,
      target,
      time - scrollStartedAtRef.current,
      scrollDurationRef.current,
    );
    window.scrollTo({ top: next, behavior: "instant" });

    if (next === target) {
      scrollFrameRef.current = 0;
      scrollTargetRef.current = null;
      return;
    }

    scrollFrameRef.current = window.requestAnimationFrame(animateScroll);
  }

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

    const section = document.getElementById(id);
    if (!section) return;

    const target = Math.min(
      document.documentElement.scrollHeight - window.innerHeight,
      Math.max(0, window.scrollY + section.getBoundingClientRect().top - getDocsScrollOffset()),
    );

    const shouldSkipMotion =
      event.detail === 0 ||
      window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (shouldSkipMotion) {
      window.scrollTo({ top: target, behavior: "instant" });
      return;
    }

    scrollTargetRef.current = target;
    scrollStartRef.current = window.scrollY;
    scrollStartedAtRef.current = window.performance.now();
    scrollDurationRef.current = scrollDuration(target - scrollStartRef.current);
    if (scrollFrameRef.current === 0) {
      scrollFrameRef.current = window.requestAnimationFrame(animateScroll);
    }
  }

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
                  onClick={(event) => beginSectionNavigation(event, id)}
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
