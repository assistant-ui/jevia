"use client";

import { ArrowLeft, ArrowRight } from "lucide-react";
import { useEffect, useRef, useState, type ReactNode } from "react";

export function ScrollTimescale({ children }: { children: ReactNode }) {
  const sectionRef = useRef<HTMLElement>(null);
  const moveRef = useRef<(direction: number) => void>(() => {});
  const [edges, setEdges] = useState({ start: true, end: false });

  useEffect(() => {
    const section = sectionRef.current;
    const panel = section?.querySelector<HTMLElement>(".quickstart-panel");
    const viewport = section?.querySelector<HTMLElement>(".timescale-viewport");
    const track = section?.querySelector<HTMLElement>(".timescale-track");
    if (!section || !panel || !viewport || !track) return;

    const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");
    let distance = 0;
    let driven = false;
    let renderedLeft = viewport.scrollLeft;
    let frame = 0;

    const clamp = (value: number) => Math.min(distance, Math.max(0, value));
    const updateControls = () => {
      const start = viewport.scrollLeft <= 1;
      const end = viewport.scrollLeft >= distance - 1;
      setEdges((current) =>
        current.start === start && current.end === end ? current : { start, end },
      );
    };

    const update = () => {
      frame = 0;
      if (driven) {
        renderedLeft = clamp(-section.getBoundingClientRect().top);
        viewport.scrollLeft = renderedLeft;
      }
      updateControls();
    };

    const scheduleUpdate = () => {
      if (!frame) frame = window.requestAnimationFrame(update);
    };

    const measure = () => {
      distance = Math.max(0, viewport.scrollWidth - viewport.clientWidth);
      driven =
        !reducedMotion.matches &&
        distance > 0 &&
        panel.offsetHeight <= window.innerHeight + 1;
      section.dataset.scrollDriven = String(driven);
      section.style.height = driven ? `${panel.offsetHeight + distance}px` : "";
      scheduleUpdate();
    };

    const moveTo = (left: number) => {
      renderedLeft = clamp(left);
      if (driven) {
        const top = section.getBoundingClientRect().top + window.scrollY;
        window.scrollTo({ top: top + renderedLeft, behavior: "instant" });
      }
      viewport.scrollTo({ left: renderedLeft, behavior: "instant" });
      updateControls();
    };

    moveRef.current = (direction) => {
      const item = track.querySelector<HTMLElement>(".timescale-item");
      moveTo(viewport.scrollLeft + direction * (item?.offsetWidth ?? 320));
    };

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.target !== viewport || event.altKey || event.ctrlKey || event.metaKey) return;
      if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
        event.preventDefault();
        moveRef.current(event.key === "ArrowRight" ? 1 : -1);
      } else if (event.key === "Home" || event.key === "End") {
        event.preventDefault();
        moveTo(event.key === "End" ? distance : 0);
      }
    };

    const onHorizontalScroll = () => {
      updateControls();
      if (!driven || Math.abs(viewport.scrollLeft - renderedLeft) < 1) return;
      const bounds = section.getBoundingClientRect();
      const isPinned = bounds.top <= 1 && bounds.bottom >= window.innerHeight - 1;
      // Keep touch, horizontal trackpad input, and keyboard focus in sync with the page.
      if (isPinned || viewport.contains(document.activeElement)) {
        renderedLeft = viewport.scrollLeft;
        window.scrollTo({
          top: bounds.top + window.scrollY + renderedLeft,
          behavior: "instant",
        });
      }
    };

    const observer = new ResizeObserver(measure);
    observer.observe(panel);
    observer.observe(viewport);
    observer.observe(track);
    window.addEventListener("scroll", scheduleUpdate, { passive: true });
    window.addEventListener("resize", measure);
    viewport.addEventListener("scroll", onHorizontalScroll, { passive: true });
    viewport.addEventListener("keydown", onKeyDown);
    reducedMotion.addEventListener("change", measure);
    measure();

    return () => {
      observer.disconnect();
      window.cancelAnimationFrame(frame);
      window.removeEventListener("scroll", scheduleUpdate);
      window.removeEventListener("resize", measure);
      viewport.removeEventListener("scroll", onHorizontalScroll);
      viewport.removeEventListener("keydown", onKeyDown);
      reducedMotion.removeEventListener("change", measure);
      delete section.dataset.scrollDriven;
      section.style.height = "";
      moveRef.current = () => {};
    };
  }, []);

  return (
    <section
      id="quickstart"
      ref={sectionRef}
      className="quickstart page-frame"
      aria-labelledby="setup-title"
    >
      <div className="quickstart-panel">
        <header className="section-heading">
          <span className="frame-junctions" aria-hidden="true" />
          <h2 id="setup-title">Get started</h2>
          <div className="timeline-controls" aria-label="Timeline navigation">
            <button
              type="button"
              aria-label="Previous setup step"
              disabled={edges.start}
              onClick={() => moveRef.current(-1)}
            >
              <ArrowLeft size={16} strokeWidth={1.5} aria-hidden="true" />
            </button>
            <button
              type="button"
              aria-label="Next setup step"
              disabled={edges.end}
              onClick={() => moveRef.current(1)}
            >
              <ArrowRight size={16} strokeWidth={1.5} aria-hidden="true" />
            </button>
          </div>
        </header>
        {children}
      </div>
    </section>
  );
}
