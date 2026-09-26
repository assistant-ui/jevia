"use client";

import { ArrowLeft, ArrowRight } from "lucide-react";
import { useEffect, useRef, useState, type ReactNode } from "react";

import { isTimelineVisible, slidePosition, wheelTarget } from "../lib/timeline-scroll";

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
    let targetLeft = viewport.scrollLeft;
    let frame = 0;
    let lastTime = 0;

    const clamp = (value: number) => Math.min(distance, Math.max(0, value));
    const updateControls = () => {
      const start = viewport.scrollLeft <= 1;
      const end = viewport.scrollLeft >= distance - 1;
      setEdges((current) =>
        current.start === start && current.end === end ? current : { start, end },
      );
    };

    const stopAnimation = () => {
      window.cancelAnimationFrame(frame);
      frame = 0;
      targetLeft = viewport.scrollLeft;
    };

    const animate = (time: number) => {
      const next = slidePosition(viewport.scrollLeft, targetLeft, time - lastTime);
      lastTime = time;
      viewport.scrollTo({ left: next, behavior: "instant" });
      if (next === targetLeft) {
        frame = 0;
        updateControls();
      } else {
        frame = window.requestAnimationFrame(animate);
      }
    };

    const moveTo = (left: number) => {
      const next = clamp(left);
      if (reducedMotion.matches) {
        stopAnimation();
        targetLeft = next;
        viewport.scrollTo({ left: next, behavior: "instant" });
        updateControls();
        return;
      }

      targetLeft = next;
      if (!frame) {
        lastTime = performance.now();
        frame = window.requestAnimationFrame(animate);
      }
    };

    const measure = () => {
      stopAnimation();
      distance = Math.max(0, viewport.scrollWidth - viewport.clientWidth);
      targetLeft = clamp(viewport.scrollLeft);
      updateControls();
    };

    moveRef.current = (direction) => {
      const item = track.querySelector<HTMLElement>(".timescale-item");
      const left = frame ? targetLeft : viewport.scrollLeft;
      moveTo(left + direction * (item?.offsetWidth ?? 320));
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
        event.defaultPrevented ||
        !event.cancelable ||
        event.ctrlKey ||
        event.metaKey ||
        distance <= 0
      ) {
        return;
      }

      // Native horizontal gestures must be free to take over an in-flight slide.
      if (event.shiftKey || Math.abs(event.deltaX) > Math.abs(event.deltaY)) {
        stopAnimation();
        return;
      }
      if (event.deltaY === 0) return;

      // Measure the actual track, not the section's heading and outer padding.
      // No sticky spacer or extra vertical travel is needed to enter the timeline.
      if (!isTimelineVisible(viewport.getBoundingClientRect(), window.innerHeight)) return;

      const unit =
        event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? window.innerHeight : 1;
      const left = frame ? targetLeft : viewport.scrollLeft;
      const next = wheelTarget(viewport.scrollLeft, left, event.deltaY * unit, distance);
      // Release normal page scrolling at either end, once the slide has arrived.
      if (next === left) {
        if (frame && Math.abs(targetLeft - viewport.scrollLeft) >= 1) event.preventDefault();
        return;
      }

      event.preventDefault();
      moveTo(next);
    };

    const observer = new ResizeObserver(measure);
    observer.observe(viewport);
    observer.observe(track);
    window.addEventListener("wheel", onWheel, { passive: false });
    viewport.addEventListener("scroll", updateControls, { passive: true });
    viewport.addEventListener("keydown", onKeyDown);
    viewport.addEventListener("pointerdown", stopAnimation, { passive: true });
    viewport.addEventListener("focusin", stopAnimation);
    reducedMotion.addEventListener("change", stopAnimation);
    measure();

    return () => {
      observer.disconnect();
      stopAnimation();
      window.removeEventListener("wheel", onWheel);
      viewport.removeEventListener("scroll", updateControls);
      viewport.removeEventListener("keydown", onKeyDown);
      viewport.removeEventListener("pointerdown", stopAnimation);
      viewport.removeEventListener("focusin", stopAnimation);
      reducedMotion.removeEventListener("change", stopAnimation);
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
