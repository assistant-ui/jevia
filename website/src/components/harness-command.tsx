"use client";

import { useEffect, useState } from "react";

import { CopyButton } from "./copy-button";

const HARNESSES = [
  "codex",
  "claude",
  "opencode",
  "gemini",
  "cursor",
  "copilot",
  "aider",
  "goose",
  "amp",
] as const;
const TASK = "fix the flaky integration test";

interface Rotation {
  current: number;
  previous: number | null;
  cycle: number;
}

export function HarnessCommand() {
  const [paused, setPaused] = useState(false);
  const [rotation, setRotation] = useState<Rotation>({
    current: 0,
    previous: null,
    cycle: 0,
  });

  useEffect(() => {
    const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");
    let interval = 0;

    const stop = () => window.clearInterval(interval);
    const start = () => {
      stop();
      if (reducedMotion.matches || paused) return;

      interval = window.setInterval(() => {
        if (document.hidden) return;
        setRotation(({ current, cycle }) => ({
          current: (current + 1) % HARNESSES.length,
          previous: current,
          cycle: cycle + 1,
        }));
      }, 2400);
    };

    start();
    reducedMotion.addEventListener("change", start);
    return () => {
      stop();
      reducedMotion.removeEventListener("change", start);
    };
  }, [paused]);

  const harness = HARNESSES[rotation.current];
  const previous = rotation.previous === null ? null : HARNESSES[rotation.previous];
  const command = `jevia run ${harness} "${TASK}"`;

  return (
    <div
      className="command-block"
      data-compact="true"
      onPointerEnter={() => setPaused(true)}
      onPointerLeave={() => setPaused(false)}
      onFocus={() => setPaused(true)}
      onBlur={() => setPaused(false)}
    >
      <div className="command-line">
        <span className="command-prompt" aria-hidden="true">
          $
        </span>
        <code>
          <span className="sr-only">{command}</span>
          <span aria-hidden="true">
            <span className="shell-command">jevia</span>
            <span> run </span>
            <span className="harness-name-slot" data-cycle={rotation.cycle}>
              {previous && (
                <span
                  key={`previous-${rotation.cycle}`}
                  className="harness-name harness-name-exit"
                >
                  {previous}
                </span>
              )}
              <span
                key={`current-${rotation.cycle}`}
                className="harness-name harness-name-enter"
              >
                {harness}
              </span>
            </span>
            <span> </span>
            <span className="shell-string">&quot;{TASK}&quot;</span>
          </span>
        </code>
        <div className="command-actions">
          <CopyButton value={command} label="Launch harness" />
        </div>
      </div>
    </div>
  );
}
