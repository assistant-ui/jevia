"use client";

import { ArrowLeft, ArrowRight } from "lucide-react";
import { useEffect, useRef, useState, type ReactNode } from "react";

export function ScrollTimescale({ children }: { children: ReactNode }) {
  const sectionRef = useRef<HTMLElement>(null);
  const moveRef = useRef<(direction: number) => void>(() => {});
  const [edges, setEdges] = useState({ start: true, end: false });

  useEffect(() => {
    const section = sectionRef.current;
    const viewport = section?.querySelector<HTMLElement>(".timescale-viewport");
    const track = section?.querySelector<HTMLElement>(".timescale-track");
    if (!section || !viewport || !track) return;

    const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");
    let distance = 0;

    const clamp = (value: number) => Math.min(distance, Math.max(0, value));
    const updateControls = () => {
      const start = viewport.scrollLeft <= 1;
      const end = viewport.scrollLeft >= distance - 1;
      setEdges((current) =>
        current.start === start && current.end === end ? current : { start, end },
      );
    };

    const measure = () => {
      distance = Math.max(0, viewport.scrollWidth - viewport.clientWidth);
      updateControls();
    };

    const moveTo = (left: number) => {
      viewport.scrollTo({ left: clamp(left), behavior: "instant" });
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

    const onWheel = (event: WheelEvent) => {
      if (
        reducedMotion.matches ||
        event.defaultPrevented ||
        !event.cancelable ||
        event.ctrlKey ||
        event.metaKey ||
        event.shiftKey ||
        Math.abs(event.deltaX) > Math.abs(event.deltaY) ||
        event.deltaY === 0 ||
        distance <= 0
      ) {
        return;
      }

      const bounds = section.getBoundingClientRect();
      // Start as soon as the compact section is visible, without moving the page
      // or adding vertical travel. Short screens keep their normal page scroll.
      if (bounds.top < -1 || bounds.bottom > window.innerHeight + 1) return;

      const unit =
        event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? window.innerHeight : 1;
      const left = viewport.scrollLeft;
      const next = clamp(left + event.deltaY * unit);
      // Let the browser handle the gesture normally at either end of the track.
      if (Math.abs(next - left) < 1) return;

      event.preventDefault();
      moveTo(next);
    };

    const observer = new ResizeObserver(measure);
    observer.observe(viewport);
    observer.observe(track);
    window.addEventListener("wheel", onWheel, { passive: false });
    viewport.addEventListener("scroll", updateControls, { passive: true });
    viewport.addEventListener("keydown", onKeyDown);
    measure();

    return () => {
      observer.disconnect();
      window.removeEventListener("wheel", onWheel);
      viewport.removeEventListener("scroll", updateControls);
      viewport.removeEventListener("keydown", onKeyDown);
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
