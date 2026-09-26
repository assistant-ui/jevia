"use client";

import type { ComponentProps, ReactNode } from "react";
import { useEffect, useRef } from "react";

function cx(base: string, className?: string) {
  return className ? `${base} ${className}` : base;
}

export type TimescaleRootProps = ComponentProps<"div"> & {
  orientation?: "horizontal" | "vertical";
};

export function TimescaleRoot({
  className,
  orientation = "horizontal",
  ...props
}: TimescaleRootProps) {
  return (
    <div
      data-slot="timescale-root"
      data-orientation={orientation}
      className={cx("timescale-root", className)}
      {...props}
    />
  );
}

export type TimescaleViewportProps = ComponentProps<"div">;

export function TimescaleViewport({ className, ...props }: TimescaleViewportProps) {
  return (
    <div
      data-slot="timescale-viewport"
      className={cx("timescale-viewport", className)}
      {...props}
    />
  );
}

export type TimescaleHeaderProps = ComponentProps<"div">;

export function TimescaleHeader({ className, ...props }: TimescaleHeaderProps) {
  return (
    <div
      data-slot="timescale-header"
      aria-hidden="true"
      className={cx("timescale-header", className)}
      {...props}
    />
  );
}

export type TimescaleTrackProps = ComponentProps<"div">;

export function TimescaleTrack({ className, ...props }: TimescaleTrackProps) {
  return (
    <div
      data-slot="timescale-track"
      className={cx("timescale-track", className)}
      {...props}
    />
  );
}

export type TimescaleRailProps = ComponentProps<"div">;

export function TimescaleRail({ className, ...props }: TimescaleRailProps) {
  return (
    <div
      data-slot="timescale-rail"
      aria-hidden="true"
      className={cx("timescale-rail", className)}
      {...props}
    />
  );
}

export type TimescaleItemProps = ComponentProps<"div">;

export function TimescaleItem({ className, ...props }: TimescaleItemProps) {
  return (
    <div
      data-slot="timescale-item"
      className={cx("timescale-item", className)}
      {...props}
    />
  );
}

export type TimescaleTickProps = ComponentProps<"span">;

export function TimescaleTick({ className, ...props }: TimescaleTickProps) {
  return (
    <span
      data-slot="timescale-tick"
      aria-hidden="true"
      className={cx("timescale-tick", className)}
      {...props}
    />
  );
}

export type TimescaleAgeProps = ComponentProps<"p">;

export function TimescaleAge({ className, ...props }: TimescaleAgeProps) {
  return (
    <p data-slot="timescale-age" className={cx("timescale-age", className)} {...props} />
  );
}

export type TimescaleYearProps = ComponentProps<"p">;

export function TimescaleYear({ className, ...props }: TimescaleYearProps) {
  return (
    <p data-slot="timescale-year" className={cx("timescale-year", className)} {...props} />
  );
}

export type TimescaleContentProps = ComponentProps<"div">;

export function TimescaleContent({ className, ...props }: TimescaleContentProps) {
  return (
    <div
      data-slot="timescale-content"
      className={cx("timescale-content", className)}
      {...props}
    />
  );
}

const INTRO_SCROLL_START_HOLD = 200;

export function TimescaleIntroScroll({ children }: { children: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const viewport = ref.current?.querySelector<HTMLElement>(
      '[data-slot="timescale-viewport"]',
    );
    if (!viewport) return;

    const distance = viewport.scrollWidth - viewport.clientWidth;
    if (distance <= 0) return;

    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
      viewport.scrollLeft = distance;
      return;
    }

    const timer = window.setTimeout(() => {
      viewport.scrollTo({ left: distance, behavior: "smooth" });
    }, INTRO_SCROLL_START_HOLD);

    return () => window.clearTimeout(timer);
  }, []);

  return (
    <div ref={ref} className="timescale-intro-scroll">
      {children}
    </div>
  );
}
