import type { ComponentProps } from "react";

function cx(base: string, className?: string) {
  return className ? `${base} ${className}` : base;
}

export function TimescaleRoot({ className, ...props }: ComponentProps<"div">) {
  return <div className={cx("timescale-root", className)} data-slot="timescale-root" {...props} />;
}

export function TimescaleHeader({ className, ...props }: ComponentProps<"div">) {
  return (
    <div
      className={cx("timescale-header", className)}
      data-slot="timescale-header"
      aria-hidden="true"
      {...props}
    />
  );
}

export function TimescaleViewport({ className, ...props }: ComponentProps<"div">) {
  return (
    <div
      className={cx("timescale-viewport", className)}
      data-slot="timescale-viewport"
      {...props}
    />
  );
}

export function TimescaleTrack({ className, ...props }: ComponentProps<"ol">) {
  return <ol className={cx("timescale-track", className)} data-slot="timescale-track" {...props} />;
}

export function TimescaleRail({ className, ...props }: ComponentProps<"div">) {
  return (
    <div
      className={cx("timescale-rail", className)}
      data-slot="timescale-rail"
      aria-hidden="true"
      {...props}
    />
  );
}

export function TimescaleItem({ className, ...props }: ComponentProps<"li">) {
  return <li className={cx("timescale-item", className)} data-slot="timescale-item" {...props} />;
}

export function TimescaleTick({ className, ...props }: ComponentProps<"span">) {
  return (
    <span
      className={cx("timescale-tick", className)}
      data-slot="timescale-tick"
      aria-hidden="true"
      {...props}
    />
  );
}

export function TimescaleAge({ className, ...props }: ComponentProps<"p">) {
  return <p className={cx("timescale-age", className)} data-slot="timescale-age" {...props} />;
}

export function TimescaleYear({ className, ...props }: ComponentProps<"p">) {
  return <p className={cx("timescale-year", className)} data-slot="timescale-year" {...props} />;
}

export function TimescaleContent({ className, ...props }: ComponentProps<"div">) {
  return (
    <div className={cx("timescale-content", className)} data-slot="timescale-content" {...props} />
  );
}
